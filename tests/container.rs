#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    bin: PathBuf,
    config: PathBuf,
    engine: &'static str,
    _socket: UnixListener,
}

impl Fixture {
    fn new(engine: &'static str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        let bin = root.path().join("bin");
        for p in [&home, &project, &bin] {
            fs::create_dir(p).unwrap();
        }
        let socket = root.path().join("engine.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let mut f = Self {
            root,
            home,
            project,
            bin,
            config: PathBuf::new(),
            engine,
            _socket: listener,
        };
        let script = format!(
            r##"#!/bin/sh
set -eu
cd '{}'
printf '%s\n' "$*" >> calls
if [ "$1" = --remote ]; then shift 3; else shift 2; fi
case "$1 ${{2:-}}" in
  'info --format') /bin/cat info.json;;
  'image inspect') printf '[{{"Config":{{}}}}]\n';;
  'ps --all') test ! -e fail-ps; if [ -e exists ]; then printf '%s\n' '{ID}'; fi; if [ -e extra ]; then /bin/cat extra; fi;;
  'container inspect') test -e exists; test ! -e fail-inspect; /bin/cat inspect.json;;
  'create --name') if [ -e fail-before-create ]; then exit 17; fi; : > exists; /bin/cp stopped.json inspect.json; if [ -e lose-create-reply ]; then exit 17; fi; printf '%s\n' '{ID}';;
  'start '*) /bin/cp running.json inspect.json;;
  'unpause '*) /bin/cp running.json inspect.json;;
  'stop '*) /bin/cp stopped.json inspect.json;;
  'rm '*) if [ ! -e leave-container ]; then /bin/rm exists; fi; if [ -e fail-after-rm ]; then : > fail-ps; fi;;
  'exec '*) for a in "$@"; do if [ "$a" = fail-fixture ]; then exit 23; fi; done;;
  *) exit 9;;
