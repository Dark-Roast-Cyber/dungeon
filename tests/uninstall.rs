#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};

#[test]
fn uninstall_requires_approval_and_rejects_project_selection_without_native_contact() {
    let root = tempfile::tempdir().unwrap();
    for (args, expected) in [
        (vec!["uninstall"], "terminal or explicit --yes"),
        (
            vec!["--config", "/missing/dungeon.json", "uninstall", "--yes"],
            "accepts no PROJECT or --config",
        ),
        (
            vec!["uninstall", "--yes", "--dry-run"],
            "cannot be used with",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .args(args)
            .env_clear()
            .env("HOME", root.path())
            .env("PATH", "/missing-tools")
            .current_dir(root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{output:?}"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
