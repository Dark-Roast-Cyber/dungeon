use crate::config::{Config, read_json, read_json_text};
use crate::{Error, Result};
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};

pub(crate) struct TransferGuard {
    locks: Vec<File>,
    _directories: Vec<File>,
}

impl Drop for TransferGuard {
    fn drop(&mut self) {
        // fork may retain the open file description even with CLOEXEC.
        for file in &self.locks {
            let _ = file.unlock();
        }
    }
}

impl TransferGuard {
    pub(crate) fn acquire(config: &Config) -> Result<Self> {
        let mut guard = Self::environment(config)?;
        guard.project(config)?;
        Ok(guard)
    }

    pub(crate) fn project(&mut self, config: &Config) -> Result<()> {
        let path = crate::binding::transfer_lock_path(&config.values.project)?;
        #[cfg(test)]
        let path = config
            .values
            .project
            .parent()
            .unwrap()
            .join("transfer-locks")
            .join(path.file_name().unwrap());
        self.project_at(config, &path)
    }

    #[cfg(not(unix))]
    pub(crate) fn environment(_: &Config) -> Result<Self> {
        // Non-Unix creation retains its existing contract; copy transfer refuses below.
        Ok(Self {
            locks: vec![],
            _directories: vec![],
        })
    }

    #[cfg(not(unix))]
    fn project_at(&mut self, _: &Config, _: &Path) -> Result<()> {
        Err(Error::refusal(
            crate::ErrorCode::Unsupported,
            "private copy-transfer locking requires a Unix host",
        ))
    }

    #[cfg(unix)]
    pub(crate) fn environment(config: &Config) -> Result<Self> {
        use private::*;
        let parent = directory(config.path.parent().expect("config parent"), false)?;
        let mut guard = Self {
            locks: vec![],
            _directories: vec![parent],
        };
        guard
            .locks
            .push(lock(&guard._directories[0], "transfer.lock")?);
        guard.verify_environment(config)?;
        Ok(guard)
    }

    #[cfg(unix)]
    pub(crate) fn verify_environment(&self, config: &Config) -> Result<()> {
        use private::*;
        let current = directory(config.path.parent().unwrap(), false)?;
        use std::os::unix::fs::MetadataExt;
        let a = current.metadata()?;
        let b = self._directories[0].metadata()?;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return Err(Error::new(
                "configuration directory changed during transfer",
            ));
        }
        let dir = &self._directories[0];
        let mut values: crate::config::ConfigDoc =
            serde_json::from_value(json_file(dir, config.path.file_name().unwrap())?)
                .map_err(|_| Error::new("invalid private transfer configuration"))?;
        if values.project.exists() {
            values.project = crate::config::canonical_project(&values.project)?;
        }
        let policy = json_file(dir, "policy.json")?;
        if serde_json::to_value(values)? != serde_json::to_value(&config.values)?
            || policy != config.policy
        {
            return Err(Error::new(
                "policy/configuration changed before transfer; retry explicitly",
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn verify_environment(&self, _: &Config) -> Result<()> {
        Ok(())
    }

    #[cfg(unix)]
    fn project_at(&mut self, config: &Config, project_lock: &Path) -> Result<()> {
        use private::*;
        self.verify_environment(config)?;
        if crate::config::canonical_project(&config.values.project)? != config.values.project {
            return Err(Error::new("project identity changed before transfer"));
        }
        if project_lock.starts_with(&config.values.project) {
            return Err(Error::new("transfer locks must stay outside the project"));
        }
        let authority = directory(project_lock.parent().unwrap().parent().unwrap(), true)?;
        let root = directory(project_lock.parent().expect("lock parent"), true)?;
        self.locks
            .push(lock(&root, project_lock.file_name().unwrap())?);
        let current = directory(project_lock.parent().unwrap(), false)?;
        use std::os::unix::fs::MetadataExt;
        let a = current.metadata()?;
        let b = root.metadata()?;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return Err(Error::new("project lock directory changed during transfer"));
        }
        self._directories.push(root);
        self._directories.push(authority);
        check_recovery(&config.values.project)?;
        Ok(())
    }

    pub(crate) fn runtime_state(&self, config: &Config) -> Result<crate::config::RuntimeState> {
        #[cfg(unix)]
        {
            self.verify_environment(config)?;
            Ok(serde_json::from_value(private::json_file(
                &self._directories[0],
                "runtime.json",
            )?)?)
        }
        #[cfg(not(unix))]
        {
            crate::config::read_runtime_state(&config.state_path())
        }
    }

    pub(crate) fn complete_initial_upload(
        &self,
        config: &Config,
        expected: &crate::config::RuntimeState,
    ) -> Result<()> {
        self.complete_initial_upload_with(
            config,
            expected,
            |temp, path| {
                temp.persist(path).map_err(|e| Error::new(e.to_string()))?;
                Ok(())
            },
            |parent| Ok(File::open(parent)?.sync_all()?),
        )
    }

    fn complete_initial_upload_with(
        &self,
        config: &Config,
        expected: &crate::config::RuntimeState,
        replace: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<()>,
        sync: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<()> {
        if !expected.initial_upload_pending || self.runtime_state(config)? != *expected {
            return Err(Error::new(
                "initial upload binding changed before completion",
            ));
        }
        let parent = config.path.parent().unwrap();
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        let completed = crate::config::RuntimeState {
            initial_upload_pending: false,
            ..expected.clone()
        };
        serde_json::to_writer_pretty(&mut temp, &completed)?;
        temp.write_all(b"\n")?;
        temp.as_file().sync_all()?;
        // All transfer/baseline checks precede publication. A later directory-sync
        // failure may leave this completed state visible; it must not be rolled back.
        if self.runtime_state(config)? != *expected {
            return Err(Error::new(
                "initial upload binding changed before completion",
            ));
        }
        replace(temp, &config.state_path())?;
        sync(parent).map_err(|_| Error::new("initial upload completed but state persistence unconfirmed; inspect before continuing"))
    }
}

#[cfg(unix)]
mod private {
    use super::*;
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};

    fn invalid() -> Error {
        Error::new("unsafe private transfer state; inspect manually")
    }

    pub(super) fn open(dir: &File, name: impl AsRef<OsStr>, flags: i32, mode: u32) -> Result<File> {
        let name = CString::new(name.as_ref().as_bytes()).map_err(|_| invalid())?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                mode,
            )
        };
        if fd < 0 {
            return Err(invalid());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn metadata(file: &File, dir: bool) -> Result<()> {
        let m = file.metadata()?;
        if m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o7777 != if dir { 0o700 } else { 0o600 }
            || if dir {
                !m.is_dir()
            } else {
                !m.is_file() || m.nlink() != 1
            }
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(super) fn directory(path: &Path, create: bool) -> Result<File> {
        if !path.is_absolute() {
            return Err(invalid());
        }
        let mut dir = File::open("/")?;
        for part in path.components() {
            let Component::Normal(part) = part else {
                if part == Component::RootDir {
                    continue;
                }
                return Err(invalid());
            };
            if create {
                let name = CString::new(part.as_bytes()).map_err(|_| invalid())?;
                if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) } != 0
                    && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
                {
                    return Err(invalid());
                }
            }
            dir = open(&dir, part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let m = dir.metadata()?;
            if (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() })
                || (m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & 0o1000 != 0))
            {
                return Err(invalid());
            }
        }
        metadata(&dir, true)?;
        Ok(dir)
    }

