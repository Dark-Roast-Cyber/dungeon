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
  'ps --all') if [ -e exists ]; then printf '%s\n' '{ID}'; fi; if [ -e extra ]; then /bin/cat extra; fi;;
  'container inspect') test -e exists; /bin/cat inspect.json;;
  'create --name') : > exists; /bin/cp stopped.json inspect.json; printf '%s\n' '{ID}';;
  'start '*) /bin/cp running.json inspect.json;;
  'stop '*) /bin/cp stopped.json inspect.json;;
  'rm '*) if [ ! -e leave-container ]; then /bin/rm exists; fi;;
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
