#![cfg(unix)]
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Pending(Option<Child>);
impl Pending {
    fn output(mut self) -> Output {
        let child = self.0.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "lifecycle child did not terminate"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.0.take().unwrap().wait_with_output().unwrap()
    }
    fn interrupt(&mut self) {
        let child = self.0.as_mut().unwrap();
        assert!(child.try_wait().unwrap().is_none());
        // Only our unreaped private leader authorizes this fixture group teardown.
        assert_eq!(
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) },
            0
        );
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if child.try_wait().unwrap().is_none() {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
            }
            let _ = child.wait();
        }
    }
}

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["home", "project", "guest", "bin"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        fs::write(root.path().join("project/file"), "host\n").unwrap();
        let config = root.path().join("private/dungeon.json");
        let f = Self { root, config };
        let out = f
            .command()
            .args(["machine", "--schema", "1", "init", "--yes", "--directory"])
            .arg(f.config.parent().unwrap())
            .arg("--project")
            .arg(f.root.path().join("project"))
            .args([
                "--name",
                "fixture",
                "--gateway",
                "synthetic",
                "--image",
                "localhost/synthetic:1",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let plan = f
            .command()
            .arg("--config")
            .arg(&f.config)
            .arg("plan")
            .output()
            .unwrap();
        let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
        let policy: Value = serde_json::from_slice(
            &fs::read(f.config.parent().unwrap().join("policy.json")).unwrap(),
        )
        .unwrap();
        fs::write(f.root.path().join("remote.json"), json!({"id":"synthetic-id","name":"fixture","workspace":"default","phase":"Ready","policy_source":"sandbox","labels":{"agentarchaeology.ai/dungeon-config":plan["fingerprint"]},"policy":policy}).to_string()).unwrap();
        let stub = f.root.path().join("bin/openshell");
        fs::write(
            &stub,
            r#"#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$1" = --version ]; then echo 'openshell 0.0.116'; exit 0; fi
shift 4
printf '%s\n' "$*" >> "$root/calls"
barrier() {
  touch "$root/$1-entered"
  i=0
  while test ! -e "$root/release"; do
    i=$((i+1)); test "$i" -lt 300 || exit 91
    sleep 0.02
  done
}
case "$1:$2" in
 sandbox:create)
   test ! -e "$root/block-create" || barrier create
   echo Ready > "$root/phase"; echo '{}';;
 sandbox:get)
   test -e "$root/phase" || exit 7
   sed "s/\"phase\":\"Ready\"/\"phase\":\"$(cat "$root/phase")\"/" "$root/remote.json";;
 sandbox:upload)
   test ! -e "$root/block-upload" || barrier upload
   test ! -e "$root/fail-upload" || exit 8
   cp "$root/project/file" "$root/guest/file";;
  sandbox:download)
    test ! -e "$root/fail-download" || exit 8
    for dest do :; done; cp "$root/guest/file" "$dest/file";;
 sandbox:stop) echo Stopped > "$root/phase";;
 sandbox:start) echo Ready > "$root/phase";;
 sandbox:exec)
   while test "$#" -gt 0 && test "$1" != --; do shift; done
   shift
   if test "$#" = 2 && test "$1" = /bin/bash && test "$2" = -l; then exit 0; fi
   test "$#" = 1 && test "$1" = synthetic-edit || exit 92
   printf 'guest-work\n' > "$root/guest/file";;
 *) exit 93;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(stub, fs::Permissions::from_mode(0o700)).unwrap();
        f
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_dungeon"));
        c.env_clear()
            .env("HOME", self.root.path().join("home"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .current_dir(self.root.path().join("project"))
            .process_group(0);
        c
    }
    fn spawn(&self, args: &[&str]) -> Pending {
        Pending(Some(
            self.command()
                .arg("--config")
                .arg(&self.config)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn run(&self, args: &[&str]) -> Output {
        self.spawn(args).output()
    }
    fn flag(&self, name: &str) {
        fs::write(self.root.path().join(name), "").unwrap();
    }
    fn until(&self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.root.path().join(name).exists() {
            assert!(Instant::now() < deadline, "barrier {name} timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn calls(&self, action: &str) -> usize {
        fs::read_to_string(self.root.path().join("calls"))
            .unwrap()
            .lines()
            .filter(|line| line.starts_with(&format!("sandbox {action} ")))
            .count()
    }
    fn pending(&self) -> bool {
        let state: Value = serde_json::from_slice(
            &fs::read(self.config.parent().unwrap().join("runtime.json")).unwrap(),
        )
        .unwrap();
        state["initial_upload_pending"].as_bool().unwrap_or(false)
    }
}

#[test]
fn shell_opens_in_the_guest_project_without_uploading_again() {
    let f = Fixture::new();
    assert!(f.run(&["up"]).status.success());
    let out = f.run(&["shell"]);
    assert!(out.status.success(), "{out:?}");
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
    let exec = calls
        .lines()
        .find(|line| line.starts_with("sandbox exec "))
        .unwrap();
    assert!(exec.contains("/sandbox/project"), "{exec}");
    assert!(exec.ends_with("-- /bin/bash -l"), "{exec}");
    assert_eq!(f.calls("upload"), 1);
}

#[test]
fn initial_copy_publication_blocks_new_work_until_the_only_upload_completes() {
    let f = Fixture::new();
    f.flag("block-upload");
    let a = f.spawn(&["up"]);
    f.until("upload-entered");
    assert!(f.pending());
    assert!(!f.run(&["up"]).status.success());
    assert!(!f.run(&["create"]).status.success());
    let status = f.run(&["machine", "--schema", "1", "status"]);
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["result"]["phase"], "ready");
    assert_eq!(status["result"]["initial_upload_pending"], true);
    assert_eq!(status["result"]["copy_baseline_valid"], false);
    let human = f.run(&["status"]);
    assert!(human.status.success());
    let human: Value = serde_json::from_slice(&human.stdout).unwrap();
    assert_eq!(human["initial_upload_pending"], true);
    assert!(human.get("policy").is_none());
    assert!(human.get("providers").is_none());
    assert!(!f.run(&["exec", "--", "synthetic-edit"]).status.success());
    #[cfg(target_os = "linux")]
    {
        let machine = f.run(&[
            "machine",
            "--schema",
            "1",
            "exec",
            "--owned",
            "--",
            "synthetic-edit",
        ]);
        assert_eq!(machine.status.code(), Some(1));
        assert!(machine.stderr.is_empty());
        let event: Value = serde_json::from_slice(&machine.stdout).unwrap();
        assert_eq!(event["event"], "finished");
        assert_eq!(event["ok"], false);
        assert!(event["exit_code"].is_null());
        assert_eq!(event["error"]["effects"], "none");
        assert_eq!(event["error"]["code"], "operation_failed");
    }
    assert_eq!(f.calls("exec"), 0);
    f.flag("release");
    assert!(a.output().status.success());
    assert!(!f.pending());
    assert_eq!(f.calls("upload"), 1);
    let status = f.run(&["machine", "--schema", "1", "status"]);
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["result"]["copy_baseline_valid"], true);
    assert!(f.run(&["exec", "--", "synthetic-edit"]).status.success());
    assert!(f.run(&["up"]).status.success());
    assert!(f.run(&["create"]).status.success());
    assert_eq!(f.calls("upload"), 1);
    assert_eq!(
        fs::read_to_string(f.root.path().join("guest/file")).unwrap(),
        "guest-work\n"
    );
}

#[test]
fn concurrent_absence_is_serialized_before_native_creation() {
    let f = Fixture::new();
    f.flag("block-create");
    let a = f.spawn(&["up"]);
    f.until("create-entered");
    assert!(!f.config.parent().unwrap().join("runtime.json").exists());
    for args in [
        ["machine", "--schema", "1", "up", "--yes"],
        ["machine", "--schema", "1", "create", "--yes"],
    ] {
        let out = f.run(&args);
        assert_eq!(out.status.code(), Some(1));
        let result: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(result["error"]["code"], "busy");
    }
    assert_eq!(f.calls("create"), 1);
    assert_eq!(f.calls("upload"), 0);
    f.flag("release");
    assert!(a.output().status.success());
    assert_eq!(f.calls("upload"), 1);
}

#[test]
fn interrupted_initial_upload_requires_explicit_ready_only_recovery() {
    let f = Fixture::new();
    f.flag("block-upload");
    let mut a = f.spawn(&["up"]);
    f.until("upload-entered");
    a.interrupt();
    assert!(!a.output().status.success());
    assert!(f.pending());
    let before = f.calls("upload");
    assert!(!f.run(&["up"]).status.success());
    assert!(!f.run(&["create"]).status.success());
    assert_eq!(f.calls("upload"), before);
    assert!(f.run(&["down"]).status.success());
    assert!(!f.run(&["upload", "--yes"]).status.success());
    assert!(f.run(&["start"]).status.success());
    f.flag("release");
    f.flag("fail-upload");
    assert!(!f.run(&["upload", "--yes"]).status.success());
    assert!(f.pending());
    fs::remove_file(f.root.path().join("fail-upload")).unwrap();
    assert!(f.run(&["upload", "--yes"]).status.success());
    assert!(!f.pending());
    assert!(f.run(&["exec", "--", "synthetic-edit"]).status.success());
}

#[test]
fn failed_initial_transfer_and_baseline_keep_pending_without_automatic_retry() {
    for failure in ["fail-upload", "fail-download", "host-drift", "baseline"] {
        let f = Fixture::new();
        f.flag("block-upload");
        let a = f.spawn(&["machine", "--schema", "1", "up", "--yes"]);
        f.until("upload-entered");
        match failure {
            "host-drift" => fs::write(f.root.path().join("project/file"), "changed\n").unwrap(),
            "baseline" => fs::create_dir(f.config.parent().unwrap().join("baseline.json")).unwrap(),
            flag => f.flag(flag),
        }
        f.flag("release");
        let out = a.output();
        assert_eq!(out.status.code(), Some(1), "{failure}");
        assert!(out.stderr.is_empty());
        let result: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(result["error"]["code"], "operation_failed");
        assert_eq!(result["error"]["effects"], "unknown");
        assert_eq!(result["error"]["message"], "Environment operation failed.");
        assert!(f.pending());
        let refused = f.run(&["machine", "--schema", "1", "up", "--yes"]);
        let refused: Value = serde_json::from_slice(&refused.stdout).unwrap();
        assert_eq!(refused["error"]["code"], "operation_failed");
        assert_eq!(refused["error"]["effects"], "none");
        assert_eq!(f.calls("upload"), 1);
        assert_eq!(f.calls("create"), 1);
    }
}

#[test]
fn explicit_create_without_baseline_never_requests_initial_copy() {
    let f = Fixture::new();
    assert!(f.run(&["create"]).status.success());
    assert!(!f.pending());
    assert!(!f.config.parent().unwrap().join("baseline.json").exists());
    let status = f.run(&["machine", "--schema", "1", "status"]);
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["result"]["copy_baseline_valid"], false);
    assert!(f.run(&["exec", "--", "synthetic-edit"]).status.success());
    assert!(f.run(&["up"]).status.success());
    assert!(f.run(&["down"]).status.success());
    assert!(f.run(&["up"]).status.success());
    assert_eq!(f.calls("upload"), 0);
    assert_eq!(f.calls("create"), 1);
}
