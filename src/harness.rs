//! Harness-specific imports under the existing import authority and lock.
use crate::config::{Agent, Config, HarnessConfig, HarnessMode};
use crate::{Error, Result};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

const CONFIG_TARGET: &str = "/sandbox/.config/opencode";
const AUTH_TARGET: &str = "/sandbox/.local/share/opencode/auth.json";
const CONFIG_FILES: &[&str] = &["opencode.json", "opencode.jsonc", "cli.json", "AGENTS.md"];
const CONFIG_DIRS: &[&str] = &[
    "plugins", "plugin", "themes", "skills", "skill", "agents", "agent", "commands", "command",
    "tools", "tool",
];

fn home() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("HOME or USERPROFILE is required for harness paths"))?;
    if !home.is_absolute() {
        return Err(Error::new("harness HOME must be absolute"));
    }
    Ok(home)
}
pub(crate) const WARNING: &str = "Read-only prevents host writes, not credential reading, use or exfiltration. Config/plugins execute in the guest. OpenCode 2.1.2 database credentials and sessions are NOT shared; readable legacy auth.json does not establish usable guest authentication. Original-source imports are refused on enforcing SELinux hosts; choose copy explicitly. Read-only auth may prevent login refresh, and snapshots may become stale. Host-only paths are not remapped. No network policy is widened.";

pub(crate) fn enabled(config: &Config) -> bool {
    config
        .values
        .harness
        .as_ref()
        .is_some_and(|h| h.mode != HarnessMode::Disabled)
}

fn relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && Path::new(path)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
}

fn forbidden(path: &str) -> bool {
    Path::new(path).components().any(|p| {
        let name = p.as_os_str();
        // The harness root itself is allowed, never a nested native state tree.
        crate::config::reserved_import_name(name) || name == ".git" || name == "node_modules"
    })
}

fn selected(h: &HarnessConfig, path: &str) -> bool {
    if !relative(path) || forbidden(path) {
        return false;
    }
    let first = path.split('/').next().unwrap_or_default();
    if !CONFIG_FILES.contains(&path) && !CONFIG_DIRS.contains(&first) {
        return false;
    }
    if h.exclude_provider_auth && ["opencode.json", "opencode.jsonc", "cli.json"].contains(&path) {
        return false;
    }
    !h.exclude
        .iter()
        .any(|excluded| path == excluded || path.starts_with(&format!("{excluded}/")))
}

fn allowed_file(h: &HarnessConfig, file: &str) -> bool {
    if file == "auth.json" {
        !h.exclude_provider_auth
    } else {
        file.strip_prefix("config/").is_some_and(|p| selected(h, p))
    }
}

pub(crate) fn validate(h: &HarnessConfig, project: &Path, config: &Path) -> Result<()> {
    if h.agent != Agent::Opencode {
        return Err(Error::new(
            "harness configuration currently supports OpenCode only; Codex then Claude Code follow separately",
        ));
    }
    for source in [&h.source, &h.legacy_auth] {
        if !source.is_absolute()
            || source.as_os_str().len() > 4096
            || source == Path::new("/")
            || source == &home()?
            || source.components().any(|c| {
                !matches!(c, Component::RootDir | Component::Normal(_))
                    || c.as_os_str()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".env")
                    || c.as_os_str()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .starts_with(".env.")
            })
            || source
                .to_string_lossy()
                .split('/')
                .any(|p| p == "." || p == "..")
            || source.starts_with(project)
            || project.starts_with(source)
            || source.starts_with(config.parent().expect("config parent"))
            || config.starts_with(source)
        {
            return Err(Error::new(
                "harness sources must be specific absolute paths outside the project/private config, without links or traversal",
            ));
        }
    }
    if h.legacy_auth.file_name().and_then(|p| p.to_str()) != Some("auth.json") {
        return Err(Error::new(
            "OpenCode legacy auth source must name auth.json",
        ));
    }
    let mut paths = BTreeSet::new();
    for path in &h.exclude {
        if path.len() > 4096 || !relative(path) || !paths.insert(path) {
            return Err(Error::new(
                "harness exclusions must be unique relative paths, not globs or traversal",
            ));
        }
    }
    paths.clear();
    for file in &h.files {
        if file.len() > 4096 || !allowed_file(h, file) || !paths.insert(file) || paths.len() > 4096
        {
            return Err(Error::new(
                "invalid, duplicate, excluded or excessive selected harness file",
            ));
        }
    }
    if h.snapshot
        .as_deref()
        .is_some_and(|s| !super::valid_digest(s))
        || (h.snapshot.is_some() && h.mode != HarnessMode::Copy)
    {
        return Err(Error::new(
            "only copy mode accepts a SHA-256 harness snapshot",
        ));
    }
    Ok(())
}

