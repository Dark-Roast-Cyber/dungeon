use std::fs;
use std::process::Command;

#[test]
fn setup_flags_parse_and_missing_prerequisites_leave_home_and_project_empty() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let home = root.path().join("home");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&home).unwrap();
    for args in [
        vec!["setup", "--dry-run"],
        vec!["setup", "--enable-live", "--dry-run"],
        vec!["setup", "--copy-only", "--dry-run"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .current_dir(&project)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
    }
    let incompatible = Command::new(env!("CARGO_BIN_EXE_dungeon"))
        .args(["setup", "--copy-only", "--enable-live"])
        .output()
        .unwrap();
    assert!(!incompatible.status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn dry_run_does_not_query_or_initialize_an_empty_engine_store() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let home = root.path().join("home");
    let tools = root.path().join("tools");
    for path in [&project, &home, &tools] {
        fs::create_dir(path).unwrap();
    }
    let openshell = tools.join("openshell");
    fs::write(
        &openshell,
        "#!/bin/sh\n[ \"$1\" = --version ] || exit 9\nprintf 'openshell 0.0.116\\n'\n",
    )
    .unwrap();
    let podman = tools.join("podman");
    fs::write(&podman, "#!/bin/sh\n[ \"$1\" = --version ] || { printf 'ENGINE QUERY FORBIDDEN' >&2; exit 9; }\nprintf 'podman version 5.8.4\\n'\n").unwrap();
    for path in [&openshell, &podman] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_dungeon"))
        .current_dir(&project)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &tools)
        .args(["setup", "--enable-live", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("established default rootless Podman storage required")
    );
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn native_duplicate_secret_keys_and_control_bytes_never_reach_stderr() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let project = root.path().join("project");
    let tools = root.path().join("tools");
    let store = home.join(".local/share/containers/storage");
    fs::create_dir_all(&store).unwrap();
    fs::write(store.join("db.sql"), "established synthetic marker").unwrap();
    fs::create_dir(&project).unwrap();
    fs::create_dir(&tools).unwrap();
    let openshell = tools.join("openshell");
    fs::write(&openshell, "#!/bin/sh\nprintf 'openshell 0.0.116\\n'\n").unwrap();
    let podman = tools.join("podman");
    fs::write(
        &podman,
        r#"#!/bin/sh
if [ "$1" = --version ]; then printf 'podman version 5.8.4\n'; else
printf '%s\n' '{"private-secret-\u001b[31m":1,"private-secret-\u001b[31m":2}'
fi
"#,
    )
    .unwrap();
    for path in [&openshell, &podman] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_dungeon"))
        .current_dir(&project)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &tools)
        .args(["setup", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains("invalid Podman info output"));
    assert!(!error.contains("private-secret"));
    assert!(!error.contains('\u{1b}'));
    assert!(!home.join(".config").exists());
    assert!(!home.join(".local/state").exists());
}
