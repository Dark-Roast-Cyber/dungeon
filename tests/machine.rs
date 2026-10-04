#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["home", "project", "bin"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        let config = root.path().join("private/dungeon.json");
        let f = Self { root, config };
        let initialized = f
            .command()
            .args(["machine", "--schema", "1", "init", "--yes", "--directory"])
            .arg(f.config.parent().unwrap())
            .arg("--project")
            .arg(f.root.path().join("project"))
            .args([
                "--name",
                "fixture",
                "--gateway",
                "local",
                "--image",
                "localhost/synthetic:1",
            ])
            .output()
            .unwrap();
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stdout)
        );
        let plan = f
            .command()
            .arg("--config")
            .arg(&f.config)
            .arg("plan")
            .output()
            .unwrap();
        let fingerprint: Value = serde_json::from_slice(&plan.stdout).unwrap();
        let policy: Value = serde_json::from_slice(
            &fs::read(f.config.parent().unwrap().join("policy.json")).unwrap(),
        )
        .unwrap();
        let digest = data_encoding::BASE32_NOPAD
            .encode(&Sha256::digest(serde_json::to_vec(&policy).unwrap()))
            .to_ascii_lowercase();
        fs::write(f.config.parent().unwrap().join("runtime.json"),json!({"schema":1,"id":"opaque-id","config":fingerprint["fingerprint"],"observed_policy":digest}).to_string()).unwrap();
        let remote = json!({"id":"opaque-id","name":"fixture","workspace":"default","phase":"Ready","labels":{"agentarchaeology.ai/dungeon-config":fingerprint["fingerprint"]},"policy":policy});
        fs::write(f.root.path().join("remote.json"), remote.to_string()).unwrap();
        let stub = f.root.path().join("bin/openshell");
        fs::write(
            &stub,
            format!(
                r#"#!/bin/sh
if [ "$1" = --version ]; then echo 'openshell 0.0.116'; exit 0; fi
shift 4
if [ "$2" = get ]; then cat '{}'; exit 0; fi
if [ "$2" = exec ]; then
  while [ "$1" != -- ]; do shift; done
  shift
  exec "$@"
fi
echo 'SYNTHETIC_SECRET_NATIVE_ERROR' >&2
exit 9
"#,
                f.root.path().join("remote.json").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
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
            .current_dir(self.root.path().join("project"));
        c
    }
    fn machine(&self) -> Command {
        let mut c = self.command();
        c.arg("--config")
            .arg(&self.config)
            .args(["machine", "--schema", "1"]);
        c
    }
}

#[test]
fn fixed_protocol_parse_resolution_and_native_errors() {
    let f = Fixture::new();
    for args in [
        vec!["--schema", "999", "status"],
        vec!["--schema", "1", "exec", "--env", "SYNTHETIC_SECRET"],
        vec!["--schema", "1", "--SYNTHETIC_SECRET"],
    ] {
        let out = f.command().arg("machine").args(args).output().unwrap();
        assert!(!out.status.success());
        assert!(out.stderr.is_empty());
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["error"]["code"], "invalid_request");
        assert!(
            !String::from_utf8(out.stdout)
                .unwrap()
                .contains("SYNTHETIC_SECRET")
        );
    }
    let out = f
        .command()
        .args(["machine", "--schema", "1", "resolve"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["result"]["bound"], false);
    assert!(!f.root.path().join("home/.config").exists());
    let status = f.machine().arg("status").output().unwrap();
    let v: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(v["result"]["environment"]["sandbox_id"], "opaque-id");
    assert!(v["result"].get("policy").unwrap().is_string());
    let out = f.machine().args(["upload", "--yes"]).output().unwrap();
    assert!(!out.status.success());
    assert!(out.stderr.is_empty());
    assert!(
        !String::from_utf8(out.stdout)
            .unwrap()
            .contains("SYNTHETIC_SECRET")
    );
    fs::write(
        &f.config,
        b"{\"SYNTHETIC_SECRET\\u001b[31m\":1,\"SYNTHETIC_SECRET\\u001b[31m\":2}",
    )
    .unwrap();
    let out = f.machine().arg("status").output().unwrap();
    assert!(out.stderr.is_empty());
    assert!(
        !String::from_utf8(out.stdout)
            .unwrap()
            .contains("SYNTHETIC_SECRET")
    );
}