esac
"##,
            f.root.path().display()
        );
        fs::write(f.bin.join(engine), script).unwrap();
        fs::set_permissions(f.bin.join(engine), fs::Permissions::from_mode(0o700)).unwrap();
        let info = if engine == "docker" {
            json!({"ID":"synthetic-daemon", "OSType":"linux", "SecurityOptions":[]})
        } else {
            json!({"host":{"os":"linux","hostname":"synthetic", "idMappings":{},"security":{"rootless":true}},"store":{"graphRoot":"/synthetic"}})
        };
        f.write("info.json", &info);
        let endpoint = format!("unix://{}", socket.display());
        let out = f.run(&[
            "init",
            "--yes",
            "--agent",
            "codex",
            "--runtime",
            "container",
            "--engine",
            engine,
            "--endpoint",
            &endpoint,
            "--image",
            "localhost/fixture:1",
        ]);
        assert!(out.status.success(), "{out:?}");
        let out = f.run(&["config"]);
        let config: Value = serde_json::from_slice(&out.stdout).unwrap();
        f.config = PathBuf::from(config["config"].as_str().unwrap());
        let out = f.run(&["plan"]);
        assert!(out.status.success(), "{out:?}");
        // Fingerprint is obtained through the offline plan, never guessed from native output.
        let plan: Value = serde_json::from_slice(&out.stdout).unwrap();
        let fingerprint = plan["fingerprint"].as_str().unwrap();
        let user = if engine == "podman" {
            "998:998".into()
        } else {
            format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
                libc::getegid()
            })
        };
        let c = json!({"Id":ID,"Name":format!("/dungeon-{}-{}",config["values"]["name"].as_str().unwrap(), &fingerprint[..8]),
            "Config":{"User":user,"Labels":{"agentarchaeology.ai/dungeon-config":fingerprint,"agentarchaeology.ai/dungeon-runtime":"container"}},
            "Image":"sha256:synthetic","HostConfig":{"Privileged":false},
            "Mounts":[{"Type":"bind","Source":f.project,"Destination":"/sandbox/project","RW":true}],"State":{"Running":false}});
        f.write("stopped.json", &json!([c.clone()]));
        let mut c = c;
        c["State"]["Running"] = json!(true);
        f.write("running.json", &json!([c]));
        f
    }
    fn write(&self, name: &str, value: &Value) {
        fs::write(
            self.root.path().join(name),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .current_dir(&self.project)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Output {
        let o = self.run(args);
        assert!(o.status.success(), "{args:?}: {o:?}");
        o
    }
    fn state(&self) -> PathBuf {
        self.config.with_file_name("runtime.json")
    }
}

#[test]
fn both_engines_use_native_lifecycle_and_pin_identity_without_openshell() {
    for engine in ["podman", "docker"] {
        let f = Fixture::new(engine);
        let out = Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .args([
                "setup",
                "--runtime",
                "container",
                "--engine",
                engine,
                "--endpoint",
                &format!("unix://{}", f.root.path().join("engine.sock").display()),
                "--dry-run",
            ])
            .env_clear()
            .env("HOME", &f.home)
            .env("PATH", &f.bin)
            .current_dir(&f.home)
            .output()
            .unwrap();
        assert!(out.status.success(), "setup from HOME: {out:?}");
        f.ok(&["up"]);
        let output = f.ok(&[
            "--config",
            f.config.to_str().unwrap(),
            "machine",
            "--schema",
            "1",
            "status",
        ]);
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["result"]["policy"], "unenforced");
        assert_eq!(status["result"]["environment"]["runtime"], "container");
        assert_eq!(status["result"]["environment"]["engine"], engine);
        assert_eq!(
            status["result"]["capabilities"]["streamed_exec"]["state"],
            "unsupported"
        );
        let before = fs::read(f.state()).unwrap();
        let state: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(state["container"]["engine"], engine);
        assert_eq!(state["initial_upload_pending"], Value::Null);
        assert_eq!(
            f.run(&["exec", "--", "fail-fixture"]).status.code(),
            Some(23)
        );
        f.ok(&["down"]);
        f.ok(&["up"]);
        assert_eq!(fs::read(f.state()).unwrap(), before);
        f.ok(&["exec", "--", "printf", "two words; $(host-command)"]);
        let unsupported = f.run(&["forward", "--port", "1234"]);
        assert!(!unsupported.status.success());
        assert!(String::from_utf8_lossy(&unsupported.stderr).contains("requires OpenShell"));
        let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
        assert!(calls.contains("--volume"));
        assert!(!calls.contains("sandbox create"));
        f.ok(&["down"]);
        f.ok(&["destroy", "--yes"]);
        assert!(!f.state().exists());
        assert!(f.config.exists());
    }
}

#[test]
fn replacement_service_mount_drift_and_failed_removal_retain_binding() {
    let f = Fixture::new("docker");
    f.ok(&["up"]);
    let original = fs::read(f.root.path().join("info.json")).unwrap();
    f.write(
        "info.json",
        &json!({"ID":"replacement","OSType":"linux","SecurityOptions":[]}),
    );
    let out = f.run(&["up"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("identity changed"));
    fs::write(f.root.path().join("info.json"), original).unwrap();
    let mut c: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("inspect.json")).unwrap()).unwrap();
    c[0]["Mounts"][0]["RW"] = json!(false);
    f.write("inspect.json", &c);
    assert!(!f.run(&["exec", "--", "true"]).status.success());
    fs::copy(
        f.root.path().join("running.json"),
        f.root.path().join("inspect.json"),
    )
    .unwrap();
    f.ok(&["down"]);
    fs::write(f.root.path().join("leave-container"), "").unwrap();
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(f.state().exists());
    fs::rename(f.bin.join(f.engine), f.bin.join("unavailable")).unwrap();
    let out = f.run(&["up"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("install docker"));
}

