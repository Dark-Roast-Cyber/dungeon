use crate::config::{Config, digest, load_config, sandbox_name};
use crate::{Error, Result};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

fn private_config_root() -> Result<PathBuf> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("HOME or USERPROFILE is required for private project binding"))?;
    let root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::new(
            "Dungeon private configuration root must be an absolute normalized path",
        ));
    }
    let root = if root.exists() {
        root.canonicalize()?
    } else {
        root
    };
    Ok(root.join("dungeon/projects"))
}

pub(crate) fn preferences_path() -> Result<PathBuf> {
    Ok(private_config_root()?
        .parent()
        .unwrap()
        .join("container-defaults.json"))
}

pub(crate) fn list() -> Result<Vec<Value>> {
    let root = private_config_root()?;
    list_in(&root)
}

pub(crate) fn list_in(root: &Path) -> Result<Vec<Value>> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => {
            return Err(Error::new(
                "private Dungeon project root is not a directory",
            ));
        }
    }
    let mut entries = Vec::new();
    for item in fs::read_dir(root)? {
        if entries.len() >= 4096 {
            return Err(Error::new("too many private project bindings to list"));
        }
        let item = item?;
        let path = item.path();
        let key = item.file_name();
        if !item.file_type()?.is_dir() {
            entries.push(json!({"key": key.to_string_lossy(), "binding": {"state": "invalid"}}));
            continue;
        }
        let config_path = path.join("dungeon.json");
        let policy_path = path.join("policy.json");
        if ![&config_path, &policy_path]
            .iter()
            .all(|p| fs::symlink_metadata(p).is_ok_and(|meta| meta.file_type().is_file()))
        {
            entries.push(json!({"key": key.to_string_lossy(), "binding": {"state": "invalid"}}));
            continue;
        }
        let config = match load_config(&config_path) {
            Ok(config) if project_key(&config.values.project)? == key.to_string_lossy() => config,
            _ => {
                entries
                    .push(json!({"key": key.to_string_lossy(), "binding": {"state": "invalid"}}));
                continue;
            }
        };
        let state = match fs::symlink_metadata(config.state_path()) {
            Ok(meta) if !meta.file_type().is_file() => json!({"state":"invalid"}),
            Ok(_) => match crate::config::read_runtime_state(&config.state_path()) {
                Ok(runtime) if runtime.schema == 1 && runtime.config == config.fingerprint => {
                    json!({"state":"bound", "sandbox_id": runtime.id})
                }
                _ => json!({"state":"invalid"}),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                json!({"state":"configured"})
            }
            Err(_) => json!({"state":"invalid"}),
        };
        entries.push(json!({
            "key": key.to_string_lossy(),
            "project": config.values.project,
            "project_available": config.values.project.is_dir(),
            "config": config.path,
            "name": config.values.name,
            "gateway": config.values.gateway,
            "workspace": config.values.workspace,
            "transfer": config.values.project_transfer,
            "binding": state,
        }));
    }
    entries.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    Ok(entries)
}

pub(crate) fn list_scoped(directory: &Path) -> Result<Vec<Value>> {
    let key = project_key(directory)?;
    Ok(list()?
        .into_iter()
        .filter(|entry| {
            entry["key"].as_str() == Some(key.as_str())
                || entry["project"]
                    .as_str()
                    .is_some_and(|project| Path::new(project).starts_with(directory))
        })
        .collect())
}

fn project_key(project: &Path) -> Result<String> {
    digest(&json!({"project": project.to_string_lossy()}))
}

pub(crate) fn transfer_lock_path(project: &Path) -> Result<PathBuf> {
    let projects = private_config_root()?;
    let path = projects
        .parent()
        .expect("Dungeon root")
        .join("transfer-locks")
        .join(project_key(project)?);
    if path.starts_with(project) {
        return Err(Error::new("transfer locks must stay outside the project"));
    }
    Ok(path)
}

fn config_path_in(project: &Path, root: &Path) -> Result<PathBuf> {
    let directory = root.join(project_key(project)?);
    if directory.starts_with(project) {
        return Err(Error::new(
            "Dungeon private project binding must stay outside the selected project",
        ));
    }
    Ok(directory.join("dungeon.json"))
}

pub(crate) fn config_path(project: &Path) -> Result<PathBuf> {
    config_path_in(project, &private_config_root()?)
}

pub(crate) fn generated_name(project: &Path) -> Result<String> {
    let mut base = String::new();
    let mut separator = false;
    for byte in project
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .bytes()
    {
        let byte = byte.to_ascii_lowercase();
        if byte.is_ascii_alphanumeric() {
            if separator && !base.is_empty() {
                base.push('-');
            }
            separator = false;
            base.push(byte as char);
        } else {
            separator = true;
        }
        if base.len() >= 10 {
            break;
        }
    }
    while base.ends_with('-') {
        base.pop();
    }
    if base.is_empty() {
        base.push_str("project");
    }

    let key = project_key(project)?;
    let name = format!("{base}-{}", &key[..8]);
    sandbox_name(&name)?;
    Ok(name)
}