pub(crate) fn defaults(
    agent: Agent,
    mode: HarnessMode,
    exclude_provider_auth: bool,
    exclude: Vec<String>,
    project: &Path,
    config: &Path,
) -> Result<Option<HarnessConfig>> {
    if agent != Agent::Opencode {
        return Ok(None);
    }
    let home = home()?;
    let root = |key: &str, fallback: &str| -> Result<PathBuf> {
        let root = std::env::var_os(key)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(fallback));
        if !root.is_absolute() {
            return Err(Error::new("harness XDG roots must be absolute"));
        }
        Ok(root.join("opencode"))
    };
    let mut h = HarnessConfig {
        agent,
        mode,
        source: root("XDG_CONFIG_HOME", ".config")?,
        legacy_auth: root("XDG_DATA_HOME", ".local/share")?.join("auth.json"),
        exclude_provider_auth,
        exclude,
        files: vec![],
        snapshot: None,
    };
    validate(&h, project, config)?;
    if mode == HarnessMode::ReadOnly && cfg!(target_os = "linux") {
        h.files = discover(&h)?;
    }
    Ok(Some(h))
}

pub(crate) fn mounts(config: &Config) -> Vec<Value> {
    let Some(h) = &config.values.harness else {
        return vec![];
    };
    if h.mode == HarnessMode::Disabled {
        return vec![];
    }
    h.files
        .iter()
        .map(|file| {
            let source = if h.mode == HarnessMode::Copy {
                config
                    .path
                    .parent()
                    .expect("config parent")
                    .join("harness")
                    .join(h.snapshot.as_deref().unwrap_or(
                        "0000000000000000000000000000000000000000000000000000000000000000",
                    ))
                    .join(file)
            } else {
                source(h, file)
            };
            let target = file
                .strip_prefix("config/")
                .map(|p| format!("{CONFIG_TARGET}/{p}"))
                .unwrap_or_else(|| AUTH_TARGET.into());
            let mut mount =
                json!({"type":"bind", "source":source, "target":target, "read_only":true});
            // Never relabel original host files; only Dungeon-owned copies may be relabeled.
            if h.mode == HarnessMode::Copy {
                mount["selinux_label"] = json!("private");
            }
            mount
        })
        .collect()
}

fn source(h: &HarnessConfig, file: &str) -> PathBuf {
    file.strip_prefix("config/")
        .map(|p| h.source.join(p))
        .unwrap_or_else(|| h.legacy_auth.clone())
}

pub(crate) fn plan(config: &Config) -> Value {
    let Some(h) = &config.values.harness else {
        return Value::Null;
    };
    json!({
        "configuration":h,
        "mounts":mounts(config),
        "warning":WARNING,
        "auth_exclusion": "exclude_provider_auth omits legacy auth.json and entire opencode.json/jsonc/cli.json files, not a secret scanner for arbitrary code or other files",
        "refresh": "reload reselects unbound files/snapshots only; bound runtime refresh is refused; live file replacement may leave an old inode mounted",
        "selinux": "nonempty original-source imports refuse enforcing or unreadable/invalid SELinux enforcement state before runtime contact; choose copy explicitly, never relabel originals or weaken enforcement",
        "native_paths": "selected image must supply empty guest-owned config/data/state/cache parents under /sandbox; external file:// plugins, host commands, environment and paths are not imported",
        "source_observation": "not read; plan never inventories or hashes harness source contents",
        "verification": "offline declaration only; not runtime or authentication validation"
    })
}

#[cfg(target_os = "linux")]
pub(crate) use linux::{discover, reload, verify};