#[test]
fn binary_only_uninstall_needs_no_runtime_and_preserves_configuration() {
    let f = Fixture::new("docker");
    let installed = f.bin.join("dungeon");
    fs::copy(env!("CARGO_BIN_EXE_dungeon"), &installed).unwrap();
    fs::remove_file(f.bin.join("docker")).unwrap();
    f.ok(&[
        "uninstall",
        "--binary-only",
        "--bin-dir",
        f.bin.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(installed.exists());
    f.ok(&[
        "uninstall",
        "--binary-only",
        "--bin-dir",
        f.bin.to_str().unwrap(),
        "--yes",
    ]);
    assert!(!installed.exists());
    assert!(f.config.exists());
}

#[test]
fn plain_uninstall_accepts_empty_state_directory_without_openshell() {
    let f = Fixture::new("docker");
    f.ok(&["up"]);
    let state_dir = f.home.join(".local/state/dungeon");
    fs::create_dir_all(&state_dir).unwrap();
    fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let installed = f.bin.join("dungeon");
    fs::copy(env!("CARGO_BIN_EXE_dungeon"), &installed).unwrap();
    f.ok(&["uninstall", "--bin-dir", f.bin.to_str().unwrap(), "--yes"]);
    assert!(!installed.exists());
    assert!(!state_dir.exists());
    assert!(!f.root.path().join("exists").exists());
}

#[test]
fn failed_initial_inspection_retains_recoverable_pending_binding() {
    let f = Fixture::new("docker");
    fs::write(f.root.path().join("fail-inspect"), "").unwrap();
    assert!(!f.run(&["up"]).status.success());
    let state: Value = serde_json::from_slice(&fs::read(f.state()).unwrap()).unwrap();
    assert_eq!(state["initial_upload_pending"], true);
    assert_eq!(state["observed_policy"], "");
    fs::remove_file(f.root.path().join("fail-inspect")).unwrap();
    assert!(!f.run(&["up"]).status.success());
    f.ok(&["down"]);
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
    assert!(!f.root.path().join("exists").exists());
}

#[test]
fn paused_container_can_be_stopped_and_destroyed() {
    let f = Fixture::new("docker");
    f.ok(&["up"]);
    let mut c: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("inspect.json")).unwrap()).unwrap();
    c[0]["State"]["Paused"] = json!(true);
    f.write("inspect.json", &c);
    f.ok(&["down"]);
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
}

#[test]
fn destroy_recovers_a_binding_when_the_recorded_container_is_absent() {
    let f = Fixture::new("docker");
    f.ok(&["up"]);
    f.ok(&["down"]);
    fs::remove_file(f.root.path().join("exists")).unwrap();
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
}

#[test]
fn retry_destroy_after_post_removal_inventory_failure() {
    let f = Fixture::new("docker");
    f.ok(&["up"]);
    f.ok(&["down"]);
    fs::write(f.root.path().join("fail-after-rm"), "").unwrap();
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(f.state().exists());
    fs::remove_file(f.root.path().join("fail-ps")).unwrap();
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
}

#[test]
fn lost_create_reply_recovers_only_the_pinned_matching_container() {
    for engine in ["podman", "docker"] {
        let f = Fixture::new(engine);
        fs::write(f.root.path().join("lose-create-reply"), "").unwrap();
        assert!(!f.run(&["up"]).status.success());
        let state: Value = serde_json::from_slice(&fs::read(f.state()).unwrap()).unwrap();
        assert_eq!(state["id"], "");
        assert_eq!(state["initial_upload_pending"], true);
        fs::write(f.root.path().join("extra"), format!("{}\n", "b".repeat(64))).unwrap();
        assert!(!f.run(&["destroy", "--yes"]).status.success());
        assert!(f.root.path().join("exists").exists());
        fs::remove_file(f.root.path().join("extra")).unwrap();
        let mut bad: Value =
            serde_json::from_slice(&fs::read(f.root.path().join("inspect.json")).unwrap()).unwrap();
        bad[0]["Config"]["Labels"]["agentarchaeology.ai/dungeon-runtime"] = json!("foreign");
        f.write("inspect.json", &bad);
        assert!(!f.run(&["destroy", "--yes"]).status.success());
        assert!(f.root.path().join("exists").exists());
        fs::copy(
            f.root.path().join("stopped.json"),
            f.root.path().join("inspect.json"),
        )
        .unwrap();
        f.ok(&["down"]);
        f.ok(&["destroy", "--yes"]);
        assert!(!f.state().exists());
        assert!(!f.root.path().join("exists").exists());
    }
}

#[test]
fn unresolved_intent_is_retained_when_engine_has_no_matching_container() {
    let f = Fixture::new("docker");
    fs::write(f.root.path().join("fail-before-create"), "").unwrap();
    assert!(!f.run(&["up"]).status.success());
    assert!(f.state().exists());
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    assert!(f.state().exists());
    assert!(!f.root.path().join("exists").exists());
    // A delayed engine result remains recoverable using the original intent.
    fs::write(f.root.path().join("exists"), "").unwrap();
    fs::copy(
        f.root.path().join("stopped.json"),
        f.root.path().join("inspect.json"),
    )
    .unwrap();
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
}