#[test]
fn streamed_unowned_exec_is_incremental_binary_exact_and_stdin_is_finite() {
    let f = Fixture::new();
    let release = f.root.path().join("release");
    let app = format!(
        "const fs=require('fs');process.stdout.write(Buffer.from([0,255,226]));process.stderr.write('first\\n');const t=setInterval(()=>{{if(fs.existsSync({})){{clearInterval(t);process.stdout.write(Buffer.from([130,172]));process.stderr.write('tail');process.exit(23)}}}},10)",
        serde_json::to_string(&release).unwrap()
    );
    let mut child = f
        .machine()
        .args(["exec", "--", "/usr/bin/node", "-e", &app])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let reader = BufReader::new(child.stdout.take().unwrap());
    let mut stdout = vec![];
    let mut stderr = vec![];
    let mut final_event = None;
    for (seq, line) in reader.lines().enumerate() {
        let event: Value = serde_json::from_str(&line.unwrap()).unwrap();
        assert_eq!(event["seq"], seq);
        match event["event"].as_str().unwrap() {
            "started" => {}
            "output" => {
                let bytes = data_encoding::BASE64
                    .decode(event["data"].as_str().unwrap().as_bytes())
                    .unwrap();
                if event["stream"] == "stdout" {
                    stdout.extend(bytes)
                } else {
                    stderr.extend(bytes)
                };
                if !stdout.is_empty() && !stderr.is_empty() && !release.exists() {
                    assert!(child.try_wait().unwrap().is_none());
                    fs::write(&release, b"go").unwrap();
                }
            }
            "finished" => final_event = Some(event),
            _ => panic!("unknown event"),
        }
    }
    assert_eq!(child.wait().unwrap().code(), Some(23));
    assert_eq!(stdout, [0, 255, 226, 130, 172]);
    assert_eq!(stderr, b"first\ntail");
    assert_eq!(final_event.unwrap()["exit_code"], 23);
    let mut child=f.machine().args(["exec","--stdin","--","/usr/bin/node","-e","process.stdin.on('data',b=>process.stdout.write(b));process.stdin.on('end',()=>process.exit(23))"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"finite\0input")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(23));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("ZmluaXRlAGlucHV0")
    );
    for args in [
        vec!["exec", "--owned", "--stdin", "--", "true"],
        vec!["exec", "--cwd", "/sandbox/project/../other", "--", "true"],
        vec!["close"],
    ] {
        let out = f.machine().args(args).output().unwrap();
        assert!(!out.status.success());
        assert!(out.stderr.is_empty());
    }
    let out = f
        .machine()
        .args(["exec", "--", "true"])
        .stdout(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn stalled_owned_sink_cleans_guest_group_without_requiring_output_eof() {
    let f = Fixture::new();
    let marker = f.root.path().join("group");
    let app = format!(
        "const fs=require('fs');fs.writeFileSync({},fs.readFileSync('/proc/self/stat','utf8').split(') ')[1].split(' ')[2]);console.log('ready');function f(){{if(process.stdout.write(Buffer.alloc(4096)))setImmediate(f);else process.stdout.once('drain',f)}}f();setInterval(()=>{{}},1000)",
        serde_json::to_string(&marker).unwrap()
    );
    let mut child = f
        .machine()
        .args(["exec", "--owned", "--", "/usr/bin/node", "-e", &app])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["event"] == "output" {
            break;
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "owned stalled consumer did not terminate"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert!(!status.success());
    let group = fs::read_to_string(marker).unwrap().parse::<i32>().unwrap();
    assert_eq!(unsafe { libc::kill(-group, 0) }, -1);
}

#[test]
fn malformed_primary_stream_cannot_be_overridden_by_a_clean_receipt() {
    let f = Fixture::new();
    let stub = f.root.path().join("bin/openshell");
    let source = fs::read_to_string(&stub).unwrap();
    fs::write(&stub,source.replace("  exec \"$@\"", "  case \"$*\" in *machine-launch*) printf '%s\\n' '{\"v\":1,\"seq\":0,\"seq\":0,\"event\":\"output\",\"stream\":\"stdout\",\"data\":\"QQ==\"}';; esac\n  exec \"$@\"" )).unwrap();
    let out = f
        .machine()
        .args([
            "exec",
            "--owned",
            "--",
            "/usr/bin/node",
            "-e",
            "setInterval(()=>{},1000)",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stderr.is_empty());
    let events = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let final_event = events.last().unwrap();
    assert_eq!(final_event["event"], "finished");
    assert_eq!(final_event["ok"], false);
    assert!(final_event["exit_code"].is_null());
}

#[test]
fn opaque_identity_too_large_for_started_event_never_launches_work() {
    let f = Fixture::new();
    let id = "x".repeat(65536);
    for path in [
        f.root.path().join("remote.json"),
        f.config.parent().unwrap().join("runtime.json"),
    ] {
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["id"] = json!(id);
        fs::write(path, value.to_string()).unwrap();
    }
    let stub = f.root.path().join("bin/openshell");
    let source = fs::read_to_string(&stub).unwrap();
    let marker = f.root.path().join("launched");
    fs::write(
        stub,
        source.replace(
            "if [ \"$2\" = exec ]; then",
            &format!("if [ \"$2\" = exec ]; then\n touch '{}'", marker.display()),
        ),
    )
    .unwrap();
    let out = f
        .machine()
        .args(["exec", "--owned", "--", "true"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    assert!(!marker.exists());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["event"], "finished");
    assert_eq!(v["ok"], false);
    assert_eq!(v["seq"], 0);
}

#[test]
fn post_mutation_identity_failures_keep_category_but_never_claim_no_effects() {
    for operation in ["create", "stop"] {
        let f = Fixture::new();
        if operation == "create" {
            fs::remove_file(f.config.parent().unwrap().join("runtime.json")).unwrap();
        }
        let mut bad: Value =
            serde_json::from_slice(&fs::read(f.root.path().join("remote.json")).unwrap()).unwrap();
        bad["labels"]["agentarchaeology.ai/dungeon-config"] = json!("synthetic-wrong-label");
        fs::write(f.root.path().join("bad-remote.json"), bad.to_string()).unwrap();
        let stub = f.root.path().join("bin/openshell");
        let source = fs::read_to_string(&stub).unwrap();
        let marker = f.root.path().join("mutated");
        fs::write(stub,source.replace("shift 4",&format!("shift 4\nif [ \"$2\" = {operation} ]; then touch '{}'; cp '{}' '{}'; exit 0; fi",marker.display(),f.root.path().join("bad-remote.json").display(),f.root.path().join("remote.json").display()))).unwrap();
        let mut c = f.machine();
        c.arg(operation);
        if operation == "create" {
            c.arg("--yes");
        }
        let out = c.output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stderr.is_empty());
        assert!(marker.exists());
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["error"]["code"], "identity_mismatch");
        assert_eq!(value["error"]["effects"], "unknown");
    }
}

#[test]
fn unowned_exec_postlaunch_drift_is_a_fixed_unknown_effects_completion() {
    let f = Fixture::new();
    let remote = f.root.path().join("remote.json");
    let token_file = f.root.path().join("invocation-token");
    let app = format!(
        "const fs=require('fs');const argv=fs.readFileSync('/proc/'+process.ppid+'/cmdline','utf8').split('\\0');fs.writeFileSync({},argv[argv.indexOf('stream-launch')+1]);const path={};const v=JSON.parse(fs.readFileSync(path));v.labels['agentarchaeology.ai/dungeon-config']='changed-after-launch';fs.writeFileSync(path,JSON.stringify(v));process.stdout.write('ran\\n');process.exit(23)",
        serde_json::to_string(&token_file).unwrap(),
        serde_json::to_string(&remote).unwrap()
    );
    let out = f
        .machine()
        .args(["exec", "--", "/usr/bin/node", "-e", &app])
        .output()
        .unwrap();
    // Work has finished; remove only the fixture's independently recorded private controls.
    let token = fs::read_to_string(token_file).unwrap();
    assert!(token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()));
    fs::remove_dir_all(format!("/tmp/dungeon-owned-{token}")).unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    let events = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events[0]["event"], "started");
    assert!(
        events
            .iter()
            .any(|v| v["event"] == "output" && v["data"] == "cmFuCg==")
    );
    let final_event = events.last().unwrap();
    assert_eq!(final_event["event"], "finished");
    assert_eq!(final_event["ok"], false);
    assert!(final_event["exit_code"].is_null());
    assert_eq!(final_event["error"]["code"], "identity_mismatch");
    assert_eq!(final_event["error"]["effects"], "unknown");
    assert!(!final_event.to_string().contains("changed-after-launch"));
}

#[test]
fn unapproved_import_transfer_refuses_before_any_version_probe() {
    let f = Fixture::new();
    let source = f.root.path().join("nvim-source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("init.lua"), "return {}").unwrap();
    let mut doc: Value = serde_json::from_slice(&fs::read(&f.config).unwrap()).unwrap();
    doc["imports"] = json!([{"source":source,"destination":"/sandbox/.config/nvim"}]);
    fs::write(&f.config, doc.to_string()).unwrap();
    let marker = f.root.path().join("version-probed");
    let stub = f.root.path().join("bin/openshell");
    let content = fs::read_to_string(&stub).unwrap();
    fs::write(
        &stub,
        content.replace(
            "if [ \"$1\" = --version ]; then",
            &format!(
                "if [ \"$1\" = --version ]; then touch '{}';",
                marker.display()
            ),
        ),
    )
    .unwrap();
    for operation in ["upload", "export"] {
        let out = f
            .command()
            .arg("--config")
            .arg(&f.config)
            .args([operation, "--yes"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .contains("not approved")
        );
        assert!(!marker.exists());
        let out = f.machine().args([operation, "--yes"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stderr.is_empty());
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "operation_failed");
        assert!(!marker.exists());
    }
}