    pub(super) fn lock(dir: &File, name: impl AsRef<OsStr>) -> Result<File> {
        let file = open(dir, name, libc::O_RDWR | libc::O_CREAT, 0o600)?;
        metadata(&file, false)?;
        file.try_lock().map_err(|_| {
            Error::refusal(
                crate::ErrorCode::Busy,
                "another environment operation owns the transfer lock; retry explicitly",
            )
        })?;
        Ok(file)
    }

    pub(super) fn bytes(dir: &File, name: impl AsRef<OsStr>, limit: u64) -> Result<Vec<u8>> {
        let file = open(dir, name, libc::O_RDONLY, 0)?;
        metadata(&file, false)?;
        let mut bytes = vec![];
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(invalid());
        }
        Ok(bytes)
    }

    pub(super) fn json_file(dir: &File, name: impl AsRef<OsStr>) -> Result<serde_json::Value> {
        let bytes = bytes(dir, name, 1024 * 1024)?;
        read_json_text(std::str::from_utf8(&bytes).map_err(|_| invalid())?).map_err(|_| invalid())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(crate) enum Entry {
    Dir,
    File { sha256: String, executable: bool },
}

pub(crate) type Tree = BTreeMap<String, Entry>;

// Upload can omit ignored paths or leave existing guest entries untouched.
// Record only pre-upload host entries exactly observed in the post-upload guest.
pub(crate) fn upload_baseline(host: &Tree, guest: &Tree) -> Tree {
    host.iter()
        .filter(|(name, entry)| guest.get(*name) == Some(*entry))
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    schema: u64,
    config: String,
    sandbox: String,
    paths: Tree,
}

fn relative(name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if name.is_empty()
        || name.contains(['\\', '\0'])
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::new(format!("unsafe relative path: {name:?}")));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn snapshot(root: &Path) -> Result<Tree> {
    if !fs::symlink_metadata(root)?.file_type().is_dir() {
        return Err(Error::new("project root is not an ordinary directory"));
    }
    let mut tree = Tree::new();
    walk(root, root, &mut tree)?;
    Ok(tree)
}

fn walk(root: &Path, parent: &Path, tree: &mut Tree) -> Result<()> {
    for item in fs::read_dir(parent)? {
        let item = item?;
        let path = item.path();
        let name = path
            .strip_prefix(root)
            .map_err(|e| Error::new(e.to_string()))?
            .to_str()
            .ok_or_else(|| Error::new("non-UTF-8 project path is unsupported"))?
            .replace(std::path::MAIN_SEPARATOR, "/");
        relative(&name)?;
        let metadata = fs::symlink_metadata(&path)?;
        let kind = metadata.file_type();
        let entry = if kind.is_dir() {
            Entry::Dir
        } else if kind.is_file() {
            let mut file = File::open(&path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let size = file.read(&mut buffer)?;
                if size == 0 {
                    break;
                }
                hash.update(&buffer[..size]);
            }
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            Entry::File {
                sha256: HEXLOWER.encode(&hash.finalize()),
                executable,
            }
        } else {
            return Err(Error::new(format!(
                "unsafe symlink or special file: {name}"
            )));
        };
        if tree.insert(name.clone(), entry).is_some() {
            return Err(Error::new("duplicate project path"));
        }
        if kind.is_dir() {
            walk(root, &path, tree)?;
        }
    }
    Ok(())
}

fn verify_tree(tree: &Tree) -> Result<()> {
    for (name, entry) in tree {
        let path = relative(name)?;
        for ancestor in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            let parent = ancestor
                .to_str()
                .ok_or_else(|| Error::new("invalid baseline path"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if tree.get(&parent) != Some(&Entry::Dir) {
                return Err(Error::new("baseline has a missing or non-directory parent"));
            }
        }
        if let Entry::File { sha256, .. } = entry
            && (sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::new("invalid baseline hash"));
        }
    }
    Ok(())
}

pub(crate) fn load_baseline(config: &Config) -> Result<Tree> {
    let path = config.baseline_path();
    if !path.exists() {
        return Err(Error::new(
            "no trustworthy upload baseline; preserve guest work manually via openshell sandbox download to a private directory, then explicitly upload --yes to reset the baseline",
        ));
    }
    if !fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(Error::new("invalid baseline file"));
    }
    let mut file = File::open(&path)?;
    let mut text = String::new();
    Read::by_ref(&mut file)
        .take(64 * 1024 * 1024 + 1)
        .read_to_string(&mut text)?;
    if text.len() > 64 * 1024 * 1024 {
        return Err(Error::new("upload baseline exceeds 64 MiB"));
    }
    let baseline: Baseline = serde_json::from_value(read_json_text(&text)?)?;
    let state: crate::config::RuntimeState =
        serde_json::from_value(read_json(&config.state_path())?)?;
    if baseline.schema != 1 || baseline.config != config.fingerprint || baseline.sandbox != state.id
    {
        return Err(Error::new("upload baseline does not match this sandbox"));
    }
    verify_tree(&baseline.paths)?;
    Ok(baseline.paths)
}

pub(crate) fn save_baseline(config: &Config, tree: &Tree) -> Result<()> {
    save_baseline_with_sync(config, tree, |parent| Ok(File::open(parent)?.sync_all()?))
}

