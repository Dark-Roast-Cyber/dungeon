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
        self.command().args(args).output().unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dungeon"));
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("PATH", "")
            .current_dir(&self.project)
            .stdin(Stdio::null());
        command
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
    // Repository content cannot change the selected transfer, image or provider.
    fs::write(f.project.join("dungeon.json"), json!({"project_transfer":"copy","image":"localhost/untrusted:1","providers":["untrusted"]}).to_string()).unwrap();
    let out = f.run(&["init", "--yes", "--agent", "opencode"]);
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
    assert!(values["harness"].is_null());
    assert_eq!(values["providers"], json!([]));
    assert_eq!(
        values["image"],
        "ghcr.io/nvidia/openshell-community/sandboxes/base:latest"
    );
    assert_eq!(config["project_transfer"], "live");
    assert_eq!(values["project_transfer"], "live");
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
fn explicit_copy_stays_available_for_new_bindings() {
    let f = Fixture::new();
    let out = f.run(&[
        "init",
        "--yes",
        "--agent",
        "claude",
        "--project-transfer",
        "copy",
    ]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(f.config()["project_transfer"], "copy");
    assert!(f.config()["values"]["project_transfer"].is_null());
}

#[test]
fn every_runtime_and_harness_uses_the_same_image_default_and_accepts_pins() {
    for runtime in ["container", "openshell"] {
        for agent in ["opencode", "claude", "codex"] {
            for image in [
                None,
                Some("ghcr.io/example/agent:Release-X"),
                Some(
                    "ghcr.io/example/agent@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
            ] {
                let f = Fixture::new();
                let mut args = vec!["init", "--yes", "--runtime", runtime, "--agent", agent];
                if let Some(image) = image {
                    args.extend(["--image", image]);
                }
                let out = f.run(&args);
                assert!(out.status.success(), "{out:?}");
                assert_eq!(
                    f.config()["values"]["image"],
                    image.unwrap_or("ghcr.io/nvidia/openshell-community/sandboxes/base:latest")
                );
            }
        }
    }
}

#[test]
fn saved_openshell_defaults_are_authoritative_without_an_installed_cli() {
    let f = Fixture::new();
    let directory = f.home.join("config/dungeon");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("container-defaults.json");
    fs::write(
        &path,
        json!({"runtime":"openshell","container":null,"image":"ghcr.io/example/agent:1"})
            .to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(
        f.run(&["init", "--yes", "--agent", "codex"])
            .status
            .success()
    );
    let values = &f.config()["values"];
    assert!(values["container"].is_null());
    assert_eq!(values["image"], "ghcr.io/example/agent:1");
    #[cfg(target_os = "linux")]
    {
        let report: Value = serde_json::from_slice(&f.run(&["doctor", "--json"]).stdout).unwrap();
        assert!(report.get("supported_openshell").is_some());
    }
}

#[cfg(unix)]
#[test]
fn setup_defaults_cannot_be_loaded_from_the_selected_project_or_an_alias() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for alias in [false, true] {
        let f = Fixture::new();
        let source = f.project.join("config");
        fs::create_dir_all(source.join("dungeon")).unwrap();
        let prefs = source.join("dungeon/container-defaults.json");
        fs::write(&prefs, json!({"runtime":"openshell","container":null,"image":"ghcr.io/untrusted/agent:latest"}).to_string()).unwrap();
        fs::set_permissions(&prefs, fs::Permissions::from_mode(0o600)).unwrap();
        let config_root = if alias {
            let config = f.home.join("alias");
            fs::create_dir(&config).unwrap();
            symlink(source.join("dungeon"), config.join("dungeon")).unwrap();
            config
        } else {
            source
        };
        let out = f
            .command()
            .current_dir(&f.home)
            .env("XDG_CONFIG_HOME", &config_root)
            .args([
                "init",
                "--yes",
                "--agent",
                "codex",
                "--project",
                f.project.to_str().unwrap(),
                "--directory",
                f.home.join("private").to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("defaults must be outside the project"),
            "{out:?}"
        );
        assert!(!config_root.join("dungeon/projects").exists());
    }
}

#[test]
fn versions_is_read_only_and_uses_the_reviewed_image_pin() {
    let f = Fixture::new();
    let out = f.run(&["versions"]);
    assert!(out.status.success(), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["reviewed_guest_opencode"], "2.1.2");
    assert_eq!(
        report["reviewed_local_image"],
        "localhost/dungeon-agent:0.0.3-opencode-2.1.2"
    );
    assert!(report["registry_stable"].is_null());
    assert_eq!(report["reviewed_update_available"], false);
    assert!(report["selected_project_image"].is_null());
    assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
    assert!(
        f.run(&["init", "--yes", "--agent", "codex"])
            .status
            .success()
    );
    let bound: serde_json::Value = serde_json::from_slice(&f.run(&["versions"]).stdout).unwrap();
    assert_eq!(
        bound["selected_project_image"],
        report["default_container_image"]
    );
}

#[test]
fn attach_requires_terminal_before_starting_a_sandbox() {
    let f = Fixture::new();
    assert!(
        f.run(&["init", "--yes", "--agent", "opencode"])
            .status
            .success()
    );
    let result = f.run(&["attach"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires a terminal"));
    let selected = f.config()["config"].as_str().unwrap().to_owned();
    assert!(
        !PathBuf::from(selected)
            .with_file_name("runtime.json")
            .exists()
    );
}

#[test]
fn missing_binding_and_unapproved_init_leave_private_storage_empty() {
    let f = Fixture::new();
    for args in [
        vec!["plan"],
        vec!["config"],
        vec!["config", "--path"],
        vec!["init"],
        vec!["init", "--yes"],
        vec!["init", "--agent", "codex"],
        vec![],
        vec!["run", "claude"],
        vec!["run"],
        vec!["up"],
        vec!["create"],
        vec!["attach"],
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
    assert!(String::from_utf8_lossy(&out.stdout).contains("WRITE the selected host project"));
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
        vec![
            "init",
            "--yes",
            "--agent",
            "codex",
            "--image",
            "unqualified",
        ],
        vec!["init", "--yes", "--agent", "codex", "--name", "UPPER"],
        vec![
            "init",
            "--yes",
            "--agent",
            "codex",
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
    let out = f.run(&[
        "init",
        "--yes",
        "--agent",
        "codex",
        "--directory",
        directory.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(!directory.exists());
    let directory = f.root.path().join("absent/../Example project/private");
    let out = f.run(&[
        "init",
        "--yes",
        "--agent",
        "codex",
        "--directory",
        directory.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(!f.root.path().join("absent").exists());
    assert_eq!(fs::read_dir(&f.project).unwrap().count(), 0);
    #[cfg(unix)]
    {
        let alias = f.root.path().join("alias");
        std::os::unix::fs::symlink(&f.project, &alias).unwrap();
        let directory = alias.join("private");
        let out = f.run(&[
            "init",
            "--yes",
            "--agent",
            "codex",
            "--directory",
            directory.to_str().unwrap(),
        ]);
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
        "dungeon config --path",
        "dungeon machine --help",
    ] {
        assert!(text.contains(command), "{command}");
    }
    let everyday = text.split_once("Everyday commands:").unwrap().1;
    let (everyday, rest) = everyday.split_once("Setup and configuration:").unwrap();
    let (setup, advanced) = rest.split_once("Advanced and recovery:").unwrap();
    assert!(everyday.contains("export"));
    assert!(everyday.contains("run"));
    assert!(!everyday.contains("create"));
    assert!(setup.contains("doctor"));
    assert!(advanced.contains("create"));
    assert!(advanced.contains("start"));
    assert!(advanced.contains("attach"));
    assert_eq!(f.run(&["-h"]).stdout, text.as_bytes());
    assert_eq!(f.run(&["help"]).stdout, text.as_bytes());
    let out = f.run(&["create", "--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: dungeon create"));
    let out = f.run(&["init", "--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("--yes"));
    assert!(text.contains("--non-interactive"));
    assert!(text.contains("--project-transfer"));
    assert!(!text.contains("[default: opencode]"));
    assert!(text.contains("default: disabled"));
    let out = f.run(&["run", "--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("leading native flags"));
    assert!(text.contains("run codex exec PROMPT"));
}

#[test]
fn bare_launch_requires_a_terminal_for_every_configured_harness_without_runtime_work() {
    for agent in ["opencode", "claude", "codex"] {
        let f = Fixture::new();
        assert!(f.run(&["init", "--yes", "--agent", agent]).status.success());
        let config = f.config();
        let path = PathBuf::from(config["config"].as_str().unwrap());
        let before = fs::read(&path).unwrap();
        for args in [vec![], vec!["--config", path.to_str().unwrap()]] {
            let out = f.run(&args);
            assert!(!out.status.success());
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(
                error.contains("interactive launch requires a terminal"),
                "{error}"
            );
            assert!(error.contains("dungeon run"));
            assert_eq!(fs::read(&path).unwrap(), before);
            assert!(!path.with_file_name("runtime.json").exists());
        }
    }
}

#[cfg(unix)]
mod terminal {
    use super::*;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::Child;
    use std::time::{Duration, Instant};

    struct Terminal {
        child: Child,
        master: File,
        text: String,
    }

    impl Terminal {
        fn spawn(mut command: Command, args: &[&str]) -> Self {
            let (mut master, mut slave) = (-1, -1);
            assert_eq!(
                unsafe {
                    libc::openpty(
                        &mut master,
                        &mut slave,
                        std::ptr::null_mut(),
                        std::ptr::null(),
                        std::ptr::null(),
                    )
                },
                0
            );
            let master = unsafe { File::from_raw_fd(master) };
            let slave = unsafe { File::from_raw_fd(slave) };
            assert_ne!(
                unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
                -1
            );
            command
                .args(args)
                .stdin(slave.try_clone().unwrap())
                .stdout(slave.try_clone().unwrap())
                .stderr(slave);
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            Self {
                child: command.spawn().unwrap(),
                master,
                text: String::new(),
            }
        }

        fn drain(&mut self) {
            let mut bytes = [0; 4096];
            loop {
                match self.master.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => self.text.push_str(&String::from_utf8_lossy(&bytes[..n])),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.raw_os_error() == Some(libc::EIO) =>
                    {
                        break;
                    }
                    Err(e) => panic!("terminal read: {e}"),
                }
            }
        }

        fn until(&mut self, expected: &str) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                self.drain();
                if self.text.contains(expected) {
                    return;
                }
                assert!(
                    self.child.try_wait().unwrap().is_none(),
                    "exited before {expected}: {}",
                    self.text
                );
                assert!(
                    Instant::now() < deadline,
                    "waiting for {expected}: {}",
                    self.text
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn send(&mut self, text: &str) {
            self.master.write_all(text.as_bytes()).unwrap();
        }

        fn finish(mut self, success: bool) -> String {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                self.drain();
                if let Some(status) = self.child.try_wait().unwrap() {
                    self.drain();
                    assert_eq!(status.success(), success, "{}", self.text);
                    return self.text.clone();
                }
                assert!(
                    Instant::now() < deadline,
                    "terminal child timed out: {}",
                    self.text
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for Terminal {
        fn drop(&mut self) {
            if self.child.try_wait().unwrap().is_none() {
                let _ = self.child.kill();
            }
            let _ = self.child.wait();
        }
    }

    #[cfg(target_os = "linux")]
    fn selinux_fixture() -> Option<Fixture> {
        if fs::read("/sys/fs/selinux/enforce").ok().as_deref() != Some(b"1") {
            eprintln!("guided SELinux recovery requires an enforcing Linux host");
            return None;
        }
        let f = Fixture::new();
        let source = f.home.join("config/opencode/themes");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("synthetic.json"), "{}").unwrap();
        let out = f.run(&[
            "init",
            "--yes",
            "--runtime",
            "openshell",
            "--agent",
            "opencode",
            "--harness-config",
            "read-only",
            "--exclude-provider-auth",
        ]);
        assert!(out.status.success(), "{out:?}");
        Some(f)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn guided_selinux_recovery_reviews_choice_and_continues_launch() {
        for (choice, mode) in [("1", "copy"), ("2", "disabled")] {
            let Some(f) = selinux_fixture() else { return };
            let config = f.config();
            let path = PathBuf::from(config["config"].as_str().unwrap());
            let before = fs::read(&path).unwrap();
            let mut terminal =
                Terminal::spawn(f.command(), &["run", "opencode", "--", "--version"]);
            terminal.until("Harness configuration [1-3;");
            terminal.send("invalid\n");
            terminal.until("Enter 1, 2 or 3");
            assert_eq!(fs::read(&path).unwrap(), before);
            terminal.send(&format!("{choice}\n"));
            terminal.until("Approve this harness configuration selection? Type yes:");
            assert_eq!(fs::read(&path).unwrap(), before);
            terminal.send("yes\n");
            // No runtime is installed in the fixture: reaching its lookup proves continuation.
            let text = terminal.finish(false);
            assert!(text.contains("install the OpenShell"), "{text}");
            let selected = f.config();
            assert_eq!(selected["values"]["harness"]["mode"], mode);
            assert_eq!(
                selected["values"]["harness"]["snapshot"].is_string(),
                mode == "copy"
            );
            assert!(!path.with_file_name("runtime.json").exists());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn guided_selinux_cancellation_and_concurrent_binding_preserve_config() {
        for action in ["cancel", "reject", "bound", "concurrent"] {
            let Some(f) = selinux_fixture() else { return };
            let path = PathBuf::from(f.config()["config"].as_str().unwrap());
            let before = fs::read(&path).unwrap();
            if action == "bound" {
                fs::write(path.with_file_name("runtime.json"), "synthetic binding").unwrap();
            }
            let mut terminal = Terminal::spawn(f.command(), &["run", "opencode"]);
            if action != "bound" {
                terminal.until("Harness configuration [1-3;");
                if action == "cancel" {
                    terminal.send("\n");
                } else {
                    terminal.send("1\n");
                    terminal.until("Approve this harness configuration selection? Type yes:");
                    if action == "concurrent" {
                        fs::write(path.with_file_name("runtime.json"), "synthetic binding")
                            .unwrap();
                    }
                    terminal.send(if action == "reject" { "no\n" } else { "yes\n" });
                }
            }
            let text = terminal.finish(false);
            assert_eq!(fs::read(&path).unwrap(), before, "{action}: {text}");
            assert!(!path.with_file_name("harness").exists());
            if action == "bound" {
                assert!(text.contains("runtime state"));
                assert!(!text.contains("Harness configuration [1-3;"));
            }
        }
    }

    #[test]
    fn setup_wizard_reviews_runtime_and_preserves_image_case_before_approval() {
        for runtime in ["1", "OpenShell"] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &["setup"]);
            terminal.until("Runtime [container/openshell;");
            terminal.send("invalid\n");
            terminal.until("Enter 1 or 2");
            terminal.send(&format!("{runtime}\n"));
            terminal.until("Image [");
            terminal.send("ghcr.io/example/agent:Release-X\n");
            terminal.until("Apply this setup?");
            assert!(
                terminal
                    .text
                    .contains("\"image\": \"ghcr.io/example/agent:Release-X\"")
            );
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
            terminal.send("n\n");
            terminal.finish(false);
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
        }
    }

    #[test]
    fn cancelling_setup_selection_creates_nothing() {
        for input in ["\x03", "\x04"] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &["setup"]);
            terminal.until("Runtime [container/openshell;");
            terminal.send(input);
            terminal.finish(false);
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
        }
    }

    #[test]
    fn implicit_and_explicit_initialization_share_every_configuration_default() {
        for agent in ["opencode", "claude", "codex"] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &["run", agent]);
            terminal.until("Save this private configuration and continue?");
            terminal.send("y\n");
            terminal.finish(false);
            let implicit = f.config();
            let home = f.root.path().join("explicit-home");
            fs::create_dir(&home).unwrap();
            let explicit_command = || {
                let mut command = f.command();
                command
                    .env("HOME", &home)
                    .env("USERPROFILE", &home)
                    .env("XDG_CONFIG_HOME", home.join("config"));
                command
            };
            let out = explicit_command()
                .args(["init", "--yes", "--agent", agent])
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
            let out = explicit_command().arg("config").output().unwrap();
            assert!(out.status.success(), "{out:?}");
            let explicit: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(implicit["values"], explicit["values"]);
            assert_eq!(
                fs::read(implicit["policy"].as_str().unwrap()).unwrap(),
                fs::read(explicit["policy"].as_str().unwrap()).unwrap()
            );
        }
    }

    #[test]
    fn initial_choice_has_no_default_and_persists_only_after_scope_approval() {
        for (input, agent) in [("1", "opencode"), ("claude", "claude"), ("3", "codex")] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &["init"]);
            terminal.until("Harness [1-3 or name; no default]:");
            terminal.send("\ninvalid\n");
            terminal.until("Enter 1, 2 or 3");
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
            terminal.send(&format!("{input}\n"));
            terminal.until("Save this private configuration and continue?");
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
            terminal.send("y\n");
            terminal.finish(true);
            let config = f.config();
            assert_eq!(config["values"]["agent"], agent);
            assert!(config["values"]["harness"].is_null());
            let text = Terminal::spawn(f.command(), &[]).finish(false);
            assert!(!text.contains("Choose this project's harness"));
            assert!(text.contains("install Podman or Docker CE"), "{text}");
        }
    }

    #[test]
    fn implicit_initialization_uses_the_same_choice_and_never_skips_scope_approval() {
        for args in [vec![], vec!["up"], vec!["create"], vec!["run"]] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &args);
            terminal.until("Harness [1-3 or name; no default]:");
            terminal.send("claude\n");
            terminal.until("Save this private configuration and continue?");
            terminal.send("y\n");
            terminal.finish(false);
            assert_eq!(f.config()["values"]["agent"], "claude");
        }
        for args in [
            vec!["run", "opencode"],
            vec!["run", "codex"],
            vec!["attach"],
        ] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &args);
            terminal.until("Save this private configuration and continue?");
            assert!(!terminal.text.contains("Choose this project's harness"));
            terminal.send("n\n");
            terminal.finish(false);
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
        }
    }

    #[test]
    fn cancelling_choice_or_scope_creates_no_binding() {
        for (select, cancel) in [
            (false, "\x03"),
            (false, "\x04"),
            (true, "n\n"),
            (true, "\x03"),
        ] {
            let f = Fixture::new();
            let mut terminal = Terminal::spawn(f.command(), &["init"]);
            terminal.until("Harness [1-3 or name; no default]:");
            if select {
                terminal.send("codex\n");
                terminal.until("Save this private configuration and continue?");
            }
            terminal.send(cancel);
            terminal.finish(false);
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 0);
        }
    }

    #[test]
    fn bare_and_run_share_guest_launch_and_overrides_do_not_change_the_default() {
        for agent in ["opencode", "claude", "codex"] {
            let f = Fixture::new();
            assert!(
                f.run(&[
                    "init",
                    "--yes",
                    "--agent",
                    agent,
                    "--project-transfer",
                    "copy"
                ])
                .status
                .success()
            );
            let config = f.config();
            let path = PathBuf::from(config["config"].as_str().unwrap());
            let before = fs::read(&path).unwrap();
            let plan: Value = serde_json::from_slice(&f.run(&["plan"]).stdout).unwrap();
            let policy: Value =
                serde_json::from_slice(&fs::read(path.with_file_name("policy.json")).unwrap())
                    .unwrap();
            fs::write(f.home.join("remote.json"), json!({"id":"synthetic-id", "name":config["values"]["name"], "workspace":"default", "phase":"Ready", "policy_source":"sandbox", "labels":{"agentarchaeology.ai/dungeon-config":plan["fingerprint"]}, "policy":policy}).to_string()).unwrap();
            let bin = f.home.join("bin");
            fs::create_dir(&bin).unwrap();
            let stub = bin.join("openshell");
            fs::write(&stub, r#"#!/bin/sh
if test "$1" = --version; then echo 'openshell 0.0.116'; exit 0; fi
shift 4
printf '%s\n' "$*" >> "$HOME/calls"
case "$1:$2" in
 sandbox:create) echo Ready > "$HOME/phase"; echo '{}';;
 sandbox:get) test -e "$HOME/phase" || exit 7; sed "s/\"phase\":\"Ready\"/\"phase\":\"$(cat "$HOME/phase")\"/" "$HOME/remote.json";;
 sandbox:upload|sandbox:download) exit 0;;
 sandbox:stop) echo Stopped > "$HOME/phase";;
  sandbox:start) echo Ready > "$HOME/phase"; echo 'Started sandbox';;
  sandbox:exec) while test "$1" != --; do shift; done; shift; printf 'guest-launch:%s\n' "$*"; if test "${2:-}" = fail-fixture; then printf 'guest diagnostic\n' >&2; exit 23; fi;;
 *) exit 93;;
esac
"#).unwrap();
            fs::set_permissions(stub, fs::Permissions::from_mode(0o700)).unwrap();
            let command = || {
                let mut command = f.command();
                command.env("PATH", format!("{}:/usr/bin:/bin", bin.display()));
                command
            };
            let out = command()
                .args(["run", agent, "--", "fail-fixture"])
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(23), "{out:?}");
            assert_eq!(
                out.stdout,
                format!("guest-launch:{agent} fail-fixture\n").as_bytes()
            );
            assert!(String::from_utf8_lossy(&out.stderr).contains("Created"));
            for stopped in [false, true] {
                if stopped {
                    assert!(command().arg("down").output().unwrap().status.success());
                }
                let out = command()
                    .args(["run", agent, "--", "fail-fixture"])
                    .output()
                    .unwrap();
                assert_eq!(out.status.code(), Some(23), "{out:?}");
                assert_eq!(
                    out.stdout,
                    format!("guest-launch:{agent} fail-fixture\n").as_bytes()
                );
                assert!(String::from_utf8_lossy(&out.stderr).contains("guest diagnostic"));
            }
            let text = Terminal::spawn(command(), &[]).finish(true);
            assert!(text.contains(&format!("guest-launch:{agent}")), "{text}");
            assert!(!text.contains("Choose this project's harness"));
            for args in [vec!["run"], vec!["run", agent]] {
                let text = Terminal::spawn(command(), &args).finish(true);
                assert!(text.contains(&format!("guest-launch:{agent}")), "{text}");
            }
            let out = command().args(["run", "--", "--version"]).output().unwrap();
            assert!(out.status.success(), "{out:?}");
            assert!(
                String::from_utf8_lossy(&out.stdout)
                    .contains(&format!("guest-launch:{agent} --version"))
            );
            let out = command()
                .args(["run", "codex", "--version"])
                .output()
                .unwrap();
            assert!(out.status.success());
            assert!(String::from_utf8_lossy(&out.stdout).contains("guest-launch:codex --version"));
            for (arguments, expected) in [
                (
                    vec!["run", agent, "--", "--version"],
                    format!("{agent} --version"),
                ),
                (
                    vec!["run", "claude", "--", "-p", "two words"],
                    "claude -p two words".into(),
                ),
                (
                    vec!["run", "codex", "--", "exec", "--", "--version"],
                    "codex exec -- --version".into(),
                ),
            ] {
                let out = command().args(arguments).output().unwrap();
                assert!(out.status.success(), "{out:?}");
                assert_eq!(
                    String::from_utf8(out.stdout).unwrap().trim(),
                    format!("guest-launch:{expected}")
                );
            }
            assert!(command().arg("down").output().unwrap().status.success());
            let text = Terminal::spawn(command(), &[]).finish(true);
            assert!(text.contains(&format!("guest-launch:{agent}")), "{text}");
            assert_eq!(fs::read(path).unwrap(), before);
            let calls = fs::read_to_string(f.home.join("calls")).unwrap();
            assert_eq!(
                calls
                    .lines()
                    .filter(|line| line.starts_with("sandbox create "))
                    .count(),
                1
            );
            assert_eq!(
                calls
                    .lines()
                    .filter(|line| line.starts_with("sandbox upload "))
                    .count(),
                1
            );
            assert_eq!(
                calls
                    .lines()
                    .filter(|line| line.starts_with("sandbox start "))
                    .count(),
                2
            );
            assert!(!calls.contains(" serve"));
        }
    }
}