#[cfg(not(target_os = "linux"))]
pub(crate) fn discover(_: &HarnessConfig) -> Result<Vec<String>> {
    Err(Error::new("harness imports require Linux"))
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn verify(config: &Config) -> Result<()> {
    if enabled(config) {
        Err(Error::new("harness imports require Linux"))
    } else {
        Ok(())
    }
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn reload(_: &Config, _: bool) -> Result<i32> {
    Err(Error::new("harness imports require Linux"))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::super::linux::{
        bytes, directory, error, fd_path, hash, locked, metadata, mkdir, name, open, present,
        private_bytes, unchanged, write,
    };
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::fs::{self, File};
    use std::io::{IsTerminal, Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    const MAX_FILE: u64 = 4 * 1024 * 1024;
    const MAX_BYTES: u64 = 32 * 1024 * 1024;
    const MAX_MANIFEST: u64 = 1024 * 1024;

    fn optional(path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(error()),
        }
    }

    fn source_file(h: &HarnessConfig, file: &str) -> Result<File> {
        let path = source(h, file);
        let dir = directory(path.parent().ok_or_else(error)?, false)?;
        let file = open(&dir, path.file_name().ok_or_else(error)?, libc::O_RDONLY, 0)?;
        metadata(&file, false, false)?;
        if file.metadata()?.len() > MAX_FILE {
            return Err(error());
        }
        Ok(file)
    }

    fn scan(
        h: &HarnessConfig,
        dir: &File,
        prefix: &str,
        depth: usize,
        files: &mut Vec<String>,
        visited: &mut usize,
    ) -> Result<()> {
        if depth > 32 {
            return Err(error());
        }
        metadata(dir, true, false)?;
        for item in fs::read_dir(fd_path(dir))? {
            *visited += 1;
            if *visited > 8192 {
                return Err(Error::new("harness inventory exceeds entry limit"));
            }
            let child = item?.file_name();
            let child_text = child.to_str().ok_or_else(error)?;
            let path = if prefix.is_empty() {
                child_text.into()
            } else {
                format!("{prefix}/{child_text}")
            };
            // Exclude before opening: .env and other forbidden inputs are never read.
            if !selected(h, &path) {
                continue;
            }
            let entry = open(dir, &child, libc::O_RDONLY, 0)?;
            if entry.metadata()?.is_dir() {
                scan(h, &entry, &path, depth + 1, files, visited)?;
            } else {
                metadata(&entry, false, false)?;
                if entry.metadata()?.len() > MAX_FILE || files.len() >= 4096 {
                    return Err(error());
                }
                files.push(format!("config/{path}"));
            }
        }
        Ok(())
    }

    pub(crate) fn discover(h: &HarnessConfig) -> Result<Vec<String>> {
        let mut files = Vec::new();
        if optional(&h.source)? {
            scan(h, &directory(&h.source, false)?, "", 0, &mut files, &mut 0)?;
        }
        if !h.exclude_provider_auth && optional(&h.legacy_auth)? {
            source_file(h, "auth.json")?;
            if files.len() >= 4096 {
                return Err(error());
            }
            files.push("auth.json".into());
        }
        files.sort();
        Ok(files)
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Manifest {
        schema: u64,
        source: PathBuf,
        legacy_auth: PathBuf,
        exclude_provider_auth: bool,
        exclude: Vec<String>,
        files: BTreeMap<String, String>,
    }

    fn encoded(m: &Manifest) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&serde_json::to_value(m)?)?;
        if bytes.len() as u64 > MAX_MANIFEST {
            return Err(error());
        }
        Ok(bytes)
    }

    fn child_file(root: &File, path: &str) -> Result<File> {
        let mut dir = root.try_clone()?;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let file = open(&dir, OsStr::new(part), libc::O_RDONLY, 0)?;
            if parts.peek().is_none() {
                return Ok(file);
            }
            metadata(&file, true, true)?;
            dir = file;
        }
        Err(error())
    }

    fn original_source_compatibility(state: std::io::Result<Vec<u8>>) -> Result<()> {
        match state {
            Ok(bytes) if bytes == b"0" => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(bytes) if bytes == b"1" => Err(Error::new(
                "original-source harness imports are unsupported with enforcing SELinux; initialize a separate environment with --harness-config copy and approve reload, or select disabled; originals are never relabeled and no fallback is automatic",
            )),
            _ => Err(Error::new(
                "cannot establish SELinux compatibility for original-source harness imports; choose explicitly reviewed copy or disabled mode; enforcement is never weakened",
            )),
        }
    }

    fn verify_original_sources(h: &HarnessConfig) -> Result<()> {
        if !h.files.is_empty() {
            let state = (|| {
                let mut bytes = Vec::new();
                File::open("/sys/fs/selinux/enforce")?
                    .take(2)
                    .read_to_end(&mut bytes)?;
                Ok(bytes)
            })();
            original_source_compatibility(state)?;
        }
        for file in &h.files {
            source_file(h, file)?;
        }
        Ok(())
    }

    pub(crate) fn verify(config: &Config) -> Result<()> {
        let Some(h) = &config.values.harness else {
            return Ok(());
        };
        match h.mode {
            HarnessMode::Disabled => Ok(()),
            HarnessMode::ReadOnly => verify_original_sources(h),
            HarnessMode::Copy => {
                let digest = h.snapshot.as_deref().ok_or_else(|| {
                    Error::new("harness snapshot not approved; run reload before first up")
                })?;
                let root = directory(
                    &config
                        .path
                        .parent()
                        .expect("config parent")
                        .join("harness")
                        .join(digest),
                    true,
                )?;
                let (_, data) = private_bytes(&root, "manifest.json", MAX_MANIFEST)?;
                let m: Manifest = serde_json::from_value(
                    crate::config::read_json_text(std::str::from_utf8(&data).map_err(|_| error())?)
                        .map_err(|_| error())?,
                )
                .map_err(|_| error())?;
                if m.schema != 1
                    || m.source != h.source
                    || m.legacy_auth != h.legacy_auth
                    || m.exclude_provider_auth != h.exclude_provider_auth
                    || m.exclude != h.exclude
                    || m.files.keys().collect::<BTreeSet<_>>()
                        != h.files.iter().collect::<BTreeSet<_>>()
                    || hash(&encoded(&m)?) != digest
                {
                    return Err(error());
                }
                for (path, digest) in &m.files {
                    if !allowed_file(h, path) {
                        return Err(error());
                    }
                    let mut file = child_file(&root, path)?;
                    metadata(&file, false, true)?;
                    if file.metadata()?.mode() & 0o777 != 0o400
                        || hash(&bytes(&mut file, MAX_FILE)?) != *digest
                    {
                        return Err(error());
                    }
                }
                Ok(())
            }
        }
    }

    fn capture(root: &File, path: &str, data: &[u8]) -> Result<()> {
        let mut dir = root.try_clone()?;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return write(&dir, OsStr::new(part), data, 0o400);
            }
            dir = if present(&dir, part)? {
                let next = open(
                    &dir,
                    OsStr::new(part),
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                metadata(&next, true, true)?;
                next
            } else {
                mkdir(&dir, part)?
            };
        }
        Err(error())
    }

    pub(crate) fn reload(config: &Config, yes: bool) -> Result<i32> {
        let h = config
            .values
            .harness
            .as_ref()
            .ok_or_else(|| Error::new("no harness configuration declared"))?;
        if h.mode == HarnessMode::Disabled {
            return Err(Error::new("harness configuration is disabled"));
        }
        let parent = directory(config.path.parent().expect("config parent"), true)?;
        if present(&parent, "runtime.json")? {
            return Err(Error::new(
                "reload refused for a bound sandbox; preserve guest auth/sessions and initialize a separate environment; active refresh is not implemented",
            ));
        }
        let (parent, _lock) = locked(config)?;
        if present(&parent, "runtime.json")? {
            return Err(error());
        }
        let files = discover(h)?;
        let mut next = h.clone();
        next.files = files;
        next.snapshot = None;
        let mut candidate = config.clone();
        candidate.values.harness = Some(next.clone());
        crate::openshell::validate_mount_request(&candidate)?;
        let stage = tempfile::Builder::new()
            .prefix("harness-review-")
            .tempdir_in(fd_path(&parent))?;
        let staged = open(
            &parent,
            stage.path().file_name().ok_or_else(error)?,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        staged.set_permissions(fs::Permissions::from_mode(0o700))?;
        if h.mode == HarnessMode::Copy {
            let mut hashes = BTreeMap::new();
            let mut total = 0;
            for path in &next.files {
                let data = bytes(&mut source_file(h, path)?, MAX_FILE)?;
                total += data.len() as u64;
                if total > MAX_BYTES {
                    return Err(error());
                }
                capture(&staged, path, &data)?;
                hashes.insert(path.clone(), hash(&data));
            }
            let manifest = Manifest {
                schema: 1,
                source: h.source.clone(),
                legacy_auth: h.legacy_auth.clone(),
                exclude_provider_auth: h.exclude_provider_auth,
                exclude: h.exclude.clone(),
                files: hashes,
            };
            let data = encoded(&manifest)?;
            next.snapshot = Some(hash(&data));
            write(&staged, OsStr::new("manifest.json"), &data, 0o600)?;
        }
        println!(
            "{}",
            crate::config::review_json(
                &json!({"harness":next, "warning":WARNING, "effect":"select files for an unbound environment only; no runtime contact or guest auth refresh"})
            )?
        );
        if !yes {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                return Err(Error::new(
                    "reload approval requires a terminal and typed yes, or --yes",
                ));
            }
            print!("Approve this harness configuration selection? Type yes: ");
            std::io::stdout().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if answer.trim() != "yes" {
                return Err(Error::new("reload cancelled; configuration unchanged"));
            }
        }
        unchanged(config, &parent)?;
        if present(&parent, "runtime.json")? {
            return Err(error());
        }
        if let Some(digest) = &next.snapshot {
            let artifacts = if present(&parent, "harness")? {
                let dir = open(
                    &parent,
                    OsStr::new("harness"),
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                metadata(&dir, true, true)?;
                dir
            } else {
                mkdir(&parent, "harness")?
            };
            if !present(&artifacts, digest)? {
                if unsafe {
                    libc::renameat2(
                        parent.as_raw_fd(),
                        name(stage.path().file_name().ok_or_else(error)?)?.as_ptr(),
                        artifacts.as_raw_fd(),
                        name(OsStr::new(digest))?.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                } != 0
                {
                    return Err(error());
                }
                let _ = stage.keep();
                artifacts.sync_all()?;
            }
            let mut candidate = config.clone();
            candidate.values.harness = Some(next.clone());
            verify(&candidate)?;
        } else {
            for path in &next.files {
                source_file(&next, path)?;
            }
        }
        let mut values = config.values.clone();
        values.harness = Some(next);
        crate::config::load_config_data(
            config.path.clone(),
            serde_json::to_value(&values)?,
            |path| Ok((path.to_path_buf(), config.policy.clone())),
        )?;
        let replacement = tempfile::Builder::new()
            .prefix("harness-config-")
            .tempfile_in(fd_path(&parent))?;
        replacement
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        replacement
            .as_file()
            .write_all(&serde_json::to_vec_pretty(&values)?)?;
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
            "Harness selection approved; no sandbox changed. Guest authentication refresh is not implied."
        );
        Ok(0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Error as IoError, ErrorKind};

        #[test]
        fn original_source_selinux_gate_is_conservative_and_sanitized() {
            assert!(original_source_compatibility(Ok(b"0".to_vec())).is_ok());
            assert!(original_source_compatibility(Err(IoError::from(ErrorKind::NotFound))).is_ok());
            let denied = original_source_compatibility(Ok(b"1".to_vec())).unwrap_err();
            assert!(denied.to_string().contains("enforcing SELinux"));
            assert!(denied.to_string().contains("--harness-config copy"));
            for bytes in [b"".as_slice(), b"2", b"10", b"0\n", b"synthetic-untrusted"] {
                let error = original_source_compatibility(Ok(bytes.to_vec())).unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("cannot establish SELinux compatibility")
                );
                assert!(!error.to_string().contains("synthetic-untrusted"));
            }
            for kind in [
                ErrorKind::PermissionDenied,
                ErrorKind::Interrupted,
                ErrorKind::Other,
            ] {
                assert!(
                    original_source_compatibility(Err(IoError::new(kind, "synthetic-untrusted")))
                        .is_err()
                );
            }
        }
    }
}
