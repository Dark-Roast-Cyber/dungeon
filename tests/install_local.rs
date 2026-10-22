#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    root: tempfile::TempDir,
    checkout: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let checkout = root.path().join("source checkout");
        let home = root.path().join("home");
        let tools = root.path().join("tools");
        fs::create_dir(&checkout).unwrap();
        fs::create_dir(&home).unwrap();
        fs::create_dir(&tools).unwrap();
        fs::create_dir(checkout.join("scripts")).unwrap();
        fs::write(
            checkout.join("Cargo.toml"),
            "[package]\nname = 'agent-dungeon'\nversion = '0.0.2'\n",
        )
        .unwrap();
        fs::write(
            checkout.join("scripts/install-local.sh"),
            include_str!("../scripts/install-local.sh"),
        )
        .unwrap();
        let cargo = tools.join("cargo");
        fs::write(
            &cargo,
            r#"#!/bin/sh
 printf '%s\n' "$*" >> "$HOME/cargo-calls"
 test ! -e "$HOME/fail-build" || exit 17
 mkdir -p target/release
printf '#!/bin/sh\nprintf "dungeon 0.0.2\\n"\n' > target/release/dungeon
chmod 755 target/release/dungeon
"#,
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            checkout,
            home,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new("/bin/sh")
            .arg(self.checkout.join("scripts/install-local.sh"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("tools").display()),
            )
            .current_dir(self.root.path())
            .output()
            .unwrap()
    }
}

#[test]
fn local_install_is_explicit_idempotent_and_dry_run_is_read_only() {
    let f = Fixture::new();
    let out = f.run(&["--dry-run"]);
    assert!(out.status.success(), "{out:?}");
    assert!(!f.home.join(".local").exists());
    assert!(!f.home.join("cargo-calls").exists());

    let out = f.run(&[]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        fs::read_to_string(f.home.join("cargo-calls")).unwrap(),
        "build --release --locked\n"
    );
    let installed = f.home.join(".local/bin/dungeon");
    let first = fs::read(&installed).unwrap();
    assert!(String::from_utf8_lossy(&first).contains("dungeon 0.0.2"));
    let out = f.run(&[]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("Already installed"));
    assert_eq!(fs::read(&installed).unwrap(), first);
}

#[test]
fn replacement_and_symlink_targets_require_operator_action() {
    let f = Fixture::new();
    let bin = f.root.path().join("user bin");
    fs::create_dir(&bin).unwrap();
    let target = bin.join("dungeon");
    fs::write(&target, "older user binary").unwrap();
    let bin_arg = bin.to_str().unwrap();
    let out = f.run(&["--bin-dir", bin_arg]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--replace"));
    assert_eq!(fs::read(&target).unwrap(), b"older user binary");

    fs::write(f.home.join("fail-build"), "").unwrap();
    assert!(!f.run(&["--bin-dir", bin_arg, "--replace"]).status.success());
    assert_eq!(fs::read(&target).unwrap(), b"older user binary");
    fs::remove_file(f.home.join("fail-build")).unwrap();

    let out = f.run(&["--bin-dir", bin_arg, "--replace"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&fs::read(&target).unwrap()).contains("dungeon 0.0.2"));

    fs::remove_file(&target).unwrap();
    let sentinel = f.root.path().join("sentinel");
    fs::write(&sentinel, "unchanged").unwrap();
    std::os::unix::fs::symlink(&sentinel, &target).unwrap();
    assert!(!f.run(&["--bin-dir", bin_arg, "--replace"]).status.success());
    assert_eq!(fs::read(&sentinel).unwrap(), b"unchanged");
}
