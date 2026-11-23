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
        Self::with_selinux(engine, None)
    }
    fn with_selinux(engine: &'static str, mode: Option<&str>) -> Self {
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
  'image inspect') test ! -e missing-image; printf '[{{"Config":{{}}}}]\n';;
  'pull '*) test ! -e fail-pull; /bin/rm -f missing-image;;
  'ps --all') test ! -e fail-ps; if [ -e exists ]; then printf '%s\n' '{ID}'; fi; case "$*" in *'--filter id='*) ;; *) if [ -e extra ]; then /bin/cat extra; fi;; esac;;
  'container inspect') test -e exists; test ! -e fail-inspect; /bin/cat inspect.json;;
  'create --name') if [ -e fail-before-create ]; then exit 17; fi; : > exists; /bin/cp stopped.json inspect.json; if [ -e lose-create-reply ]; then exit 17; fi; printf '%s\n' '{ID}';;
  'start '*) /bin/cp running.json inspect.json; if [ -e replace-seed-at-start ]; then /bin/chmod 600 "$(/bin/cat replace-seed-at-start)"; printf 'changed-after-validation' > "$(/bin/cat replace-seed-at-start)"; fi;;
  'unpause '*) /bin/cp running.json inspect.json;;
  'stop '*) /bin/cp stopped.json inspect.json;;
  'rm '*) if [ ! -e leave-container ]; then /bin/rm exists; fi; if [ -e fail-after-rm ]; then : > fail-ps; fi;;
  'exec '*) if [ "${{2:-}}" = --interactive ]; then /bin/cat > seeded-stdin; if [ -e fail-seed ]; then exit 29; fi; fi; for a in "$@"; do if [ "$a" = fail-fixture ]; then exit 23; fi; if [ "$a" = output-fixture ]; then printf '{{"fixture":true}}\n'; printf 'guest diagnostic\n' >&2; exit 23; fi; done;;
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
            json!({"host":{"os":"linux","hostname":"synthetic", "idMappings":{},"security":{"rootless":true,"selinuxEnabled":true}},"store":{"graphRoot":"/synthetic"}})
        };
        f.write("info.json", &info);
        let endpoint = format!("unix://{}", socket.display());
        let mut args = vec![
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
        ];
        if let Some(mode) = mode {
            args.extend(["--selinux-isolation", mode]);
        }
        let out = f.run(&args);
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
            "Image":"sha256:synthetic","HostConfig":{"Privileged":false,"SecurityOpt":if mode == Some("disabled") || (mode.is_none() && engine == "podman") {json!(["no-new-privileges","label=disable"])} else {json!(["no-new-privileges"])}},
            "ProcessLabel":if mode == Some("disabled") || (mode.is_none() && engine == "podman") {""} else {"system_u:system_r:container_t:s0:c123,c456"},
            "MountLabel":"system_u:object_r:container_file_t:s0:c123,c456",
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
    fn import_preferences(&self, bytes: &[u8]) -> PathBuf {
        let source = self.home.join(".config/opencode");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("cli.json"), bytes).unwrap();
        self.ok(&[
            "config",
            "--edit",
            "--yes",
            "--agent",
            "opencode",
            "--harness-config",
            "copy",
        ]);
        let config: Value = serde_json::from_slice(&self.ok(&["config"]).stdout).unwrap();
        let plan: Value = serde_json::from_slice(&self.ok(&["plan"]).stdout).unwrap();
        let fingerprint = plan["fingerprint"].as_str().unwrap();
        for file in ["stopped.json", "running.json"] {
            let mut native: Value =
                serde_json::from_slice(&fs::read(self.root.path().join(file)).unwrap()).unwrap();
            native[0]["Config"]["Labels"]["agentarchaeology.ai/dungeon-config"] =
                json!(fingerprint);
            native[0]["Name"] = json!(format!(
                "/dungeon-{}-{}",
                config["values"]["name"].as_str().unwrap(),
                &fingerprint[..8],
            ));
            self.write(file, &native);
        }
        self.config
            .with_file_name("harness")
            .join(config["values"]["harness"]["snapshot"].as_str().unwrap())
            .join("config/cli.json")
    }
}

#[test]
fn first_create_seeds_exact_snapshot_bytes_and_resume_preserves_guest_changes() {
    let f = Fixture::new("podman");
    let snapshot = f.import_preferences(b"approved\0\xff");
    f.ok(&["up"]);
    assert_eq!(
        fs::read(f.root.path().join("seeded-stdin")).unwrap(),
        b"approved\0\xff"
    );
    fs::write(f.root.path().join("seeded-stdin"), b"guest-preference").unwrap();
    f.ok(&["down"]);
    f.ok(&["up"]);
    assert_eq!(
        fs::read(f.root.path().join("seeded-stdin")).unwrap(),
        b"guest-preference"
    );
    assert_eq!(fs::read(snapshot).unwrap(), b"approved\0\xff");
}