fn save_baseline_with_sync(
    config: &Config,
    tree: &Tree,
    sync_parent: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    verify_tree(tree)?;
    let state: crate::config::RuntimeState =
        serde_json::from_value(read_json(&config.state_path())?)?;
    let path = config.baseline_path();
    if path.exists() && !fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(Error::new("invalid baseline destination"));
    }
    let parent = path.parent().expect("baseline parent");
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(
        &mut temp,
        &Baseline {
            schema: 1,
            config: config.fingerprint.clone(),
            sandbox: state.id,
            paths: tree.clone(),
        },
    )?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(&path).map_err(|e| Error::new(e.to_string()))?;
    sync_parent(parent).map_err(|_| Error::new("baseline replaced but persistence unconfirmed; inspect baseline and recovery before retry"))?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum Change {
    Add,
    Modify,
    Delete,
    Conflict,
}

fn classify(base: Option<&Entry>, host: Option<&Entry>, guest: Option<&Entry>) -> Option<Change> {
    if host == guest || guest == base {
        return None;
    }
    if host != base {
        return Some(Change::Conflict);
    }
    // Type changes are deliberately unsupported, including file/directory replacement.
    if base.is_some()
        && guest.is_some()
        && std::mem::discriminant(base.unwrap()) != std::mem::discriminant(guest.unwrap())
    {
        return Some(Change::Conflict);
    }
    Some(if guest.is_none() {
        Change::Delete
    } else if base.is_none() {
        Change::Add
    } else {
        Change::Modify
    })
}

fn changes(base: &Tree, host: &Tree, guest: &Tree) -> BTreeMap<String, Change> {
    let names: BTreeSet<_> = base.keys().chain(host.keys()).chain(guest.keys()).collect();
    let mut result = BTreeMap::new();
    for name in names {
        if let Some(change) = classify(base.get(name), host.get(name), guest.get(name)) {
            result.insert(name.clone(), change);
        }
    }
    // A guest-deleted directory cannot be removed if the host added entries beneath it.
    // Diagnose this before any host mutation, rather than relying on remove_dir failure.
    for (name, change) in &mut result {
        if *change == Change::Delete && base.get(name) == Some(&Entry::Dir) {
            let prefix = format!("{name}/");
            if host
                .keys()
                .any(|path| path.starts_with(&prefix) && !base.contains_key(path))
            {
                *change = Change::Conflict;
            }
        }
    }
    result
}

fn same_host(root: &Path, reviewed: &Tree) -> Result<()> {
    if snapshot(root)? != *reviewed {
        return Err(Error::new(
            "host changed since review; aborting without apply",
        ));
    }
    Ok(())
}

