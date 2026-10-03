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