#[test]
fn seed_failure_preserves_pending_state_and_allows_explicit_destruction() {
    let f = Fixture::new("podman");
    f.import_preferences(b"approved");
    fs::write(f.root.path().join("fail-seed"), "").unwrap();
    let output = f.run(&["up"]);
    assert!(!output.status.success(), "{output:?}");
    let state: Value = serde_json::from_slice(&fs::read(f.state()).unwrap()).unwrap();
    assert_eq!(state["initial_upload_pending"], true);
    assert!(!f.run(&["run", "codex"]).status.success());
    f.ok(&["down"]);
    f.ok(&["destroy", "--yes"]);
    assert!(!f.state().exists());
}

#[test]
fn creation_freezes_approved_seed_bytes_before_native_mutations() {
    let f = Fixture::new("podman");
    let snapshot = f.import_preferences(b"approved");
    fs::write(
        f.root.path().join("replace-seed-at-start"),
        snapshot.as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    f.ok(&["up"]);
    assert_eq!(
        fs::read(f.root.path().join("seeded-stdin")).unwrap(),
        b"approved"
    );
    assert_eq!(fs::read(snapshot).unwrap(), b"changed-after-validation");
}

#[test]
fn tampered_seed_is_refused_before_container_creation() {
    let f = Fixture::new("podman");
    let snapshot = f.import_preferences(b"approved");
    fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&snapshot, b"changed-before-validation").unwrap();
    fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o400)).unwrap();
    let output = f.run(&["up"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(!f.state().exists());
    assert!(!f.root.path().join("exists").exists());
}

#[test]
fn disabled_podman_mode_is_reviewed_inspected_and_preserved_on_resume() {
    let f = Fixture::with_selinux("podman", Some("disabled"));
    let config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    assert_eq!(config["selinux_isolation"], "disabled");
    let plan: Value = serde_json::from_slice(&f.ok(&["plan"]).stdout).unwrap();
    assert_eq!(plan["selinux_isolation"]["requested"], "disabled");
    assert_eq!(plan["selinux_isolation"]["verification"], "not-inspected");
    f.ok(&["up"]);
    let status: Value = serde_json::from_slice(&f.ok(&["status"]).stdout).unwrap();
    assert_eq!(status["selinux_isolation"]["requested"], "disabled");
    assert_eq!(
        status["selinux_isolation"]["verification"],
        "native-configuration"
    );
    assert_eq!(status["selinux_isolation"]["process_label"], "");
    let machine: Value =
        serde_json::from_slice(&f.ok(&["machine", "--schema", "1", "status"]).stdout).unwrap();
    assert_eq!(machine["schema"], 1);
    assert_eq!(
        machine["result"]["environment"]["selinux_isolation"],
        status["selinux_isolation"]
    );
    let doctor: Value = serde_json::from_slice(&f.ok(&["doctor", "--json"]).stdout).unwrap();
    assert_eq!(
        doctor["container"]["status"]["selinux_isolation"],
        status["selinux_isolation"]
    );
    let state = fs::read(f.state()).unwrap();
    f.ok(&["down"]);
    f.ok(&["up"]);
    assert_eq!(fs::read(f.state()).unwrap(), state);
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
    let create = calls
        .lines()
        .find(|line| line.contains("create --name"))
        .unwrap();
    assert!(create.contains("--security-opt label=disable"));
    assert!(create.contains("--security-opt no-new-privileges"));
    assert!(create.contains("--userns=keep-id:uid=998,gid=998"));
    assert!(create.contains("--cap-drop NET_RAW"));
    assert!(create.contains(":/sandbox/project:rw "));
    assert!(!create.contains(",Z"));
}

#[test]
fn new_podman_bindings_persist_disabled_separation_by_default() {
    let f = Fixture::new("podman");
    let config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    assert_eq!(config["selinux_isolation"], "disabled");
    f.ok(&["up"]);
    let status: Value = serde_json::from_slice(&f.ok(&["status"]).stdout).unwrap();
    assert_eq!(status["selinux_isolation"]["requested"], "disabled");
    assert_eq!(status["selinux_isolation"]["process_label"], "");
}

#[test]
fn offline_default_binding_pins_podman_without_discovery_or_docker_fallback() {
    let f = Fixture::new("podman");
    let directory = f.root.path().join("default-config");
    let output = f.ok(&[
        "init",
        "--yes",
        "--agent",
        "codex",
        "--directory",
        directory.to_str().unwrap(),
    ]);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SELinux container separation DISABLED")
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(directory.join("dungeon.json")).unwrap()).unwrap();
    assert_eq!(saved["container"]["engine"], "podman");
    assert_eq!(saved["selinux_isolation"], "disabled");
    assert!(!f.root.path().join("calls").exists());
}

