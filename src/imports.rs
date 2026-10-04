use crate::config::Config;
use crate::{Error, Result};
use std::path::PathBuf;

pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn mount_source(config: &Config) -> Option<PathBuf> {
    config
        .values
        .imports
        .first()?
        .snapshot
        .as_ref()
        .map(|digest| {
            config
                .path
                .parent()
                .expect("config parent")
                .join("imports")
                .join(digest)
                .join("nvim")
        })
}

// Explicit unlock releases the shared open-file-description lock even if a
// concurrently spawned child temporarily inherited the descriptor across fork.
pub(crate) struct CreationLock(std::fs::File);

impl Drop for CreationLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(target_os = "linux")]
pub(crate) use linux::{creation_lock, plan, stage, verify};

#[cfg(not(target_os = "linux"))]
pub(crate) fn verify(config: &Config) -> Result<()> {
    if config.values.imports.is_empty() {
        Ok(())
    } else {
        Err(Error::new("snapshot imports require Linux"))
    }
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn creation_lock(config: &Config) -> Result<Option<CreationLock>> {
    verify(config)?;
    Ok(None)
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn stage(_: &Config, _: bool) -> Result<i32> {
    Err(Error::new("snapshot imports require Linux"))
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn plan(config: &Config) -> Result<serde_json::Value> {
    Ok(unsupported_plan(&config.values.imports))
}

#[cfg(any(not(target_os = "linux"), test))]
fn unsupported_plan(imports: &[crate::config::Import]) -> serde_json::Value {
    serde_json::json!(imports.iter().map(|import| serde_json::json!({
        "source": import.source,
        "destination": import.destination,
        "snapshot": import.snapshot,
        "artifact": "unsupported",
        "source_observation": "unsupported",
        "contents": null,
        "requested_contents": null,
        "contract": "read-only mounted contents only; writable guest ancestors permit pathname redirection/replacement",
        "detail": "snapshot imports require Linux"
    })).collect::<Vec<_>>())
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use crate::config::Import;
    use serde_json::json;

    #[test]
    fn unsupported_plan_preserves_empty_and_declared_array_shapes() {
        assert_eq!(unsupported_plan(&[]), json!([]));
        let import = Import {
            source: PathBuf::from("/synthetic/nvim"),
            destination: "/sandbox/.config/nvim".into(),
            snapshot: Some("a".repeat(64)),
        };
        let plan = unsupported_plan(std::slice::from_ref(&import));
        assert_eq!(plan.as_array().unwrap().len(), 1);
        assert_eq!(plan[0]["source"], json!(import.source));
        assert_eq!(plan[0]["destination"], import.destination);
        assert_eq!(plan[0]["snapshot"], json!(import.snapshot));
        assert_eq!(plan[0]["artifact"], "unsupported");
        assert_eq!(plan[0]["source_observation"], "unsupported");
        assert!(plan[0]["contents"].is_null());
        assert!(plan[0]["requested_contents"].is_null());
        assert_eq!(plan[0]["detail"], "snapshot imports require Linux");
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::config::{ConfigDoc, Import, read_json_text, reserved_import_name};
    use crate::export::{Entry, Tree};
    use data_encoding::HEXLOWER;
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::ffi::{CString, OsStr};
    use std::fs::{self, File};
    use std::io::{IsTerminal, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Component, Path};

    const MAX_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_FILE: u64 = 4 * 1024 * 1024;
    const MAX_MANIFEST: u64 = 1024 * 1024;
    const MAX_ENTRIES: usize = 4096;
    const MAX_DEPTH: usize = 32;

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Manifest {
        schema: u64,
        source: PathBuf,
        destination: String,
        paths: Tree,
    }

    fn error() -> Error {
        Error::new("unsafe, unavailable or changed import input/artifact")
    }

    fn name(value: &OsStr) -> Result<CString> {
        CString::new(value.as_bytes()).map_err(|_| error())
    }

    fn open(dir: &File, child: &OsStr, flags: i32, mode: u32) -> Result<File> {
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name(child)?.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                mode,
            )
        };
        if fd < 0 {
            return Err(error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn fd_path(file: &File) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
    }

    fn metadata(file: &File, directory: bool, private: bool) -> Result<()> {
        let m = file.metadata().map_err(|_| error())?;
        if (directory && !m.is_dir())
            || (!directory && (!m.is_file() || m.nlink() != 1))
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & if private { 0o077 } else { 0o022 } != 0
        {
            return Err(error());
        }
        Ok(())
    }

    fn directory(path: &Path, private: bool) -> Result<File> {
        if !path.is_absolute() {
            return Err(error());
        }
        let mut dir = File::open("/")?;
        for part in path.components() {
            let Component::Normal(part) = part else {
                if part == Component::RootDir {
                    continue;
                }
                return Err(error());
            };
            dir = open(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let m = dir.metadata()?;
            if (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() })
                || (m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & libc::S_ISVTX != 0))
            {
                return Err(error());
            }
        }
        metadata(&dir, true, private)?;
        Ok(dir)
    }

    fn mkdir(dir: &File, child: &str) -> Result<File> {
        if unsafe { libc::mkdirat(dir.as_raw_fd(), name(OsStr::new(child))?.as_ptr(), 0o700) } != 0
        {
            return Err(error());
        }
        open(
            dir,
            OsStr::new(child),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )
    }

    fn present(dir: &File, child: &str) -> Result<bool> {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let rc = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name(OsStr::new(child))?.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc == 0 {
            return Ok(true);
        }
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error())
        }
    }

    fn bytes(file: &mut File, limit: u64) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|_| error())?;
        if data.len() as u64 > limit {
            return Err(Error::new("import exceeds bounded size limit"));
        }
        Ok(data)
    }

    fn private_bytes(dir: &File, child: &str, limit: u64) -> Result<(File, Vec<u8>)> {
        let mut file = open(dir, OsStr::new(child), libc::O_RDONLY, 0)?;
        metadata(&file, false, true)?;
        let data = bytes(&mut file, limit)?;
        Ok((file, data))
    }

    fn write(dir: &File, child: &OsStr, data: &[u8], mode: u32) -> Result<()> {
        let mut file = open(
            dir,
            child,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            mode,
        )?;
        file.write_all(data)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        Ok(())
    }

    fn hash(data: &[u8]) -> String {
        HEXLOWER.encode(&Sha256::digest(data))
    }

    fn walk(
        dir: &File,
        output: Option<&File>,
        prefix: &str,
        depth: usize,
        tree: &mut Tree,
        total: &mut u64,
        frozen: bool,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::new("import exceeds depth limit"));
        }
        metadata(dir, true, frozen)?;
        if frozen && dir.metadata()?.mode() & 0o777 != 0o500 {
            return Err(error());
        }
        for item in fs::read_dir(fd_path(dir)).map_err(|_| error())? {
            let item = item.map_err(|_| error())?;
            let child = item.file_name();
            let child_text = child.to_str().ok_or_else(error)?;
            if child_text.contains(['\\', '\0']) || reserved_import_name(&child) {
                return Err(error());
            }
            let relative = if prefix.is_empty() {
                child_text.to_owned()
            } else {
                format!("{prefix}/{child_text}")
            };
            let mut file = open(dir, &child, libc::O_RDONLY, 0)?;
            let m = file.metadata()?;
            let entry = if m.is_dir() {
                let captured = output.map(|out| mkdir(out, child_text)).transpose()?;
                walk(
                    &file,
                    captured.as_ref(),
                    &relative,
                    depth + 1,
                    tree,
                    total,
                    frozen,
                )?;
                Entry::Dir
            } else {
                metadata(&file, false, frozen)?;
                if frozen && m.mode() & 0o777 != 0o400 {
                    return Err(error());
                }
                let data = bytes(&mut file, MAX_FILE)?;
                *total += data.len() as u64;
                if *total > MAX_BYTES {
                    return Err(Error::new("import exceeds total byte limit"));
                }
                if let Some(out) = output {
                    write(out, &child, &data, 0o400)?;
                }
                Entry::File {
                    sha256: hash(&data),
                    executable: false,
                }
            };
            if tree.len() >= MAX_ENTRIES || tree.insert(relative, entry).is_some() {
                return Err(Error::new("import exceeds entry limit"));
            }
        }
        Ok(())
    }

    fn inventory(dir: &File, output: Option<&File>, frozen: bool) -> Result<Tree> {
        let mut tree = Tree::new();
        walk(dir, output, "", 0, &mut tree, &mut 0, frozen)?;
        Ok(tree)
    }

    fn manifest(import: &Import, paths: Tree) -> Manifest {
        Manifest {
            schema: 1,
            source: import.source.clone(),
            destination: import.destination.clone(),
            paths,
        }
    }

    fn encoded(manifest: &Manifest) -> Result<Vec<u8>> {
        let data = serde_json::to_vec(&serde_json::to_value(manifest)?)?;
        if data.len() as u64 > MAX_MANIFEST {
            return Err(Error::new("import manifest exceeds 1 MiB"));
        }
        Ok(data)
    }

    fn artifact(config: &Config) -> Result<Manifest> {
        let import = config.values.imports.first().ok_or_else(error)?;
        let digest = import.snapshot.as_deref().ok_or_else(|| {
            Error::new("import not approved; run imports stage and review before runtime use")
        })?;
        let root = directory(
            &config
                .path
                .parent()
                .expect("config parent")
                .join("imports")
                .join(digest),
            true,
        )?;
        let (_, data) = private_bytes(&root, "manifest.json", MAX_MANIFEST)?;
        let value = read_json_text(std::str::from_utf8(&data).map_err(|_| error())?)
            .map_err(|_| error())?;
        let m: Manifest = serde_json::from_value(value).map_err(|_| error())?;
        if m.schema != 1
            || m.source != import.source
            || m.destination != import.destination
            || hash(&encoded(&m)?) != digest
        {
            return Err(error());
        }
        let nvim = open(
            &root,
            OsStr::new("nvim"),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        if inventory(&nvim, None, true)? != m.paths {
            return Err(Error::new(
                "selected import snapshot contents changed; refusing work launch",
            ));
        }
        Ok(m)
    }

    pub(crate) fn verify(config: &Config) -> Result<()> {
        if !config.values.imports.is_empty() {
            artifact(config)?;
        }
        Ok(())
    }

    fn locked(config: &Config) -> Result<(File, CreationLock)> {
        let parent = directory(config.path.parent().expect("config parent"), true)?;
        let lock = open(
            &parent,
            OsStr::new("imports.lock"),
            libc::O_RDWR | libc::O_CREAT,
            0o600,
        )?;
        metadata(&lock, false, true)?;
        lock.try_lock()
            .map_err(|_| Error::new("another import stage/create owns the environment lock"))?;
        let lock = CreationLock(lock);
        unchanged(config, &parent)?;
        Ok((parent, lock))
    }

    fn unchanged(config: &Config, parent: &File) -> Result<()> {
        let current = directory(config.path.parent().expect("config parent"), true)?;
        let a = parent.metadata()?;
        let b = current.metadata()?;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return Err(error());
        }
        let (_, data) = private_bytes(
            parent,
            config
                .path
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(error)?,
            MAX_MANIFEST,
        )?;
        let doc: ConfigDoc = serde_json::from_value(
            read_json_text(std::str::from_utf8(&data).map_err(|_| error())?)
                .map_err(|_| error())?,
        )
        .map_err(|_| error())?;
        if serde_json::to_value(&doc)? != serde_json::to_value(&config.values)? {
            return Err(Error::new(
                "configuration changed during import stage/create; retry explicitly",
            ));
        }
        let current = crate::config::load_config(&config.path)?;
        if current.fingerprint != config.fingerprint {
            return Err(Error::new(
                "policy/configuration changed during import stage/create",
            ));
        }
        Ok(())
    }

    pub(crate) fn creation_lock(config: &Config) -> Result<Option<CreationLock>> {
        if config.values.imports.is_empty() {
            return Ok(None);
        }
        let (_, lock) = locked(config)?;
        verify(config)?;
        Ok(Some(lock))
    }

    pub(crate) fn plan(config: &Config) -> Result<Value> {
        let Some(import) = config.values.imports.first() else {
            return Ok(json!([]));
        };
        let selected = artifact(config);
        let observed =
            directory(&import.source, false).and_then(|dir| inventory(&dir, None, false));
        let drift = match (&selected, &observed) {
            (Ok(m), Ok(tree)) if m.paths == *tree => "unchanged",
            (Ok(_), Ok(_)) => "drifted; selected snapshot remains frozen",
            (_, Err(_)) => "unavailable-or-unsafe; selected identity unchanged",
            _ => "not-staged",
        };
        Ok(json!([{
            "source": import.source, "destination": import.destination, "snapshot": import.snapshot,
            "artifact": if selected.is_ok() { "verified" } else { "missing-unapproved-or-changed" },
            "source_observation": drift,
            "contents": selected.as_ref().map(|m| &m.paths).ok(),
            "requested_contents": if import.snapshot.is_none() { observed.ok() } else { None },
            "contract": "read-only mounted contents only; writable guest ancestors permit pathname redirection/replacement"
        }]))
    }

    pub(crate) fn stage(config: &Config, yes: bool) -> Result<i32> {
        let import =
            config.values.imports.first().ok_or_else(|| {
                Error::new("declare one Neovim import in private dungeon.json first")
            })?;
        let preflight = directory(config.path.parent().expect("config parent"), true)?;
        if present(&preflight, "runtime.json")? {
            return Err(Error::new(
                "import refresh refused for any runtime binding; initialize a separate environment",
            ));
        }
        let (parent, _lock) = locked(config)?;
        if present(&parent, "runtime.json")? {
            return Err(Error::new(
                "import refresh refused for any runtime binding; initialize a separate environment",
            ));
        }
        let source = directory(&import.source, false)?;
        let stage = tempfile::Builder::new()
            .prefix("import-review-")
            .tempdir_in(fd_path(&parent))?;
        let staged = open(
            &parent,
            stage.path().file_name().ok_or_else(error)?,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        staged.set_permissions(fs::Permissions::from_mode(0o700))?;
        let nvim = mkdir(&staged, "nvim")?;
        let captured = manifest(import, inventory(&source, Some(&nvim), false)?);
        let data = encoded(&captured)?;
        let digest = hash(&data);
        write(&staged, OsStr::new("manifest.json"), &data, 0o600)?;
        let previous = if import.snapshot.is_some() {
            Some(artifact(config)?)
        } else {
            None
        };
        println!(
            "{}",
            crate::config::review_json(&json!({
                "source": import.source, "destination": import.destination, "snapshot": digest,
                "directories": captured.paths.values().filter(|e| matches!(e, Entry::Dir)).count(),
                "files": captured.paths.values().filter(|e| matches!(e, Entry::File { .. })).count(),
                "contents": captured.paths,
                "diff": diff(previous.as_ref().map(|m| &m.paths), &captured.paths),
                "capability": "one read-only snapshot bind; optional live project bind unchanged; local bind-enabled UID998 gateway required",
                "guest_state": ["/sandbox/.local/share/nvim", "/sandbox/.local/state/nvim", "/sandbox/.cache/nvim"],
                "warning": "Inspect all source content yourself: path checks are not a secret scanner. Lua executes only in guest under existing policy. Dungeon installs no tools/plugins and never widens policy. Mounted contents are read-only; guest ancestors and config pathname are mutable."
            }))?
        );
        if !yes {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return Err(Error::new(
                    "approval requires a terminal and typed yes, or explicit --yes",
                ));
            }
            print!("Approve this captured snapshot? Type yes: ");
            std::io::stdout().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if answer.trim() != "yes" {
                return Err(Error::new(
                    "snapshot approval cancelled; configuration unchanged",
                ));
            }
        }
        if inventory(&nvim, None, false)? != captured.paths {
            return Err(error());
        }
        unchanged(config, &parent)?;
        if present(&parent, "runtime.json")? {
            return Err(Error::new(
                "runtime binding appeared; import selection refused",
            ));
        }
        let imports = if present(&parent, "imports")? {
            open(
                &parent,
                OsStr::new("imports"),
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )?
        } else {
            mkdir(&parent, "imports")?
        };
        metadata(&imports, true, true)?;
        if present(&imports, &digest)? {
            return Err(Error::new(
                "snapshot digest already published; never overwritten; inspect or select existing approved artifact explicitly",
            ));
        }
        freeze(&nvim)?;
        let from = name(stage.path().file_name().ok_or_else(error)?)?;
        let to = name(OsStr::new(&digest))?;
        if unsafe {
            libc::renameat2(
                parent.as_raw_fd(),
                from.as_ptr(),
                imports.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(error());
        }
        // Published artifacts are retained even if selecting the config fails.
        let _ = stage.keep();
        unchanged(config, &parent)?;
        let (_, old) = private_bytes(
            &parent,
            config
                .path
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(error)?,
            MAX_MANIFEST,
        )?;
        let mut doc: Value =
            read_json_text(std::str::from_utf8(&old).map_err(|_| error())?).map_err(|_| error())?;
        doc["imports"][0]["snapshot"] = json!(digest);
        let replacement = tempfile::Builder::new()
            .prefix("import-config-")
            .tempfile_in(fd_path(&parent))?;
        replacement
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        replacement
            .as_file()
            .write_all(&serde_json::to_vec_pretty(&doc)?)?;
        replacement.as_file().sync_all()?;
        unchanged(config, &parent)?;
        if present(&parent, "runtime.json")? {
            return Err(error());
        }
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                name(replacement.path().file_name().ok_or_else(error)?)?.as_ptr(),
                parent.as_raw_fd(),
                name(config.path.file_name().ok_or_else(error)?)?.as_ptr(),
            )
        } != 0
        {
            return Err(error());
        }
        parent.sync_all()?;
        println!(
            "Snapshot approved and selected; no sandbox created. Published content is never refreshed in place."
        );
        Ok(0)
    }

    fn diff(previous: Option<&Tree>, current: &Tree) -> Value {
        let empty = Tree::new();
        let old = previous.unwrap_or(&empty);
        let names: std::collections::BTreeSet<_> = old.keys().chain(current.keys()).collect();
        json!(
            names
                .into_iter()
                .filter_map(|p| match (old.get(p), current.get(p)) {
                    (a, b) if a == b => None,
                    (None, Some(_)) => Some(json!({"path":p,"change":"add"})),
                    (Some(_), None) => Some(json!({"path":p,"change":"delete"})),
                    _ => Some(json!({"path":p,"change":"modify"})),
                })
                .collect::<Vec<_>>()
        )
    }

    fn freeze(dir: &File) -> Result<()> {
        for item in fs::read_dir(fd_path(dir))? {
            let item = item?;
            let file = open(dir, &item.file_name(), libc::O_RDONLY, 0)?;
            if file.metadata()?.is_dir() {
                freeze(&file)?;
            }
        }
        dir.set_permissions(fs::Permissions::from_mode(0o500))?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::config::{create_private_dir, load_config, write_new};
        use crate::policy::starter_policy;
        use std::os::unix::fs::symlink;

        struct Fixture {
            _root: tempfile::TempDir,
            source: PathBuf,
            config: Config,
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                fn writable(path: &Path) {
                    if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
                        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                        if let Ok(entries) = fs::read_dir(path) {
                            for entry in entries.flatten() {
                                writable(&entry.path());
                            }
                        }
                    }
                }
                writable(self._root.path());
            }
        }
        fn fixture() -> Fixture {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("nvim");
            let project = root.path().join("project");
            let private = root.path().join("private");
            for dir in [&source, &project, &private] {
                create_private_dir(dir).unwrap();
            }
            fs::write(source.join("init.lua"), "vim.g.synthetic = 42\n").unwrap();
            write_new(&private.join("policy.json"), &starter_policy()).unwrap();
            write_new(&private.join("dungeon.json"), &json!({"schema":2,"name":"test-import","gateway":"dungeon-local","workspace":"default","project":project,"image":"localhost/test:1","agent":"opencode","providers":[],"imports":[{"source":source,"destination":"/sandbox/.config/nvim"}]})).unwrap();
            let config = load_config(&private.join("dungeon.json")).unwrap();
            Fixture {
                _root: root,
                source,
                config,
            }
        }

        #[test]
        fn approved_snapshot_is_frozen_identity_not_source_and_tamper_blocks_work() {
            let f = fixture();
            assert!(verify(&f.config).is_err());
            stage(&f.config, true).unwrap();
            let approved = load_config(&f.config.path).unwrap();
            verify(&approved).unwrap();
            assert_ne!(approved.fingerprint, f.config.fingerprint);
            let path = mount_source(&approved).unwrap();
            assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o500);
            assert_eq!(
                fs::metadata(path.join("init.lua")).unwrap().mode() & 0o777,
                0o400
            );
            fs::write(f.source.join("init.lua"), "source changed").unwrap();
            assert_eq!(
                approved.fingerprint,
                load_config(&f.config.path).unwrap().fingerprint
            );
            verify(&approved).unwrap();
            assert!(
                plan(&approved).unwrap()[0]["source_observation"]
                    .as_str()
                    .unwrap()
                    .starts_with("drifted")
            );
            fs::remove_dir_all(&f.source).unwrap();
            symlink("/etc", &f.source).unwrap();
            verify(&approved).unwrap();
            assert_eq!(
                approved.fingerprint,
                load_config(&f.config.path).unwrap().fingerprint
            );
            let file = path.join("init.lua");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&file, "tampered").unwrap();
            assert!(verify(&approved).is_err());
        }

        #[test]
        fn approval_failure_and_bound_refresh_leave_authority_unchanged() {
            let f = fixture();
            let old = fs::read(&f.config.path).unwrap();
            assert!(stage(&f.config, false).is_err());
            assert_eq!(old, fs::read(&f.config.path).unwrap());
            let parent = f.config.path.parent().unwrap();
            assert!(!parent.join("imports").exists());
            assert!(!fs::read_dir(parent).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("import-review-")
            }));
            fs::remove_file(parent.join("imports.lock")).unwrap();
            fs::write(parent.join("runtime.json"), "invalid synthetic binding").unwrap();
            assert!(
                stage(&f.config, true)
                    .unwrap_err()
                    .to_string()
                    .contains("runtime binding")
            );
            assert!(!parent.join("imports.lock").exists());
            assert_eq!(old, fs::read(&f.config.path).unwrap());
        }

        #[test]
        fn capture_refuses_links_special_reserved_permissions_and_bounds() {
            for kind in [
                "symlink", "hardlink", "fifo", "reserved", "writable", "large", "depth",
            ] {
                let f = fixture();
                let p = f.source.join("unsafe");
                match kind {
                    "symlink" => symlink("init.lua", &p).unwrap(),
                    "hardlink" => fs::hard_link(f.source.join("init.lua"), &p).unwrap(),
                    "fifo" => {
                        assert_eq!(
                            unsafe { libc::mkfifo(name(p.as_os_str()).unwrap().as_ptr(), 0o600) },
                            0
                        );
                    }
                    "reserved" => fs::write(f.source.join("auth.json"), "synthetic").unwrap(),
                    "writable" => fs::set_permissions(
                        f.source.join("init.lua"),
                        fs::Permissions::from_mode(0o666),
                    )
                    .unwrap(),
                    "large" => File::create(p).unwrap().set_len(MAX_FILE + 1).unwrap(),
                    "depth" => {
                        let mut p = f.source.clone();
                        for _ in 0..MAX_DEPTH + 1 {
                            p.push("d");
                            fs::create_dir(&p).unwrap();
                        }
                    }
                    _ => unreachable!(),
                }
                assert!(stage(&f.config, true).is_err(), "{kind}");
                assert!(
                    load_config(&f.config.path).unwrap().values.imports[0]
                        .snapshot
                        .is_none()
                );
            }
        }

        #[test]
        fn manifest_is_strict_and_lock_rechecks_authority() {
            let f = fixture();
            let (_, lock) = locked(&f.config).unwrap();
            assert!(creation_lock(&f.config).is_err());
            assert!(stage(&f.config, true).is_err());
            drop(lock);
            stage(&f.config, true).unwrap();
            assert!(creation_lock(&f.config).is_err()); // stale config, never create from it
            let approved = load_config(&f.config.path).unwrap();
            assert!(creation_lock(&approved).unwrap().is_some());
            let path = mount_source(&approved)
                .unwrap()
                .parent()
                .unwrap()
                .join("manifest.json");
            let original = fs::read_to_string(&path).unwrap();
            for text in [
                original.replacen("{", "{\"schema\":1,", 1),
                original.replacen("{", "{\"unexpected\":true,", 1),
                original.replace("init.lua", "../init.lua"),
            ] {
                fs::write(&path, text).unwrap();
                assert!(verify(&approved).is_err());
            }
        }

        #[test]
        fn canonical_manifest_and_diff_are_deterministic_and_digest_closed() {
            assert!(valid_digest(&"a".repeat(64)));
            assert!(!valid_digest(&"A".repeat(64)));
            assert!(!valid_digest("../escape"));
            let f = fixture();
            let dir = directory(&f.source, false).unwrap();
            let a = manifest(
                &f.config.values.imports[0],
                inventory(&dir, None, false).unwrap(),
            );
            let b = manifest(
                &f.config.values.imports[0],
                inventory(&dir, None, false).unwrap(),
            );
            assert_eq!(encoded(&a).unwrap(), encoded(&b).unwrap());
            assert_eq!(diff(Some(&a.paths), &b.paths), json!([]));
        }

        #[test]
        fn native_state_names_are_refused_and_review_names_are_json_escaped() {
            for filename in [
                "sessions",
                "auth.sqlite",
                "state.sqlite3",
                "session.db",
                "opencode",
                ".mozilla",
            ] {
                let f = fixture();
                fs::write(f.source.join(filename), "synthetic").unwrap();
                assert!(stage(&f.config, true).is_err(), "{filename}");
            }
            let f = fixture();
            fs::write(f.source.join("name\n\u{1b}[31m.lua"), "synthetic").unwrap();
            let text = serde_json::to_string(&plan(&f.config).unwrap()).unwrap();
            assert!(!text.contains('\u{1b}'));
            assert!(text.contains("\\n\\u001b"));
            let review = crate::config::review_json(
                &json!({"path":"name\u{009b}\u{202e}\u{2066}\u{1f600}.lua"}),
            )
            .unwrap();
            assert!(review.is_ascii());
            assert_eq!(
                serde_json::from_str::<Value>(&review).unwrap()["path"],
                "name\u{009b}\u{202e}\u{2066}\u{1f600}.lua"
            );
            stage(&f.config, true).unwrap();
            verify(&load_config(&f.config.path).unwrap()).unwrap();
        }

        #[test]
        fn total_entry_and_manifest_limits_refuse_before_publication() {
            for kind in ["total", "entries", "manifest"] {
                let f = fixture();
                match kind {
                    "total" => {
                        for i in 0..9 {
                            File::create(f.source.join(format!("file-{i}")))
                                .unwrap()
                                .set_len(MAX_FILE)
                                .unwrap();
                        }
                    }
                    "entries" => {
                        for i in 0..MAX_ENTRIES {
                            fs::write(f.source.join(format!("file-{i}")), "").unwrap();
                        }
                    }
                    "manifest" => {
                        for i in 0..3500 {
                            fs::write(f.source.join(format!("{i:04}-{}.lua", "x".repeat(220))), "")
                                .unwrap();
                        }
                    }
                    _ => unreachable!(),
                }
                assert!(stage(&f.config, true).is_err(), "{kind}");
                assert!(!f.config.path.parent().unwrap().join("imports").exists());
                assert!(
                    load_config(&f.config.path).unwrap().values.imports[0]
                        .snapshot
                        .is_none()
                );
            }
        }

        #[test]
        fn dropping_lock_releases_it_while_fork_child_retains_descriptor() {
            let f = fixture();
            let (_, lock) = locked(&f.config).unwrap();
            let mut pipe = [0; 2];
            assert_eq!(
                unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC) },
                0
            );
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                unsafe {
                    libc::close(pipe[1]);
                    let mut byte = 0u8;
                    libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
                    libc::_exit(0);
                }
            }
            unsafe {
                libc::close(pipe[0]);
            }
            drop(lock);
            let reacquired = locked(&f.config);
            unsafe {
                libc::close(pipe[1]);
                libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            assert!(reacquired.is_ok());
        }
    }
}
