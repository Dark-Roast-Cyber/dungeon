use crate::config::{Config, read_json, read_json_text};
use crate::{Error, Result};
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(crate) enum Entry {
    Dir,
    File { sha256: String, executable: bool },
}

pub(crate) type Tree = BTreeMap<String, Entry>;

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
        || name.contains('\\')
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
    File::open(parent)?.sync_all()?;
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

fn check_recovery(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root.parent().expect("project parent"))? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("dungeon-recovery-")
            || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let marker = entry.path().join("RECOVER.txt");
        if marker.is_file()
            && fs::read_to_string(&marker)?.lines().next()
                == Some(format!("Dungeon export recovery for {}", root.display()).as_str())
        {
            return Err(Error::new(format!(
                "unfinished export recovery at {}; inspect and restore before retrying, then remove the recovery directory",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn review_and_apply(config: &Config, base: &Tree, guest_root: &Path, yes: bool) -> Result<()> {
    review_and_apply_checked(config, base, guest_root, yes, || Ok(()))
}

pub(crate) fn review_and_apply_checked(
    config: &Config,
    base: &Tree,
    guest_root: &Path,
    yes: bool,
    before_apply: impl FnOnce() -> Result<()>,
) -> Result<()> {
    review_and_apply_with_hooks(config, base, guest_root, yes, before_apply, || Ok(()))
}

fn review_and_apply_with_hooks(
    config: &Config,
    base: &Tree,
    guest_root: &Path,
    yes: bool,
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
        println!("No export changes; host unchanged.");
        return Ok(());
    }
    for (path, change) in &changes {
        let label = match change {
            Change::Add => "ADD",
            Change::Modify => "MODIFY",
            Change::Delete => "DELETE",
            Change::Conflict => "CONFLICT",
        };
        println!("{label:<9}{path}");
    }
    if changes.values().any(|change| *change == Change::Conflict) {
        return Err(Error::new(
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
        if !io::stdin().is_terminal() {
            return Err(Error::new(
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
    let recovery = tempfile::Builder::new()
        .prefix("dungeon-recovery-")
        .tempdir_in(host_root.parent().expect("project parent"))?;
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
    let mut instructions = File::create(recovery.path().join("RECOVER.txt"))?;
    writeln!(
        instructions,
        "Dungeon export recovery for {}",
        host_root.display()
    )?;
    writeln!(
        instructions,
        "If export was interrupted, compare the host against old/ (original changed files) and new/ (not-yet-installed guest files). Restore deliberately before retrying; the upload baseline has not advanced until export reports success."
    )?;
    for (name, change) in &changes {
        writeln!(instructions, "{change:?} {name}")?;
    }
    instructions.sync_all()?;
    for dir in [&old, &new] {
        for (name, entry) in snapshot(dir)? {
            if matches!(entry, Entry::File { .. }) {
                File::open(dir.join(relative(&name)?))?.sync_all()?;
            }
        }
    }
    File::open(recovery.path())?.sync_all()?;
    same_host(host_root, &host)?;
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
        let path = recovery.keep();
        return Err(Error::new(format!(
            "apply incomplete: {err}; baseline unchanged; recovery copies at {} (old/ and new/); inspect and restore manually before retry",
            path.display()
        )));
    }
    if let Err(err) = save_baseline(config, &guest) {
        let path = recovery.keep();
        return Err(Error::new(format!(
            "host applied but baseline not updated: {err}; recovery at {}; inspect before retry",
            path.display()
        )));
    }
    // Changes are complete and the baseline is durable; no recovery copy is needed.
    println!("Export applied; upload baseline refreshed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Agent, ConfigDoc, RuntimeState, write_new};
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
    fn unsafe_paths_and_files_refused() {
        for path in ["../escape", "/absolute", "a/../b", "a\\b", "./a", ""] {
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
        let project = root.path().join("project");
        let guest = root.path().join("guest");
        let config_dir = root.path().join("private");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&guest).unwrap();
        fs::create_dir(&config_dir).unwrap();
        let config = Config {
            path: config_dir.join("dungeon.json"),
            policy_path: config_dir.join("policy.json"),
            values: ConfigDoc {
                schema: 2,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project,
                image: "localhost/test:1".into(),
                agent: Agent::Opencode,
                providers: vec![],
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
            },
        )
        .unwrap();
        (root, config, guest)
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
}