#[test]
fn offline_machine_default_pins_podman_with_disabled_separation() {
    let f = Fixture::new("podman");
    let directory = f.root.path().join("machine-default");
    let output = f.ok(&[
        "machine",
        "--schema",
        "1",
        "init",
        "--yes",
        "--runtime",
        "container",
        "--directory",
        directory.to_str().unwrap(),
        "--project",
        f.project.to_str().unwrap(),
        "--name",
        "default",
        "--image",
        "localhost/fixture:1",
    ]);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["result"]["environment"]["selinux_isolation"]["requested"],
        "disabled"
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(directory.join("dungeon.json")).unwrap()).unwrap();
    assert_eq!(saved["container"]["engine"], "podman");
    assert_eq!(saved["selinux_isolation"], "disabled");
    assert!(!f.root.path().join("calls").exists());
}

#[test]
fn explicit_private_selection_preserves_offline_auto_engine_discovery() {
    let f = Fixture::new("podman");
    let directory = f.root.path().join("private-auto");
    f.ok(&[
        "init",
        "--yes",
        "--agent",
        "codex",
        "--engine",
        "auto",
        "--selinux-isolation",
        "private",
        "--directory",
        directory.to_str().unwrap(),
    ]);
    let saved: Value =
        serde_json::from_slice(&fs::read(directory.join("dungeon.json")).unwrap()).unwrap();
    assert_eq!(saved["container"]["engine"], "auto");
    assert!(saved.get("selinux_isolation").is_none());
    assert!(!f.root.path().join("calls").exists());
}

#[test]
fn foreign_dungeon_containers_block_uninstall_before_any_deletion() {
    let f = Fixture::new("podman");
    f.ok(&["up"]);
    let installed = f.bin.join("dungeon");
    fs::copy(env!("CARGO_BIN_EXE_dungeon"), &installed).unwrap();
    fs::write(f.root.path().join("extra"), format!("{}\n", "b".repeat(64))).unwrap();
    let before = fs::read(f.state()).unwrap();
    let output = f.run(&["uninstall", "--bin-dir", f.bin.to_str().unwrap(), "--yes"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unrecorded or foreign Dungeon containers"),
        "{output:?}"
    );
    assert_eq!(fs::read(f.state()).unwrap(), before);
    assert!(installed.exists());
    assert!(f.root.path().join("exists").exists());
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
    assert!(
        !calls
            .lines()
            .any(|line| line.contains(" rm ") || line.contains(" stop "))
    );
}

#[test]
fn historical_omitted_selinux_selection_keeps_private_native_identity() {
    let f = Fixture::with_selinux("podman", Some("private"));
    let before = fs::read(&f.config).unwrap();
    let saved: Value = serde_json::from_slice(&before).unwrap();
    assert!(saved.get("selinux_isolation").is_none());
    let plan: Value = serde_json::from_slice(&f.ok(&["plan"]).stdout).unwrap();
    assert_eq!(plan["selinux_isolation"]["requested"], "private");
    f.ok(&["up"]);
    let state = fs::read(f.state()).unwrap();
    f.ok(&["down"]);
    f.ok(&["up"]);
    assert_eq!(fs::read(&f.config).unwrap(), before);
    assert_eq!(fs::read(f.state()).unwrap(), state);
    let status: Value = serde_json::from_slice(&f.ok(&["status"]).stdout).unwrap();
    assert_eq!(status["selinux_isolation"]["requested"], "private");
}

#[test]
fn private_podman_mode_keeps_project_relabeling_and_bound_edit_refusal() {
    let f = Fixture::with_selinux("podman", Some("private"));
    f.ok(&["up"]);
    let before = fs::read(&f.config).unwrap();
    let output = f.run(&[
        "config",
        "--edit",
        "--yes",
        "--selinux-isolation",
        "disabled",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("bound sandbox"));
    assert_eq!(fs::read(&f.config).unwrap(), before);
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
    let create = calls
        .lines()
        .find(|line| line.contains("create --name"))
        .unwrap();
    assert!(create.contains(":/sandbox/project:rw,Z"));
    assert!(!create.contains("label=disable"));
}

#[test]
fn requested_disabled_mode_refuses_private_native_labels_before_start() {
    let f = Fixture::with_selinux("podman", Some("disabled"));
    let mut c: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("stopped.json")).unwrap()).unwrap();
    c[0]["HostConfig"]["SecurityOpt"] = json!(["no-new-privileges"]);
    c[0]["ProcessLabel"] = json!("system_u:system_r:container_t:s0:c123,c456");
    f.write("stopped.json", &c);
    let output = f.run(&["up"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SELinux"));
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
    assert!(!calls.lines().any(|line| line.contains(" start ")));
    assert!(f.state().exists());
}

#[test]
fn selinux_security_option_drift_blocks_work_and_preserves_binding() {
    let f = Fixture::with_selinux("podman", Some("disabled"));
    f.ok(&["up"]);
    let before = fs::read(f.state()).unwrap();
    let mut c: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("inspect.json")).unwrap()).unwrap();
    c[0]["ProcessLabel"] = json!("system_u:system_r:container_t:s0:c123,c456");
    f.write("inspect.json", &c);
    assert!(!f.run(&["exec", "--", "true"]).status.success());
    assert_eq!(fs::read(f.state()).unwrap(), before);
}

