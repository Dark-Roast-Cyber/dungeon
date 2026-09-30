use crate::config::{Config, digest, load_config, sandbox_name};
use crate::{Error, Result};
use serde_json::json;
use std::env;
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

fn project_key(project: &Path) -> Result<String> {
    digest(&json!({"project": project.to_string_lossy()}))
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
    let path = config_path(project)?;
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
    use std::fs;

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
