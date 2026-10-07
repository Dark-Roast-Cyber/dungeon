use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    project: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("Example project");
        let home = root.path().join("home");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&home).unwrap();
        Self {
            root,
            project,
            home,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .env_clear()
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("PATH", "")
            .current_dir(&self.project)
            .stdin(Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    fn config(&self) -> Value {
        let out = self.run(&["config"]);
        assert!(out.status.success(), "{:?}", out);
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

#[test]
fn default_init_binds_current_project_and_offline_commands_do_not_need_openshell() {
    let f = Fixture::new();
    // Repository content cannot select live transfer, an image or a provider.
    fs::write(f.project.join("dungeon.json"), json!({"project_transfer":"live","image":"localhost/untrusted:1","providers":["untrusted"]}).to_string()).unwrap();
    let out = f.run(&["init", "--yes"]);
    assert!(out.status.success(), "{:?}", out);
    let config = f.config();
    let values = &config["values"];
    assert_eq!(
        values["project"],
        f.project.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(values["gateway"], "dungeon-local");
    assert_eq!(values["workspace"], "default");
    assert_eq!(values["agent"], "opencode");
    assert_eq!(values["providers"], json!([]));
    assert_eq!(
        values["image"],
        "ghcr.io/nvidia/openshell-community/sandboxes/base:latest"
    );
    assert_eq!(config["project_transfer"], "copy");
    assert!(values["name"].as_str().unwrap().len() <= 19);
    let path = PathBuf::from(config["config"].as_str().unwrap());
    assert!(
        path.starts_with(
            f.home
                .canonicalize()
                .unwrap()
                .join("config/dungeon/projects")
        )
    );
    let out = f.run(&["config", "--path"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim_end(),
        path.to_str().unwrap()
    );
    let before = fs::read(&path).unwrap();
    let policy = fs::read(path.parent().unwrap().join("policy.json")).unwrap();
    let out = f.run(&["plan"]);
    assert!(out.status.success(), "{:?}", out);
    let plan: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(plan["environment"]["config"], config["config"]);
    assert!(!path.parent().unwrap().join("runtime.json").exists());
    assert!(
        !f.run(&["init", "--yes", "--agent", "codex"])
            .status
            .success()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        fs::read(path.parent().unwrap().join("policy.json")).unwrap(),
        policy
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn missing_binding_and_unapproved_init_leave_private_storage_empty() {
    let f = Fixture::new();
    for args in [
        vec!["plan"],
        vec!["config"],
        vec!["config", "--path"],
        vec!["init"],
        vec![],
    ] {
        let out = f.run(&args);
        assert!(!out.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("dungeon init"),
            "{:?}",
            out
        );
        assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
    }
}

#[test]
fn init_overrides_are_explicit_and_custom_directories_are_not_adopted() {
    let f = Fixture::new();
    let private = f.root.path().join("private");
    let selected_project = f.root.path().join("other-project");
    fs::create_dir(&selected_project).unwrap();
    let out = f.run(&[
        "init",
        "--non-interactive",
        "--directory",
        private.to_str().unwrap(),
        "--project",
        selected_project.to_str().unwrap(),
        "--name",
        "custom",
        "--agent",
        "codex",
        "--gateway",
        "reviewed",
        "--workspace",
        "team",
        "--image",
        "localhost/reviewed:1",
        "--provider",
        "first",
        "--provider",
        "second",
        "--project-transfer",
        "live",
    ]);
    assert!(out.status.success(), "{:?}", out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("guest can write directly"));
    assert!(!f.run(&["config"]).status.success());
    let path = private.join("dungeon.json");
    let out = f.run(&["--config", path.to_str().unwrap(), "config"]);
    assert!(out.status.success(), "{:?}", out);
    let config: Value = serde_json::from_slice(&out.stdout).unwrap();
    let values = &config["values"];
    assert_eq!(values["name"], "custom");
    assert_eq!(
        values["project"],
        selected_project.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(values["agent"], "codex");
    assert_eq!(values["gateway"], "reviewed");
    assert_eq!(values["workspace"], "team");
    assert_eq!(values["image"], "localhost/reviewed:1");
    assert_eq!(values["providers"], json!(["first", "second"]));
    assert_eq!(values["project_transfer"], "live");
    assert!(!private.join("runtime.json").exists());
}

#[test]
fn invalid_initial_configuration_is_rejected_before_writing_files() {
    let f = Fixture::new();
    for args in [
        vec!["init", "--yes", "--image", "unqualified"],
        vec!["init", "--yes", "--name", "UPPER"],
        vec![
            "init",
            "--yes",
            "--provider",
            "repeat",
            "--provider",
            "repeat",
        ],
        vec!["--config", "ignored.json", "init", "--yes"],
    ] {
        let out = f.run(&args);
        assert!(!out.status.success(), "{args:?}");
        assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
    }
    let directory = f.project.join("private");
    let out = f.run(&["init", "--yes", "--directory", directory.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!directory.exists());
    let directory = f.root.path().join("absent/../Example project/private");
    let out = f.run(&["init", "--yes", "--directory", directory.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!f.root.path().join("absent").exists());
    assert_eq!(fs::read_dir(&f.project).unwrap().count(), 0);
    #[cfg(unix)]
    {
        let alias = f.root.path().join("alias");
        std::os::unix::fs::symlink(&f.project, &alias).unwrap();
        let directory = alias.join("private");
        let out = f.run(&["init", "--yes", "--directory", directory.to_str().unwrap()]);
        assert!(!out.status.success());
        assert!(!directory.exists());
    }
}

#[test]
fn help_explains_the_main_workflow_and_configuration_flags() {
    let f = Fixture::new();
    let out = f.run(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for command in [
        "dungeon init",
        "dungeon exec",
        "dungeon export",
        "dungeon config --path",
        "dungeon machine --help",
    ] {
        assert!(text.contains(command), "{command}");
    }
    let out = f.run(&["init", "--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("--yes"));
    assert!(text.contains("--non-interactive"));
    assert!(text.contains("--project-transfer"));
}