#[test]
fn changing_bound_selinux_selection_refuses_before_runtime_contact() {
    let f = Fixture::with_selinux("podman", Some("disabled"));
    f.ok(&["up"]);
    let state = fs::read(f.state()).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    config["selinux_isolation"] = json!("private");
    fs::write(&f.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let calls = fs::read(f.root.path().join("calls")).unwrap();
    let output = f.run(&["up"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("configuration changed"));
    assert_eq!(fs::read(f.state()).unwrap(), state);
    assert_eq!(fs::read(f.root.path().join("calls")).unwrap(), calls);
}

#[test]
fn machine_init_persists_explicit_podman_selinux_selection_offline() {
    let f = Fixture::new("podman");
    let directory = f.root.path().join("machine-config");
    let output = f.ok(&[
        "machine",
        "--schema",
        "1",
        "init",
        "--yes",
        "--runtime",
        "container",
        "--engine",
        "podman",
        "--directory",
        directory.to_str().unwrap(),
        "--project",
        f.project.to_str().unwrap(),
        "--name",
        "machine",
        "--image",
        "localhost/fixture:1",
        "--selinux-isolation",
        "disabled",
    ]);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["result"]["environment"]["selinux_isolation"]["requested"],
        "disabled"
    );
    assert_eq!(
        result["result"]["environment"]["selinux_isolation"]["verification"],
        "not-inspected"
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(directory.join("dungeon.json")).unwrap()).unwrap();
    assert_eq!(saved["selinux_isolation"], "disabled");
    assert!(!f.root.path().join("calls").exists());
    assert!(!directory.join("runtime.json").exists());
}

#[test]
fn disabled_mode_rejects_docker_and_openshell_without_engine_contact() {
    let f = Fixture::new("docker");
    let before = fs::read(&f.config).unwrap();
    let output = f.run(&[
        "config",
        "--edit",
        "--yes",
        "--selinux-isolation",
        "disabled",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("rootless Podman"));
    assert_eq!(fs::read(&f.config).unwrap(), before);
    let private = f.root.path().join("openshell-private");
    let output = f.run(&[
        "init",
        "--yes",
        "--agent",
        "codex",
        "--runtime",
        "openshell",
        "--directory",
        private.to_str().unwrap(),
        "--selinux-isolation",
        "disabled",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("OpenShell 0.0.116"));
    assert!(!private.exists());
    assert!(!f.root.path().join("calls").exists());
}

#[test]
fn headless_run_keeps_stdout_clean_on_first_launch_ready_and_resume() {
    for engine in ["podman", "docker"] {
        let f = Fixture::new(engine);
        for phase in ["fresh", "ready", "stopped"] {
            if phase == "stopped" {
                f.ok(&["down"]);
            }
            let out = f.run(&["run", "codex", "--", "output-fixture"]);
            assert_eq!(out.status.code(), Some(23), "{engine}/{phase}: {out:?}");
            assert_eq!(out.stdout, b"{\"fixture\":true}\n", "{engine}/{phase}");
            assert!(String::from_utf8_lossy(&out.stderr).contains("guest diagnostic"));
            if phase == "fresh" {
                assert!(String::from_utf8_lossy(&out.stderr).contains("Container ready"));
            }
        }
    }
}

#[test]
fn setup_prepares_images_and_saves_only_new_project_defaults() {
    for engine in ["podman", "docker"] {
        let f = Fixture::new(engine);
        let original = fs::read(&f.config).unwrap();
        let endpoint = format!("unix://{}", f.root.path().join("engine.sock").display());
        let setup = |image: &str| {
            f.run(&[
                "setup",
                "--yes",
                "--runtime",
                "container",
                "--engine",
                engine,
                "--endpoint",
                &endpoint,
                "--image",
                image,
            ])
        };
        let preferences = f.home.join(".config/dungeon/container-defaults.json");
        let image = "ghcr.io/nvidia/openshell-community/sandboxes/base:latest";
        let output = setup(image);
        assert!(output.status.success(), "{output:?}");
        let saved: Value = serde_json::from_slice(&fs::read(&preferences).unwrap()).unwrap();
        assert_eq!(saved["runtime"], "container");
        assert_eq!(saved["image"], image);
        assert_eq!(saved["container"]["engine"], engine);
        assert_eq!(saved["container"]["endpoint"], endpoint);
        let pin = format!("ghcr.io/example/agent@sha256:{}", "a".repeat(64));
        assert!(setup(&pin).status.success());
        let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
        assert!(calls.contains(&format!("pull {image}")));
        assert!(calls.contains(&format!("pull {pin}")));
        assert!(!calls.contains("create --name"));
        assert_eq!(fs::read(&f.config).unwrap(), original);
        assert!(!f.state().exists());
        let second = f.root.path().join("second-project");
        fs::create_dir(&second).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .args(["init", "--yes", "--agent", "claude"])
            .env_clear()
            .env("HOME", &f.home)
            .env("PATH", &f.bin)
            .current_dir(&second)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert!(String::from_utf8_lossy(&out.stdout).contains(&pin));
        assert_eq!(fs::read(&f.config).unwrap(), original);
        let before = fs::read(&preferences).unwrap();
        fs::write(f.root.path().join("fail-pull"), "").unwrap();
        assert!(!setup(image).status.success());
        assert_eq!(fs::read(&preferences).unwrap(), before);
    }
}

#[test]
fn setup_dry_run_and_missing_approval_do_not_pull_or_save() {
    let f = Fixture::new("docker");
    let endpoint = format!("unix://{}", f.root.path().join("engine.sock").display());
    let args = [
        "setup",
        "--runtime",
        "container",
        "--engine",
        "docker",
        "--endpoint",
        &endpoint,
    ];
    assert!(!f.run(&args).status.success());
    let mut args = args.to_vec();
    args.push("--dry-run");
    let out = f.run(&args);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("Would pull image:"));
    assert!(
        !f.home
            .join(".config/dungeon/container-defaults.json")
            .exists()
    );
    let calls = fs::read_to_string(f.root.path().join("calls")).unwrap();
    assert!(!calls.contains("pull "));
    assert!(!calls.contains("image inspect"));
}

#[test]
fn setup_preserves_invalid_defaults_and_never_repairs_an_explicit_endpoint() {
    let f = Fixture::new("docker");
    let path = f.home.join(".config/dungeon/container-defaults.json");
    fs::write(&path, b"{bad private defaults").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let endpoint = format!("unix://{}", f.root.path().join("engine.sock").display());
    let args = [
        "setup",
        "--yes",
        "--runtime",
        "container",
        "--engine",
        "docker",
        "--endpoint",
        &endpoint,
    ];
    assert!(!f.run(&args).status.success());
    assert_eq!(fs::read(&path).unwrap(), b"{bad private defaults");
    fs::remove_file(&path).unwrap();
    fs::remove_file(f.root.path().join("engine.sock")).unwrap();
    assert!(!f.run(&args).status.success());
    assert!(!path.exists());
    assert!(!f.root.path().join("calls").exists());
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
fn docker_retains_its_existing_native_label_contract() {
    let f = Fixture::new("docker");
    let mut c: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("stopped.json")).unwrap()).unwrap();
    c[0]["HostConfig"]["SecurityOpt"] = json!(["no-new-privileges", "label=disable"]);
    f.write("stopped.json", &c);
    let mut running = c;
    running[0]["State"]["Running"] = json!(true);
    f.write("running.json", &running);
    f.ok(&["up"]);
    let status: Value = serde_json::from_slice(&f.ok(&["status"]).stdout).unwrap();
    assert_eq!(status["selinux_isolation"]["verification"], "not-inspected");
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