fn ordinary_parents(root: &Path, name: &str) -> Result<()> {
    let mut path = root.to_path_buf();
    let mut missing = false;
    for component in relative(name)?.components() {
        if !missing {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => missing = true,
                _ => {
                    return Err(Error::new(format!(
                        "host parent is not an ordinary directory: {}",
                        path.display()
                    )));
                }
            }
        }
        path.push(component);
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryIdentity {
    schema: u64,
    project: PathBuf,
}

fn recovery_refusal() -> Error {
    Error::new(
        "unfinished export recovery or unrecognized candidate beside project; inspect and restore manually before retrying, then remove the reviewed recovery directory",
    )
}

pub(crate) fn check_recovery(root: &Path) -> Result<()> {
    for (count, entry) in fs::read_dir(root.parent().expect("project parent"))?.enumerate() {
        if count >= 4096 {
            return Err(Error::new(
                "project parent exceeds recovery scan limit; inspect manually",
            ));
        }
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("dungeon-recovery-")
        {
            continue;
        }
        #[cfg(unix)]
        {
            let dir = private::directory(&entry.path(), false).map_err(|_| recovery_refusal())?;
            let bytes =
                private::bytes(&dir, "RECOVER.txt", 1024 * 1024).map_err(|_| recovery_refusal())?;
            let first = bytes
                .split(|b| *b == b'\n')
                .next()
                .ok_or_else(recovery_refusal)?;
            if !first.is_ascii() {
                return Err(recovery_refusal());
            }
            let value = read_json_text(std::str::from_utf8(first).map_err(|_| recovery_refusal())?)
                .map_err(|_| recovery_refusal())?;
            let identity: RecoveryIdentity =
                serde_json::from_value(value).map_err(|_| recovery_refusal())?;
            if identity.schema != 1
                || !identity.project.is_absolute()
                || identity
                    .project
                    .components()
                    .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
            {
                return Err(recovery_refusal());
            }
            if identity.project == root {
                return Err(recovery_refusal());
            }
        }
        #[cfg(not(unix))]
        return Err(recovery_refusal());
    }
    Ok(())
}

#[cfg(test)]
fn review_and_apply(config: &Config, base: &Tree, guest_root: &Path, yes: bool) -> Result<()> {
    review_and_apply_checked(config, base, guest_root, yes, || Ok(()))
}

#[cfg(test)]
pub(crate) fn review_and_apply_checked(
    config: &Config,
    base: &Tree,
    guest_root: &Path,
    yes: bool,
    before_apply: impl FnOnce() -> Result<()>,
) -> Result<()> {
    review_and_apply_with_hooks(config, base, guest_root, yes, before_apply, || Ok(()))
}

pub(crate) fn review_and_apply_output(
    config: &Config,
    base: &Tree,
    guest_root: &Path,
    yes: bool,
    output: &crate::output::Output,
    before_apply: impl FnOnce() -> Result<()>,
) -> Result<()> {
    apply_with_output(config, base, guest_root, yes, output, before_apply, || {
        Ok(())
    })
}

#[cfg(test)]
fn review_and_apply_with_hooks(
    config: &Config,
    base: &Tree,
    guest_root: &Path,
    yes: bool,
    after_review: impl FnOnce() -> Result<()>,
    after_step: impl FnMut() -> Result<()>,
) -> Result<()> {
    apply_with_output(
        config,
        base,
        guest_root,
        yes,
        &crate::output::Output::default(),
        after_review,
        after_step,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_with_output(
    config: &Config,
    base: &Tree,
    guest_root: &Path,
    yes: bool,
    output: &crate::output::Output,
    after_review: impl FnOnce() -> Result<()>,
    mut after_step: impl FnMut() -> Result<()>,
) -> Result<()> {
    check_recovery(&config.values.project)?;
    let guest = snapshot(guest_root)?;
    let host_root = &config.values.project;
    let host = snapshot(host_root)?;
    let changes = changes(base, &host, &guest);
    if changes.is_empty() {
        if base != &guest && host == guest {
            // Independently identical edits have converged; record that agreement.
            save_baseline(config, &guest)?;
        }
        output.review(&[])?;
        output.message(format_args!("No export changes; host unchanged."))?;
        return Ok(());
    }
    let review = changes
        .iter()
        .map(|(path, change)| {
            let label = match change {
                Change::Add => "ADD",
                Change::Modify => "MODIFY",
                Change::Delete => "DELETE",
                Change::Conflict => "CONFLICT",
            };
            (path.clone(), label)
        })
        .collect::<Vec<_>>();
    output.review(&review)?;
    if changes.values().any(|change| *change == Change::Conflict) {
        return Err(Error::refusal(
            crate::ErrorCode::Conflict,
            "export conflicts; host unchanged; resolve explicitly (no automatic merge)",
        ));
    }
    for (name, change) in &changes {
        if *change == Change::Delete {
            continue;
        }
        let path = relative(name)?;
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            let parent = parent
                .to_str()
                .ok_or_else(|| Error::new("invalid parent path"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if host.get(&parent) != Some(&Entry::Dir)
                && !(guest.get(&parent) == Some(&Entry::Dir)
                    && changes.get(&parent) == Some(&Change::Add))
            {
                return Err(Error::new(format!(
                    "cannot apply {name}: parent {parent} is not an unchanged host directory or reviewed addition"
                )));
            }
        }
    }
    if !yes {
        if output.machine || !io::stdin().is_terminal() {
            return Err(Error::refusal(
                crate::ErrorCode::ApprovalRequired,
                "review required; rerun export --yes to approve these changes",
            ));
        }
        print!("Apply these changes to the host project? Type 'yes': ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim() != "yes" {
            println!("Export cancelled; host unchanged.");
            return Ok(());
        }
    }
    after_review()?;
    same_host(host_root, &host)?;
    // Recovery lives beside the project (same filesystem); all replacements are prepared before mutation.
    let mut builder = tempfile::Builder::new();
    builder.prefix("dungeon-recovery-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let recovery = builder.tempdir_in(host_root.parent().expect("project parent"))?;
    let old = recovery.path().join("old");
    let new = recovery.path().join("new");
    fs::create_dir(&old)?;
    fs::create_dir(&new)?;
    let mut files: Vec<_> = changes
        .iter()
        .filter(|(_, c)| **c != Change::Conflict)
        .collect();
    for (name, change) in &files {
        ordinary_parents(host_root, name)?;
        let rel = relative(name)?;
        if let Some(Entry::File { .. }) = host.get(*name) {
            fs::create_dir_all(old.join(&rel).parent().unwrap())?;
            fs::copy(host_root.join(&rel), old.join(&rel))?;
        }
        if *change != &Change::Delete
            && let Some(Entry::File { executable, .. }) = guest.get(*name)
        {
            fs::create_dir_all(new.join(&rel).parent().unwrap())?;
            fs::copy(guest_root.join(&rel), new.join(&rel))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    new.join(&rel),
                    fs::Permissions::from_mode(if *executable { 0o700 } else { 0o600 }),
                )?;
            }
            #[cfg(not(unix))]
            let _ = executable;
        }
    }
    let staged = snapshot(&new)?;
    let saved = snapshot(&old)?;
    for (name, entry) in &saved {
        if matches!(entry, Entry::File { .. }) && host.get(name) != Some(entry) {
            return Err(Error::new(
                "recovery copy differs from reviewed host; host unchanged",
            ));
        }
    }
    if saved
        .values()
        .filter(|e| matches!(e, Entry::File { .. }))
        .count()
        != files
            .iter()
            .filter(|(n, _)| matches!(host.get(*n), Some(Entry::File { .. })))
            .count()
    {
        return Err(Error::new("recovery copies incomplete; host unchanged"));
    }
    for (name, entry) in &staged {
        if matches!(entry, Entry::File { .. }) && guest.get(name) != Some(entry) {
            return Err(Error::new(
                "staged file differs from reviewed guest; host unchanged",
            ));
        }
    }
    if staged
        .values()
        .filter(|e| matches!(e, Entry::File { .. }))
        .count()
        != files
            .iter()
            .filter(|(n, c)| {
                **c != Change::Delete && matches!(guest.get(*n), Some(Entry::File { .. }))
            })
            .count()
    {
        return Err(Error::new("staged files changed; host unchanged"));
    }
    same_host(host_root, &host)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut instructions = options.open(recovery.path().join("RECOVER.txt"))?;
    let identity = crate::config::review_json(&serde_json::to_value(RecoveryIdentity {
        schema: 1,
        project: host_root.clone(),
    })?)?;
    // The first line is an ASCII JSON identity; the rest is operator guidance.
    writeln!(
        instructions,
        "{}",
        identity.lines().map(str::trim).collect::<String>()
    )?;
    writeln!(
        instructions,
        "If export was interrupted, compare the host against old/ (original changed files) and new/ (not-yet-installed guest files). Restore deliberately before retrying. Baseline replacement starts only after all host changes finish; interruption or persistence failure in that phase may leave the new baseline. Inspect it too. No automatic rollback or power-loss atomicity is promised."
    )?;
    for (name, change) in &changes {
        writeln!(instructions, "{change:?} {}", name.escape_debug())?;
    }
    instructions.sync_all()?;
    for dir in [&old, &new] {
        let tree = snapshot(dir)?;
        for (name, entry) in &tree {
            if matches!(entry, Entry::File { .. }) {
                File::open(dir.join(relative(name)?))?.sync_all()?;
            }
        }
        for (name, entry) in tree.iter().rev() {
            if matches!(entry, Entry::Dir) {
                File::open(dir.join(relative(name)?))?.sync_all()?;
            }
        }
        File::open(dir)?.sync_all()?;
    }
    File::open(recovery.path())?.sync_all()?;
    File::open(host_root.parent().expect("project parent"))?.sync_all()?;
    same_host(host_root, &host)?;
    // From this point even unwinding must not discard manual recovery evidence.
    let recovery = recovery.keep();
    // Files go before directory removal; additions go after parent directory creation.
    files.sort_by(
        |(a, ca), (b, cb)| match (ca == &&Change::Delete, cb == &&Change::Delete) {
            (true, true) => b.len().cmp(&a.len()).then_with(|| a.cmp(b)),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
        },
    );
    let result: Result<()> = (|| {
        for (name, change) in &files {
            ordinary_parents(host_root, name)?;
            let rel = relative(name)?;
            let target = host_root.join(&rel);
            if *change == &Change::Delete {
                if host.get(*name) == Some(&Entry::Dir) {
                    fs::remove_dir(&target)?;
                } else {
                    fs::remove_file(&target)?;
                }
            } else if guest.get(*name) == Some(&Entry::Dir) {
                fs::create_dir(&target)?;
            } else {
                if target.exists() {
                    fs::remove_file(&target)?;
                }
                fs::rename(new.join(&rel), &target)?;
            }
            after_step()?;
        }
        Ok(())
    })();
    if let Err(err) = result {
        return Err(Error::new(format!(
            "apply incomplete: {err}; baseline unchanged; recovery copies at {} (old/ and new/); inspect and restore manually before retry",
            recovery.display().to_string().escape_debug()
        )));
    }
    if let Err(err) = save_baseline(config, &guest) {
        return Err(Error::new(format!(
            "host applied; baseline commit/persistence unconfirmed: {err}; recovery at {}; inspect host and baseline before retry",
            recovery.display().to_string().escape_debug()
        )));
    }
    // Changes are complete and the baseline is durable; no recovery copy is needed.
    fs::remove_dir_all(&recovery).map_err(|_| Error::new("host applied and baseline committed, but recovery cleanup failed; inspect remaining recovery directory manually"))?;
    output.message(format_args!("Export applied; upload baseline refreshed."))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Agent, ConfigDoc, ProjectTransfer, RuntimeState, write_new};
    use crate::policy::starter_policy;

    fn file(value: &str) -> Entry {
        Entry::File {
            sha256: value.into(),
            executable: false,
        }
    }

    #[test]
    fn three_way_classification() {
        let a = file("a");
        let b = file("b");
        let c = file("c");
        let cases = [
            (Some(&a), Some(&a), Some(&a), None),
            (Some(&a), Some(&a), Some(&b), Some(Change::Modify)),
            (Some(&a), Some(&b), Some(&a), None),
            (Some(&a), Some(&b), Some(&b), None),
            (Some(&a), Some(&b), Some(&c), Some(Change::Conflict)),
            (None, None, Some(&a), Some(Change::Add)),
            (None, Some(&a), None, None),
            (None, Some(&a), Some(&b), Some(Change::Conflict)),
            (Some(&a), Some(&a), None, Some(Change::Delete)),
            (Some(&a), None, Some(&a), None),
            (Some(&a), Some(&b), None, Some(Change::Conflict)),
            (Some(&a), None, Some(&b), Some(Change::Conflict)),
            (
                Some(&a),
                Some(&a),
                Some(&Entry::Dir),
                Some(Change::Conflict),
            ),
        ];
        for (base, host, guest, expected) in cases {
            assert_eq!(classify(base, host, guest), expected);
        }
    }

    #[test]
    fn deleting_directory_with_host_only_descendant_is_conflict() {
        let mut base = Tree::new();
        base.insert("dir".into(), Entry::Dir);
        let mut host = base.clone();
        host.insert("dir/host".into(), file("host"));
        let guest = Tree::new();
        assert_eq!(
            changes(&base, &host, &guest).get("dir"),
            Some(&Change::Conflict)
        );
    }

    #[test]
    fn upload_baseline_requires_exact_common_entries() {
        let hash = HEXLOWER.encode(&Sha256::digest(b"same"));
        let mut host = Tree::new();
        host.insert("uploaded".into(), file(&hash));
        host.insert("absent".into(), file("host-only"));
        host.insert("ignored.env".into(), file("new-host"));
        host.insert("script".into(), file("same-hash"));
        host.insert("type".into(), file("content"));
        host.insert("dir".into(), Entry::Dir);
        host.insert("dir/file".into(), file(&hash));
        let mut guest = Tree::new();
        guest.insert("uploaded".into(), file(&hash));
        guest.insert("ignored.env".into(), file("stale-guest"));
        guest.insert(
            "script".into(),
            Entry::File {
                sha256: "same-hash".into(),
                executable: true,
            },
        );
        guest.insert("type".into(), Entry::Dir);
        guest.insert("dir".into(), Entry::Dir);
        guest.insert("dir/file".into(), file(&hash));
        guest.insert("guest-add".into(), file("new"));
        let base = upload_baseline(&host, &guest);
        assert_eq!(base.get("uploaded"), host.get("uploaded"));
        assert!(!base.contains_key("absent"));
        assert!(!base.contains_key("guest-add"));
        assert!(!base.contains_key("ignored.env"));
        assert!(!base.contains_key("script"));
        assert!(!base.contains_key("type"));
        assert_eq!(base.get("dir"), Some(&Entry::Dir));
        assert_eq!(base.get("dir/file"), host.get("dir/file"));
        verify_tree(&base).unwrap();
        assert_eq!(
            changes(&base, &host, &guest).get("ignored.env"),
            Some(&Change::Conflict)
        );
        assert_eq!(
            changes(&base, &host, &guest).get("guest-add"),
            Some(&Change::Add)
        );
        assert!(!changes(&base, &host, &guest).contains_key("absent"));
    }

    #[test]
    fn unsafe_paths_and_files_refused() {
        assert_eq!(
            format!("{}", "a\n\u{1b}[31m".escape_debug()),
            "a\\n\\u{1b}[31m"
        );
        for path in [
            "../escape",
            "/absolute",
            "a/../b",
            "a/./b",
            "a//b",
            "a\\b",
            "./a",
            "",
        ] {
            assert!(relative(path).is_err(), "{path}");
        }
        let tmp = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", tmp.path().join("link")).unwrap();
            assert!(snapshot(tmp.path()).is_err());
            fs::remove_file(tmp.path().join("link")).unwrap();
            use std::ffi::CString;
            let fifo = tmp.path().join("fifo");
            let path = CString::new(fifo.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            assert!(snapshot(tmp.path()).is_err());
        }
    }

    fn fixture() -> (tempfile::TempDir, Config, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let canonical = root.path().canonicalize().unwrap();
        let project = canonical.join("project");
        let guest = canonical.join("guest");
        let config_dir = canonical.join("private");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&guest).unwrap();
        fs::create_dir(&config_dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let config = Config {
            path: config_dir.join("dungeon.json"),
            policy_path: config_dir.join("policy.json"),
            values: ConfigDoc {
                schema: 2,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project,
                project_transfer: ProjectTransfer::Copy,
                image: "localhost/test:1".into(),
                agent: Agent::Opencode,
                providers: vec![],
                imports: vec![],
            },
            policy: starter_policy(),
            fingerprint: "fingerprint".into(),
        };
        write_new(
            &config.state_path(),
            &RuntimeState {
                schema: 1,
                id: "id".into(),
                config: "fingerprint".into(),
                observed_policy: "digest".into(),
                initial_upload_pending: false,
            },
        )
        .unwrap();
        write_new(&config.path, &config.values).unwrap();
        write_new(&config.policy_path, &config.policy).unwrap();
        (root, config, guest)
    }

    #[cfg(unix)]
    #[test]
    fn initial_completion_failures_distinguish_before_and_after_publication() {
        let (_root, config, _) = fixture();
        let mut expected = crate::config::read_runtime_state(&config.state_path()).unwrap();
        expected.initial_upload_pending = true;
        fs::write(config.state_path(), serde_json::to_vec(&expected).unwrap()).unwrap();
        let guard = TransferGuard::environment(&config).unwrap();
        let before = guard.complete_initial_upload_with(
            &config,
            &expected,
            |_, _| Err(Error::new("synthetic pre-replacement failure")),
            |_| Ok(()),
        );
        assert!(before.is_err());
        assert!(
            crate::config::read_runtime_state(&config.state_path())
                .unwrap()
                .initial_upload_pending
        );
        let after = guard.complete_initial_upload_with(
            &config,
            &expected,
            |temp, path| {
                temp.persist(path).unwrap();
                Ok(())
            },
            |_| Err(Error::new("synthetic directory-sync failure")),
        );
        assert!(
            after
                .unwrap_err()
                .to_string()
                .contains("persistence unconfirmed")
        );
        assert!(
            !crate::config::read_runtime_state(&config.state_path())
                .unwrap()
                .initial_upload_pending
        );
        assert!(
            read_json(&config.state_path())
                .unwrap()
                .get("initial_upload_pending")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn initial_completion_refuses_changed_or_unsafe_private_state() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        for kind in ["identity", "boolean", "mode", "symlink", "hardlink", "fifo"] {
            let (root, config, _) = fixture();
            let mut expected = crate::config::read_runtime_state(&config.state_path()).unwrap();
            expected.initial_upload_pending = true;
            fs::write(config.state_path(), serde_json::to_vec(&expected).unwrap()).unwrap();
            let guard = TransferGuard::environment(&config).unwrap();
            let state = config.state_path();
            match kind {
                "identity" => {
                    let mut changed = expected.clone();
                    changed.id = "replacement".into();
                    fs::write(&state, serde_json::to_vec(&changed).unwrap()).unwrap();
                }
                "boolean" => {
                    let mut changed = serde_json::to_value(&expected).unwrap();
                    changed["initial_upload_pending"] = serde_json::json!("true");
                    fs::write(&state, changed.to_string()).unwrap();
                }
                "mode" => fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap(),
                "symlink" => {
                    fs::rename(&state, root.path().join("target")).unwrap();
                    symlink(root.path().join("target"), &state).unwrap();
                }
                "hardlink" => fs::hard_link(&state, root.path().join("target")).unwrap(),
                "fifo" => {
                    fs::remove_file(&state).unwrap();
                    let path = std::ffi::CString::new(state.to_str().unwrap()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                }
                _ => unreachable!(),
            }
            assert!(
                guard.complete_initial_upload(&config, &expected).is_err(),
                "{kind}"
            );
        }
    }

    #[test]
    fn apply_review_conflict_deletion_and_idempotence() {
        let (_root, config, guest) = fixture();
        let host = &config.values.project;
        fs::write(host.join("edit"), "old").unwrap();
        fs::write(host.join("delete"), "remove").unwrap();
        fs::write(guest.join("edit"), "old").unwrap();
        fs::write(guest.join("delete"), "remove").unwrap();
        let base = snapshot(host).unwrap();
        save_baseline(&config, &base).unwrap();
        fs::write(guest.join("edit"), "new").unwrap();
        fs::write(guest.join("add"), "created").unwrap();
        fs::create_dir(guest.join("nested")).unwrap();
        fs::write(guest.join("nested/inner"), "nested content").unwrap();
        fs::remove_file(guest.join("delete")).unwrap();
        assert!(review_and_apply(&config, &base, &guest, false).is_err());
        assert_eq!(fs::read(host.join("edit")).unwrap(), b"old");
        review_and_apply(&config, &base, &guest, true).unwrap();
        assert_eq!(fs::read(host.join("edit")).unwrap(), b"new");
        assert!(!host.join("delete").exists());
        assert_eq!(
            fs::read(host.join("nested/inner")).unwrap(),
            b"nested content"
        );
        assert_eq!(load_baseline(&config).unwrap(), snapshot(&guest).unwrap());
        review_and_apply(&config, &load_baseline(&config).unwrap(), &guest, true).unwrap();
    }

    #[test]
    fn conflict_and_host_only_edits_are_preserved() {
        let (_root, config, guest) = fixture();
        let host = &config.values.project;
        fs::write(host.join("conflict"), "old").unwrap();
        fs::write(guest.join("conflict"), "old").unwrap();
        let base = snapshot(host).unwrap();
        save_baseline(&config, &base).unwrap();
        fs::write(host.join("conflict"), "host").unwrap();
        fs::write(guest.join("conflict"), "guest").unwrap();
        assert!(review_and_apply(&config, &base, &guest, true).is_err());
        assert_eq!(fs::read(host.join("conflict")).unwrap(), b"host");
        assert_eq!(load_baseline(&config).unwrap(), base);
        fs::write(guest.join("conflict"), "old").unwrap();
        review_and_apply(&config, &base, &guest, true).unwrap();
        assert_eq!(fs::read(host.join("conflict")).unwrap(), b"host");
    }

    #[test]
    fn host_change_after_review_aborts_before_mutation() {
        let (_root, config, guest) = fixture();
        let host = &config.values.project;
        fs::write(host.join("edit"), "original").unwrap();
        fs::write(guest.join("edit"), "original").unwrap();
        let base = snapshot(host).unwrap();
        save_baseline(&config, &base).unwrap();
        fs::write(guest.join("edit"), "guest").unwrap();
        let err = review_and_apply_with_hooks(
            &config,
            &base,
            &guest,
            true,
            || {
                fs::write(host.join("edit"), "late host change").unwrap();
                Ok(())
            },
            || Ok(()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("host changed since review"));
        assert_eq!(fs::read(host.join("edit")).unwrap(), b"late host change");
        assert_eq!(load_baseline(&config).unwrap(), base);
    }

    #[test]
    fn partial_apply_retains_recovery_and_refuses_retry() {
        let (_root, config, guest) = fixture();
        let host = &config.values.project;
        fs::write(host.join("one"), "old").unwrap();
        fs::write(guest.join("one"), "old").unwrap();
        let base = snapshot(host).unwrap();
        save_baseline(&config, &base).unwrap();
        fs::write(guest.join("one"), "new").unwrap();
        fs::write(guest.join("two"), "new").unwrap();
        let err = review_and_apply_with_hooks(
            &config,
            &base,
            &guest,
            true,
            || Ok(()),
            || Err(Error::new("injected interruption")),
        )
        .unwrap_err();
        assert!(err.to_string().contains("apply incomplete"));
        assert_eq!(load_baseline(&config).unwrap(), base);
        let dir = fs::read_dir(host.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("dungeon-recovery-")
            })
            .unwrap();
        assert!(dir.path().join("old/one").exists());
        assert!(dir.path().join("RECOVER.txt").exists());
        assert!(
            review_and_apply(&config, &base, &guest, true)
                .unwrap_err()
                .to_string()
                .contains("unfinished export recovery")
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_bit_applies_without_arbitrary_guest_modes() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, config, guest) = fixture();
        let script = guest.join("tool");
        fs::write(&script, b"synthetic").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o751)).unwrap();
        let base = snapshot(&config.values.project).unwrap();
        save_baseline(&config, &base).unwrap();
        review_and_apply(&config, &base, &guest, true).unwrap();
        assert_eq!(
            fs::read(config.values.project.join("tool")).unwrap(),
            b"synthetic"
        );
        assert_eq!(
            fs::metadata(config.values.project.join("tool"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn host_parent_symlink_refused() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("root")).unwrap();
        std::os::unix::fs::symlink("/tmp", tmp.path().join("root/escape")).unwrap();
        assert!(ordinary_parents(&tmp.path().join("root"), "escape/file").is_err());
    }

    #[test]
    fn baseline_absence_and_binding_mismatch_refused() {
        let (_root, config, _) = fixture();
        assert!(load_baseline(&config).is_err());
        save_baseline(&config, &Tree::new()).unwrap();
        assert!(load_baseline(&config).is_ok());
        fs::write(config.state_path(), "{}").unwrap();
        assert!(load_baseline(&config).is_err());
    }

    #[test]
    fn baseline_sync_failure_reports_replacement_not_unchanged() {
        let (_root, config, _) = fixture();
        let error = save_baseline_with_sync(&config, &Tree::new(), |_| {
            Err(Error::new("injected sync failure"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("persistence unconfirmed"));
        assert_eq!(load_baseline(&config).unwrap(), Tree::new());
    }

    #[cfg(unix)]
    #[test]
    fn recovery_identity_encodes_control_and_unicode_project_names() {
        let (_root, mut config, guest) = fixture();
        let renamed = config.values.project.with_file_name("project\n\u{1b}☃");
        fs::rename(&config.values.project, &renamed).unwrap();
        config.values.project = renamed;
        fs::write(config.values.project.join("one"), "old").unwrap();
        fs::write(guest.join("one"), "new").unwrap();
        fs::write(guest.join("two"), "addition").unwrap();
        let base = snapshot(&config.values.project).unwrap();
        save_baseline(&config, &base).unwrap();
        let error = review_and_apply_with_hooks(
            &config,
            &base,
            &guest,
            true,
            || Ok(()),
            || Err(Error::new("injected")),
        )
        .unwrap_err();
        assert!(!error.to_string().contains('\u{1b}'));
        assert_retained_recovery(&config);
        let recovery = fs::read_dir(config.values.project.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("dungeon-recovery-")
            })
            .unwrap();
        let text = fs::read_to_string(recovery.join("RECOVER.txt")).unwrap();
        let first = text.lines().next().unwrap();
        assert!(first.is_ascii());
        let identity: RecoveryIdentity =
            serde_json::from_value(read_json_text(first).unwrap()).unwrap();
        assert_eq!(identity.project, config.values.project);
    }

    #[test]
    fn panic_after_mutation_retains_recovery_and_old_baseline() {
        let (_root, config, guest) = fixture();
        fs::write(config.values.project.join("one"), "old").unwrap();
        fs::write(guest.join("one"), "new").unwrap();
        fs::write(guest.join("two"), "addition").unwrap();
        let base = snapshot(&config.values.project).unwrap();
        save_baseline(&config, &base).unwrap();
        let result = std::panic::catch_unwind(|| {
            review_and_apply_with_hooks(
                &config,
                &base,
                &guest,
                true,
                || Ok(()),
                || panic!("injected unwind"),
            )
        });
        assert!(result.is_err());
        assert_eq!(load_baseline(&config).unwrap(), base);
        assert_retained_recovery(&config);
    }

    fn assert_retained_recovery(config: &Config) {
        let recovery = fs::read_dir(config.values.project.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("dungeon-recovery-")
            })
            .unwrap();
        assert_eq!(fs::read(recovery.join("old/one")).unwrap(), b"old");
        assert_eq!(fs::read(recovery.join("new/two")).unwrap(), b"addition");
        assert!(recovery.join("RECOVER.txt").is_file());
        assert!(check_recovery(&config.values.project).is_err());
    }

    #[cfg(unix)]
    fn second_config(config: &Config) -> Config {
        let mut second = config.clone();
        let dir = config
            .path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("second-private");
        crate::config::create_private_dir(&dir).unwrap();
        second.path = dir.join("dungeon.json");
        second.policy_path = dir.join("policy.json");
        write_new(&second.path, &second.values).unwrap();
        write_new(&second.policy_path, &second.policy).unwrap();
        second
    }

    #[cfg(unix)]
    #[test]
    fn locks_refuse_same_environment_and_cross_config_project_then_release() {
        let (root, config, _) = fixture();
        let second = second_config(&config);
        let guard = TransferGuard::acquire(&config).unwrap();
        assert!(TransferGuard::acquire(&config).is_err());
        assert!(TransferGuard::acquire(&second).is_err());
        let mut child = process_child(&second, "busy");
        assert!(child.wait().unwrap().success());
        drop(guard);
        TransferGuard::acquire(&second).unwrap();
        assert!(!root.path().join("projects").exists());
    }

    #[cfg(unix)]
    #[test]
    fn authority_drift_and_unsafe_lock_files_refuse() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (_root, config, _) = fixture();
        let lock = config.path.parent().unwrap().join("transfer.lock");
        let target = config.path.parent().unwrap().join("target");
        fs::write(&target, b"synthetic secret").unwrap();
        symlink(&target, &lock).unwrap();
        assert!(TransferGuard::acquire(&config).is_err());
        fs::remove_file(&lock).unwrap();
        fs::hard_link(&target, &lock).unwrap();
        assert!(TransferGuard::acquire(&config).is_err());
        fs::remove_file(&lock).unwrap();
        fs::write(&lock, "").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(TransferGuard::acquire(&config).is_err());
        fs::remove_file(&lock).unwrap();
        let fifo = std::ffi::CString::new(lock.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(TransferGuard::acquire(&config).is_err());
        fs::remove_file(&lock).unwrap();
        fs::write(&config.policy_path, "{\"secret\":\"do not print\"}").unwrap();
        fs::set_permissions(&config.policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let err = TransferGuard::acquire(&config).err().unwrap();
        assert!(err.to_string().contains("changed"));
        assert!(!err.to_string().contains("do not print"));
        assert_eq!(fs::read(target).unwrap(), b"synthetic secret");
    }

    #[cfg(unix)]
    #[test]
    fn hostile_and_unknown_recovery_candidates_are_bounded_and_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (root, config, _) = fixture();
        let recovery = root.path().join("dungeon-recovery-fixture");
        crate::config::create_private_dir(&recovery).unwrap();
        let marker = recovery.join("RECOVER.txt");
        for contents in [
            "legacy recovery",
            "\u{1b}[31m",
            "{\"schema\":1,\"schema\":1,\"project\":\"/other\"}",
            "{\"schema\":99,\"project\":\"/other\"}",
        ] {
            fs::write(&marker, contents).unwrap();
            fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
            let error = check_recovery(&config.values.project)
                .unwrap_err()
                .to_string();
            assert!(error.contains("inspect"));
            assert!(!error.contains('\u{1b}'));
        }
        fs::write(&marker, vec![b'x'; 1024 * 1024 + 1]).unwrap();
        assert!(check_recovery(&config.values.project).is_err());
        fs::remove_file(&marker).unwrap();
        let fifo = std::ffi::CString::new(marker.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(check_recovery(&config.values.project).is_err());
        fs::remove_file(&marker).unwrap();
        let secret = root.path().join("synthetic-secret");
        fs::write(&secret, "never follow").unwrap();
        symlink(&secret, &marker).unwrap();
        assert!(check_recovery(&config.values.project).is_err());
        fs::remove_file(&marker).unwrap();
        fs::hard_link(&secret, &marker).unwrap();
        assert!(check_recovery(&config.values.project).is_err());
        fs::remove_file(&marker).unwrap();
        write_new(
            &marker,
            &RecoveryIdentity {
                schema: 1,
                project: PathBuf::from("/different-project"),
            },
        )
        .unwrap();
        // Identity is strictly on the first line, unlike pretty JSON.
        fs::write(
            &marker,
            "{\"schema\":1,\"project\":\"/different-project\"}\n",
        )
        .unwrap();
        assert!(check_recovery(&config.values.project).is_ok());
        fs::set_permissions(&recovery, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_recovery(&config.values.project).is_err());
        fs::remove_dir_all(&recovery).unwrap();
        symlink(root.path().join("guest"), &recovery).unwrap();
        assert!(check_recovery(&config.values.project).is_err());
        fs::remove_file(&recovery).unwrap();
        for i in 0..4096 {
            File::create(root.path().join(format!("entry-{i}"))).unwrap();
        }
        assert!(
            check_recovery(&config.values.project)
                .unwrap_err()
                .to_string()
                .contains("scan limit")
        );
    }

    #[cfg(unix)]
    fn process_child(config: &Config, action: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "export::tests::transfer_process_child",
                "--nocapture",
            ])
            .env("DUNGEON_TRANSFER_TEST_CONFIG", &config.path)
            .env("DUNGEON_TRANSFER_TEST_ACTION", action)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn transfer_process_child() {
        let Some(path) = std::env::var_os("DUNGEON_TRANSFER_TEST_CONFIG") else {
            return;
        };
        let mut config = crate::config::load_config(Path::new(&path)).unwrap();
        config.fingerprint = "fingerprint".into();
        if std::env::var("DUNGEON_TRANSFER_TEST_ACTION").unwrap() == "busy" {
            assert!(
                TransferGuard::acquire(&config)
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("transfer lock")
            );
            return;
        }
        let _guard = TransferGuard::acquire(&config).unwrap();
        let base = load_baseline(&config).unwrap();
        review_and_apply_with_hooks(
            &config,
            &base,
            &config.values.project.parent().unwrap().join("guest"),
            true,
            || Ok(()),
            || {
                println!("TRANSFER_READY");
                io::stdout().flush().unwrap();
                let mut byte = [0];
                io::stdin().read_exact(&mut byte).unwrap();
                Ok(())
            },
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn process_int_and_term_after_first_mutation_preserve_manual_recovery() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let (_root, config, guest) = fixture();
            fs::write(config.values.project.join("one"), "old").unwrap();
            fs::write(guest.join("one"), "new").unwrap();
            fs::write(guest.join("two"), "addition").unwrap();
            let base = snapshot(&config.values.project).unwrap();
            save_baseline(&config, &base).unwrap();
            let mut child = process_child(&config, "interrupt");
            let stdout = child.stdout.take().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in io::BufReader::new(stdout).lines() {
                    if line.unwrap() == "TRANSFER_READY" {
                        let _ = tx.send(());
                        break;
                    }
                }
            });
            let ready = rx.recv_timeout(std::time::Duration::from_secs(10));
            if ready.is_err() {
                let _ = child.kill();
            } else {
                assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
            }
            let status = child.wait().unwrap();
            reader.join().unwrap();
            ready.unwrap();
            assert_eq!(status.signal(), Some(signal));
            assert_eq!(fs::read(config.values.project.join("one")).unwrap(), b"new");
            assert_eq!(load_baseline(&config).unwrap(), base);
            assert_retained_recovery(&config);
            assert!(TransferGuard::acquire(&config).is_err());
            assert!(review_and_apply(&config, &base, &guest, true).is_err());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn explicit_unlock_releases_transfer_locks_with_fork_inheritance() {
        let (_root, config, _) = fixture();
        let guard = TransferGuard::acquire(&config).unwrap();
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
        drop(guard);
        let reacquired = TransferGuard::acquire(&config);
        unsafe {
            libc::close(pipe[1]);
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        }
        assert!(reacquired.is_ok());
    }
}
