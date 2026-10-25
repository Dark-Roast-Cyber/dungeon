#![cfg(unix)]

use data_encoding::BASE32_NOPAD;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::process::{Command, Output};

#[test]
fn native_cli_transfer_locks_use_shared_private_authority_not_project_bindings() {
    let root = tempfile::Builder::new()
        .prefix("dungeon-transfer16-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let project = canonical.join("project");
    let home = canonical.join("home");
    let xdg = home.join("config");
    let bin = canonical.join("bin");
    for path in [&project, &home, &bin] {
        fs::create_dir(path).unwrap();
    }
    let openshell = bin.join("openshell");
    fs::write(
        &openshell,
        "#!/bin/sh\nif test \"$1\" = --version; then echo 'openshell 0.0.116'; else exit 91; fi\n",
    )
    .unwrap();
    fs::set_permissions(&openshell, fs::Permissions::from_mode(0o700)).unwrap();
    let run = |args: &[&str]| -> Output {
        Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .env("PATH", &bin)
            .current_dir(&project)
            .output()
            .unwrap()
    };
    let first = canonical.join("first");
    let second = canonical.join("second");
    for config in [&first, &second] {
        let result = run(&[
            "init",
            "--agent",
            "codex",
            "--directory",
            config.to_str().unwrap(),
            "--project",
            project.to_str().unwrap(),
            "--name",
            "transfer-test",
            "--gateway",
            "local",
            "--image",
            "localhost/test:1",
            "--project-transfer",
            "copy",
            "--non-interactive",
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        fs::write(config.join("baseline.json"), b"synthetic baseline sentinel").unwrap();
    }
    let authority = xdg.join("dungeon");
    let lock_root = authority.join("transfer-locks");
    fs::create_dir_all(&lock_root).unwrap();
    for path in [&authority, &lock_root] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity =
        serde_json::json!({"project": project.canonicalize().unwrap().to_str().unwrap()});
    let key = BASE32_NOPAD
        .encode(&Sha256::digest(serde_json::to_vec(&identity).unwrap()))
        .to_ascii_lowercase();
    let project_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(lock_root.join(key))
        .unwrap();
    project_lock.try_lock().unwrap();
    for config in [&first, &second] {
        for operation in ["upload", "export"] {
            let result = run(&[
                "--config",
                config.join("dungeon.json").to_str().unwrap(),
                operation,
                "--yes",
            ]);
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("transfer lock"));
            assert_eq!(
                fs::read(config.join("baseline.json")).unwrap(),
                b"synthetic baseline sentinel"
            );
            assert!(!config.join("runtime.json").exists());
        }
    }
    assert!(!authority.join("projects").exists());
    project_lock.unlock().unwrap();
    let environment_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(first.join("transfer.lock"))
        .unwrap();
    environment_lock.try_lock().unwrap();
    let result = run(&[
        "--config",
        first.join("dungeon.json").to_str().unwrap(),
        "export",
        "--yes",
    ]);
    assert!(String::from_utf8_lossy(&result.stderr).contains("transfer lock"));
    environment_lock.unlock().unwrap();

    // Existing live configurations refuse before even resolving OpenShell or locks.
    let live = canonical.join("live");
    assert!(
        run(&[
            "init",
            "--agent",
            "codex",
            "--directory",
            live.to_str().unwrap(),
            "--project",
            project.to_str().unwrap(),
            "--name",
            "transfer-live",
            "--gateway",
            "local",
            "--image",
            "localhost/test:1",
            "--project-transfer",
            "live",
            "--non-interactive"
        ])
        .status
        .success()
    );
    fs::remove_file(&openshell).unwrap();
    for operation in ["upload", "export"] {
        let result = run(&[
            "--config",
            live.join("dungeon.json").to_str().unwrap(),
            operation,
            "--yes",
        ]);
        assert!(String::from_utf8_lossy(&result.stderr).contains("live"));
        assert!(!live.join("transfer.lock").exists());
    }
}
