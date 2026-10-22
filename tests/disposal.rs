#![cfg(unix)]

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    project: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(root.path().join("home")).unwrap();
        fs::create_dir(root.path().join("bin")).unwrap();
        let mut fixture = Self {
            root,
            project,
            config: PathBuf::new(),
        };
        let init = fixture.run(&["init", "--yes", "--project-transfer", "copy"]);
        assert!(init.status.success(), "{init:?}");
        let listing: Value = serde_json::from_slice(&fixture.run(&["list"]).stdout).unwrap();
        fixture.config = PathBuf::from(listing[0]["config"].as_str().unwrap());
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.project, args)
    }

    fn run_in(&self, directory: &std::path::Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("XDG_CONFIG_HOME", self.root.path().join("home/config"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .current_dir(directory)
            .stdin(Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    fn bind_stopped(&self) {
        let plan: Value = serde_json::from_slice(&self.run(&["plan"]).stdout).unwrap();
        let policy: Value = serde_json::from_slice(
            &fs::read(self.config.parent().unwrap().join("policy.json")).unwrap(),
        )
        .unwrap();
        let name = plan["environment"]["name"].as_str().unwrap();
        let observed_policy = data_encoding::BASE32_NOPAD
            .encode(&Sha256::digest(serde_json::to_vec(&policy).unwrap()))
            .to_ascii_lowercase();
        let remote = json!({"id":"synthetic-id","name":name,"workspace":"default","phase":"Stopped","labels":{"agentarchaeology.ai/dungeon-config":plan["fingerprint"]},"policy":policy});
        fs::write(self.root.path().join("remote.json"), remote.to_string()).unwrap();
        fs::write(self.config.parent().unwrap().join("runtime.json"), json!({"schema":1,"id":"synthetic-id","config":plan["fingerprint"],"observed_policy":observed_policy}).to_string()).unwrap();
        fs::write(
            self.config.parent().unwrap().join("baseline.json"),
            "synthetic baseline",
        )
        .unwrap();
        let stub = self.root.path().join("bin/openshell");
        fs::write(
            &stub,
            r#"#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$1" = --version ]; then echo 'openshell 0.0.116'; exit 0; fi
shift 4
printf '%s\n' "$*" >> "$root/calls"
case "$1:$2" in
  sandbox:get) if test -e "$root/fail-get"; then exit 42; fi; cat "$root/remote.json";;
  sandbox:delete) if test -e "$root/fail-delete"; then exit 42; fi; touch "$root/deleted";;
  sandbox:start) sed 's/"phase":"Stopped"/"phase":"Ready"/' "$root/remote.json" > "$root/next.json"; mv "$root/next.json" "$root/remote.json";;
 *) exit 93;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn start_and_destroy_share_the_environment_lock() {
    let f = Fixture::new();
    f.bind_stopped();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(f.config.parent().unwrap().join("transfer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let out = f.run(&["start"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("transfer lock"));
    assert!(!f.root.path().join("calls").exists());
    let out = f.run(&["destroy", "--yes"]);
    assert!(!out.status.success());
    assert!(!f.root.path().join("deleted").exists());
    lock.unlock().unwrap();

    let out = f.run(&["start"]);
    assert!(out.status.success(), "{out:?}");
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(!f.root.path().join("deleted").exists());
}

#[test]
fn unavailable_local_gateway_suggests_read_only_diagnostics() {
    let f = Fixture::new();
    f.bind_stopped();
    fs::write(f.root.path().join("fail-get"), "").unwrap();
    let out = f.run(&["status"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("dungeon doctor"), "{stderr}");
    assert!(stderr.contains("never starts it"), "{stderr}");
    assert!(f.config.parent().unwrap().join("runtime.json").exists());
    assert!(!f.root.path().join("deleted").exists());
}

#[test]
fn list_is_offline_and_bounded_to_private_bindings() {
    let f = Fixture::new();
    let listing: Value = serde_json::from_slice(&f.run(&["list"]).stdout).unwrap();
    assert_eq!(listing[0]["binding"]["state"], "configured");
    assert_eq!(listing[0]["transfer"], "copy");
    assert!(!f.root.path().join("calls").exists());
    f.bind_stopped();
    let listing: Value = serde_json::from_slice(&f.run(&["list"]).stdout).unwrap();
    assert_eq!(listing[0]["binding"]["state"], "bound");
    assert_eq!(listing[0]["binding"]["sandbox_id"], "synthetic-id");
    assert!(!f.root.path().join("calls").exists());
    fs::remove_file(f.config.parent().unwrap().join("runtime.json")).unwrap();
    std::os::unix::fs::symlink(
        "/nonexistent",
        f.config.parent().unwrap().join("runtime.json"),
    )
    .unwrap();
    let listing: Value = serde_json::from_slice(&f.run(&["list"]).stdout).unwrap();
    assert_eq!(listing[0]["binding"]["state"], "invalid");
}

#[test]
fn list_scopes_to_directory_tree_and_all_includes_other_bindings() {
    let f = Fixture::new();
    let child = f.project.join("child");
    let prefix_sibling = f.root.path().join("project-other");
    let outside = f.root.path().join("outside");
    let unrelated = f.root.path().join("unbound");
    for directory in [&child, &prefix_sibling, &outside, &unrelated] {
        fs::create_dir(directory).unwrap();
    }
    for directory in [&child, &prefix_sibling, &outside] {
        let init = f.run_in(directory, &["init", "--yes"]);
        assert!(init.status.success(), "{init:?}");
    }
    let listing = |directory: &std::path::Path, args: &[&str]| -> Vec<Value> {
        let result = f.run_in(directory, args);
        assert!(result.status.success(), "{result:?}");
        serde_json::from_slice(&result.stdout).unwrap()
    };
    let scoped = listing(&f.project, &["list"]);
    assert_eq!(scoped.len(), 2);
    assert!(
        scoped
            .iter()
            .any(|entry| entry["project"] == f.project.to_str().unwrap())
    );
    assert!(
        scoped
            .iter()
            .any(|entry| entry["project"] == child.to_str().unwrap())
    );
    assert!(listing(&unrelated, &["list"]).is_empty());
    assert_eq!(listing(&f.project, &["list", "-a"]).len(), 4);
    assert_eq!(listing(&unrelated, &["list", "--all"]).len(), 4);
    assert!(!f.root.path().join("calls").exists());
}

#[test]
fn destroy_requires_approval_and_verified_stopped_identity() {
    let f = Fixture::new();
    f.bind_stopped();
    let state = f.config.parent().unwrap().join("runtime.json");
    let baseline = f.config.parent().unwrap().join("baseline.json");
    let out = f.run(&["destroy"]);
    assert!(!out.status.success());
    assert!(state.exists() && baseline.exists());
    assert!(!f.root.path().join("deleted").exists());

    let remote = f.root.path().join("remote.json");
    let original = fs::read(&remote).unwrap();
    let mut wrong: Value = serde_json::from_slice(&original).unwrap();
    wrong["id"] = json!("replacement-id");
    fs::write(&remote, wrong.to_string()).unwrap();
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(!f.root.path().join("deleted").exists());
    fs::write(&remote, &original).unwrap();

    let mut ready: Value = serde_json::from_slice(&original).unwrap();
    ready["phase"] = json!("Ready");
    fs::write(&remote, ready.to_string()).unwrap();
    let out = f.run(&["destroy", "--yes"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("dungeon down"));
    fs::write(&remote, &original).unwrap();

    fs::remove_file(&baseline).unwrap();
    std::os::unix::fs::symlink("/nonexistent", &baseline).unwrap();
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(!f.root.path().join("deleted").exists());
    fs::remove_file(&baseline).unwrap();
    fs::write(&baseline, "synthetic baseline").unwrap();

    fs::write(f.root.path().join("fail-delete"), "").unwrap();
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(state.exists() && baseline.exists());
    fs::remove_file(f.root.path().join("fail-delete")).unwrap();
    let out = f.run(&["destroy", "--yes"]);
    assert!(out.status.success(), "{out:?}");
    assert!(f.root.path().join("deleted").exists());
    assert!(!state.exists() && !baseline.exists());
    assert!(f.config.exists());
    let listing: Value = serde_json::from_slice(&f.run(&["list"]).stdout).unwrap();
    assert_eq!(listing[0]["binding"]["state"], "configured");
}
