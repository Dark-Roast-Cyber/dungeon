use std::fs;
use std::process::Command;

#[test]
fn doctor_never_initializes_a_binding_and_has_stable_json_failure_semantics() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let home = root.path().join("home");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&home).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .current_dir(&project)
            .env_clear()
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("PATH", root.path().join("absent-tools"))
            .args(args)
            .output()
            .unwrap()
    };
    let first = run(&["doctor", "--json"]);
    let second = run(&["doctor", "--json"]);
    assert_eq!(first.status.code(), Some(1));
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["schema"], 1);
    assert_eq!(report["read_only"], true);
    assert_eq!(report["containment_verified"], false);
    assert_eq!(report["exit_code"], 1);
    assert_eq!(report["checks"][0]["check"], "configuration");
    #[cfg(target_os = "linux")]
    {
        assert_eq!(report["checks"][0]["state"], "missing");
        assert_eq!(report["checks"][0]["blocking"], false);
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(report["checks"][0]["state"], "incompatible");
        assert_eq!(report["checks"][0]["blocking"], true);
        let platform = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["check"] == "platform")
            .unwrap();
        assert_eq!(platform["state"], "unsupported");
        assert_eq!(platform["blocking"], true);
    }
    assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    let explicit = run(&[
        "--config",
        "secret-token-does-not-exist",
        "doctor",
        "--json",
    ]);
    assert_eq!(explicit.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&explicit.stdout).contains("secret-token"));
    let report: serde_json::Value = serde_json::from_slice(&explicit.stdout).unwrap();
    assert_eq!(report["checks"][0]["state"], "incompatible");
    assert_eq!(report["checks"][0]["blocking"], true);
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn explicit_runtime_diagnoses_its_prerequisites_without_switching_the_binding() {
    for bound in ["container", "openshell"] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let home = root.path().join("home");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&home).unwrap();
        let run = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_dungeon"))
                .current_dir(&project)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", "")
                .args(args)
                .output()
                .unwrap()
        };
        assert!(
            run(&["init", "--yes", "--agent", "codex", "--runtime", bound])
                .status
                .success()
        );
        let config = run(&["config"]);
        for requested in ["container", "openshell", "auto"] {
            let output = run(&["doctor", "--json", "--runtime", requested]);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stderr.is_empty());
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let plain = requested == "container" || (requested == "auto" && bound == "container");
            assert_eq!(
                report.get("container").is_some(),
                plain,
                "{bound}/{requested}: {report}"
            );
            assert_eq!(report.get("supported_openshell").is_some(), !plain);
            assert_eq!(report["checks"][0]["state"], "healthy");
        }
        assert_eq!(run(&["config"]).stdout, config.stdout);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn cross_runtime_diagnostics_keep_the_explicit_projects_executable_exclusion() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    for (bound, requested) in [("container", "openshell"), ("openshell", "container")] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let bin = project.join("bin");
        let home = root.path().join("home");
        let other = root.path().join("other");
        for directory in [&project, &home, &other] {
            fs::create_dir(directory).unwrap();
        }
        let run = |cwd: &std::path::Path, args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_dungeon"))
                .current_dir(cwd)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", &bin)
                .args(args)
                .output()
                .unwrap()
        };
        assert!(
            run(
                &project,
                &["init", "--yes", "--agent", "codex", "--runtime", bound]
            )
            .status
            .success()
        );
        let output = run(&project, &["config"]);
        let config: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let socket = root.path().join("engine.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let preferences = home.join(".config/dungeon/container-defaults.json");
        fs::write(&preferences, serde_json::json!({"container":{"engine":"podman","endpoint":format!("unix://{}",socket.display())}}).to_string()).unwrap();
        fs::set_permissions(&preferences, fs::Permissions::from_mode(0o600)).unwrap();
        fs::create_dir(&bin).unwrap();
        let marker = root.path().join("untrusted-executed");
        for tool in ["openshell", "podman", "docker"] {
            let script = bin.join(tool);
            fs::write(
                &script,
                format!(
                    "#!/bin/sh\nprintf executed > '{}'\nexit 1\n",
                    marker.display()
                ),
            )
            .unwrap();
            fs::set_permissions(script, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = run(
            &other,
            &[
                "--config",
                config["config"].as_str().unwrap(),
                "doctor",
                "--json",
                "--runtime",
                requested,
            ],
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(
            !marker.exists(),
            "{bound}/{requested} executed a project-owned tool"
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["checks"][0]["state"], "healthy");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_cli_refuses_fifo_and_symlink_config_without_waiting_for_a_writer() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("dungeon.json");
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let run = || {
        let start = std::time::Instant::now();
        let result = Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .env_clear()
            .env("HOME", root.path())
            .env("PATH", "")
            .args(["--config"])
            .arg(&path)
            .args(["doctor", "--json"])
            .output()
            .unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(result.status.code(), Some(1));
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["checks"][0]["state"], "incompatible");
    };
    run();
    fs::remove_file(&path).unwrap();
    let target = directory.join("secret-target");
    fs::write(&target, "untrusted-secret").unwrap();
    symlink(&target, &path).unwrap();
    run();
    assert_eq!(fs::read_to_string(target).unwrap(), "untrusted-secret");
}