pub(crate) fn load(project: &Path) -> Result<Option<Config>> {
    load_in(project, &private_config_root()?)
}

fn load_in(project: &Path, root: &Path) -> Result<Option<Config>> {
    let path = config_path_in(project, root)?;
    let directory = path.parent().expect("binding config parent");
    if !directory.exists() {
        return Ok(None);
    }
    if !path.exists() {
        return Err(Error::new(format!(
            "private Dungeon binding for {} is incomplete at {}; inspect or remove it explicitly",
            project.display(),
            directory.display()
        )));
    }

    let config = load_config(&path)?;
    if config.values.project != project {
        return Err(Error::new(format!(
            "private Dungeon binding mismatch: {} records project {}, expected {}",
            config.path.display(),
            config.values.project.display(),
            project.display()
        )));
    }
    Ok(Some(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Agent, ConfigDoc, ProjectTransfer, create_private_dir, write_new};
    use crate::policy::starter_policy;
    use std::fs;

    fn write_binding(root: &Path, selector: &Path, recorded_project: &Path) -> PathBuf {
        let path = config_path_in(selector, root).unwrap();
        let directory = path.parent().unwrap();
        create_private_dir(directory).unwrap();
        write_new(&directory.join("policy.json"), &starter_policy()).unwrap();
        write_new(
            &path,
            &ConfigDoc {
                container: None,
                schema: 2,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project: recorded_project.to_path_buf(),
                project_transfer: ProjectTransfer::Copy,
                image: "localhost/test:1".into(),
                agent: Agent::Opencode,
                providers: vec![],
                imports: vec![],
                harness: None,
            },
        )
        .unwrap();
        path
    }

    #[test]
    fn project_binding_path_is_deterministic_and_private() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let private = root.path().join("private");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&private).unwrap();
        let project = project.canonicalize().unwrap();

        let first = config_path_in(&project, &private).unwrap();
        let second = config_path_in(&project, &private).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with(&private));
        assert!(!first.starts_with(&project));
        assert_eq!(first.file_name().unwrap(), "dungeon.json");
    }

    #[test]
    fn binding_load_reuses_exact_match_and_fails_closed_on_partial_or_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        let project = root.path().join("project");
        let other = root.path().join("other");
        let partial = root.path().join("partial");
        for path in [&private, &project, &other, &partial] {
            fs::create_dir(path).unwrap();
        }
        let project = project.canonicalize().unwrap();
        let other = other.canonicalize().unwrap();
        let partial = partial.canonicalize().unwrap();

        assert!(load_in(&project, &private).unwrap().is_none());

        let path = write_binding(&private, &project, &project);
        let first = load_in(&project, &private).unwrap().unwrap();
        let second = load_in(&project, &private).unwrap().unwrap();
        assert_eq!(first.path, path);
        assert_eq!(second.path, path);
        assert_eq!(first.values.project, project);
        assert_eq!(second.values.project, project);

        let partial_path = config_path_in(&partial, &private).unwrap();
        create_private_dir(partial_path.parent().unwrap()).unwrap();
        assert!(
            load_in(&partial, &private)
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );

        let mismatch = root.path().join("mismatch");
        fs::create_dir(&mismatch).unwrap();
        let mismatch = mismatch.canonicalize().unwrap();
        write_binding(&private, &mismatch, &other);
        assert!(
            load_in(&mismatch, &private)
                .unwrap_err()
                .to_string()
                .contains("binding mismatch")
        );
    }

    #[test]
    fn generated_names_are_safe_short_and_path_specific() {
        let root = tempfile::tempdir().unwrap();
        let left = root.path().join("left").join("same-name");
        let right = root.path().join("right").join("same-name");
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        let left = left.canonicalize().unwrap();
        let right = right.canonicalize().unwrap();

        let left_name = generated_name(&left).unwrap();
        let right_name = generated_name(&right).unwrap();
        assert_ne!(left_name, right_name);
        assert!(left_name.len() <= 19);
        assert!(right_name.len() <= 19);
        sandbox_name(&left_name).unwrap();
        sandbox_name(&right_name).unwrap();
    }

    #[test]
    fn renamed_project_gets_a_new_private_binding_key() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        let before = root.path().join("before");
        let after = root.path().join("after");
        fs::create_dir(&private).unwrap();
        fs::create_dir(&before).unwrap();
        let before = before.canonicalize().unwrap();
        let old = config_path_in(&before, &private).unwrap();

        fs::rename(&before, &after).unwrap();
        let after = after.canonicalize().unwrap();
        let new = config_path_in(&after, &private).unwrap();
        assert_ne!(old, new);
    }
}