#[test]
fn unwritable_private_state_prevents_native_create() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let f = Fixture::new("docker");
    let private = f.config.parent().unwrap();
    fs::set_permissions(private, fs::Permissions::from_mode(0o500)).unwrap();
    let output = f.run(&["up"]);
    fs::set_permissions(private, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!output.status.success());
    assert!(!f.root.path().join("exists").exists());
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap_or_default();
    assert!(!calls.contains("create --name"));
}

#[test]
fn dead_and_restarting_containers_allow_bounded_cleanup() {
    for state in ["Dead", "Restarting"] {
        let f = Fixture::new("docker");
        f.ok(&["up"]);
        let mut c: Value =
            serde_json::from_slice(&fs::read(f.root.path().join("inspect.json")).unwrap()).unwrap();
        c[0]["State"][state] = json!(true);
        if state == "Dead" {
            c[0]["State"]["Running"] = json!(false);
        }
        f.write("inspect.json", &c);
        f.ok(&["down"]);
        f.ok(&["destroy", "--yes"]);
        assert!(!f.state().exists());
    }
}

#[test]
fn rejects_restart_network_group_and_sysctl_drift() {
    for (field, value) in [
        (
            "RestartPolicy",
            json!({"Name":"always","MaximumRetryCount":0}),
        ),
        (
            "RestartPolicy",
            json!({"Name":"no","MaximumRetryCount":"invalid"}),
        ),
        ("Dns", json!(["192.0.2.1"])),
        ("ExtraHosts", json!(["example:192.0.2.1"])),
        ("GroupAdd", json!(["0"])),
        ("Sysctls", json!({"net.ipv4.ip_forward":"1"})),
    ] {
        let f = Fixture::new("docker");
        f.ok(&["up"]);
        let mut c: Value =
            serde_json::from_slice(&fs::read(f.root.path().join("inspect.json")).unwrap()).unwrap();
        c[0]["HostConfig"][field] = value;
        f.write("inspect.json", &c);
        assert!(
            !f.run(&["exec", "--", "true"]).status.success(),
            "accepted {field}"
        );
    }
}

#[test]
fn docker_rootless_flag_is_rejected_even_without_security_option() {
    let f = Fixture::new("docker");
    f.write(
        "info.json",
        &json!({"ID":"synthetic-daemon", "OSType":"linux", "Rootless":true, "SecurityOptions":[]}),
    );
    assert!(!f.run(&["up"]).status.success());
    assert!(!f.state().exists());
    assert!(!f.root.path().join("exists").exists());
}

#[test]
fn recovered_id_is_persisted_before_removal_so_failed_verification_can_retry() {
    let f = Fixture::new("docker");
    fs::write(f.root.path().join("lose-create-reply"), "").unwrap();
    assert!(!f.run(&["up"]).status.success());
    fs::write(f.root.path().join("fail-after-rm"), "").unwrap();
    assert!(!f.run(&["destroy", "--yes"]).status.success());
    let state: Value = serde_json::from_slice(&fs::read(f.state()).unwrap()).unwrap();
    assert_eq!(state["id"], ID);
    fs::remove_file(f.root.path().join("fail-ps")).unwrap();
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
}

#[test]
fn uninstall_recovers_a_container_after_lost_create_reply() {
    let f = Fixture::new("docker");
    fs::write(f.root.path().join("lose-create-reply"), "").unwrap();
    assert!(!f.run(&["up"]).status.success());
    let installed = f.bin.join("dungeon");
    fs::copy(env!("CARGO_BIN_EXE_dungeon"), &installed).unwrap();
    f.ok(&[
        "uninstall",
        "--bin-dir",
        f.bin.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(f.root.path().join("exists").exists());
    f.ok(&["uninstall", "--bin-dir", f.bin.to_str().unwrap(), "--yes"]);
    assert!(!f.root.path().join("exists").exists());
    assert!(!installed.exists());
}
