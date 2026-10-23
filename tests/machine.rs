#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::ops::{Deref, DerefMut};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};

struct TestChild(Option<Child>);
impl Deref for TestChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0.as_ref().unwrap()
    }
}
impl DerefMut for TestChild {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }
}
impl TestChild {
    fn wait_with_output(mut self) -> io::Result<Output> {
        self.0.take().unwrap().wait_with_output()
    }
}
impl Drop for TestChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                unsafe {
                    libc::kill(child.id() as i32, libc::SIGTERM);
                }
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                while child.try_wait().ok().flatten().is_none()
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                if child.try_wait().ok().flatten().is_none() {
                    let _ = child.kill();
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
        for name in ["home", "project", "bin", "guest-project", "guest-controls"] {
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
        // Host tooling is resolved before the synthetic guest's narrowed PATH.
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .filter(|p| p.is_absolute())
            .map(|p| p.join("node"))
            .find(|p| p.is_file())
            .expect("machine fixtures require provisioned host Node on PATH")
            .canonicalize()
            .unwrap();
        assert!(!node.starts_with(f.root.path()));
        assert!(
            Command::new(&node)
                .arg("--version")
                .output()
                .unwrap()
                .status
                .success()
        );
        let adapter = f.root.path().join("guest-adapter.cjs");
        fs::write(&adapter,format!(r#"'use strict';
const fs=require('node:fs'),assert=require('node:assert/strict');
const [flag,shim,separator,mode,token,...argv]=process.argv.slice(2);
assert.equal(flag,'-e');assert.equal(separator,'--');assert.equal(shim,{shim});
const controls={controls},project={project},registry={registry};
process.chdir(project);
const fields=p=>fs.readFileSync('/proc/'+p+'/stat','utf8').split(') ')[1].split(' ');
const record=p=>{{const f=fields(p);fs.appendFileSync(registry,JSON.stringify({{pid:p,group:+f[2],start:f[19]}})+'\n')}};
record(process.pid);record(process.ppid);
if(argv[0]==='/usr/bin/node')argv[0]=process.execPath;
process.argv=[process.execPath,mode,token,...argv];
let translated=shim.replace('const directory = `/tmp/dungeon-owned-${{token}}`;', 'const directory = '+JSON.stringify(controls) +' + "/dungeon-owned-" + token;');
assert.notEqual(translated,shim);
translated=translated.replace("process.chdir('/sandbox/project');",'process.chdir('+JSON.stringify(project)+');');
translated=translated.replace("process.send('ready');",'const fixtureFields=fs.readFileSync("/proc/self/stat","utf8").split(") ")[1].split(" ");fs.appendFileSync('+JSON.stringify(registry)+',JSON.stringify({{pid:process.pid,group:+fixtureFields[2],start:fixtureFields[19]}})+"\\n");process.send("ready");');
if(mode==='machine-launch' && fs.existsSync({malformed}))process.stdout.write('{{"v":1,"seq":0,"seq":0,"event":"output","stream":"stdout","data":"QQ=="}}\n');
if(mode==='machine-launch' && fs.existsSync({primary_failure}))process.once('beforeExit',()=>{{process.kill(process.ppid,'SIGTERM');process.exitCode=99}});
if(mode==='machine-launch' && fs.existsSync({buffered_primary})){{
  const write=process.stdout.write.bind(process.stdout),chunks=[];
  process.stdout.write=(bytes,callback)=>{{chunks.push(Buffer.from(bytes));if(callback)setImmediate(callback);return true}};
  process.once('beforeExit',()=>{{process.stdout.write=write;for(const bytes of chunks)write(bytes)}});
}}
eval(translated);
"#,
            shim=serde_json::to_string(include_str!("../src/owned.cjs")).unwrap(),controls=serde_json::to_string(&f.root.path().join("guest-controls")).unwrap(),project=serde_json::to_string(&f.root.path().join("guest-project")).unwrap(),registry=serde_json::to_string(&f.root.path().join("guest-processes.jsonl")).unwrap(),malformed=serde_json::to_string(&f.root.path().join("malformed-primary")).unwrap(),primary_failure=serde_json::to_string(&f.root.path().join("primary-failure")).unwrap(),buffered_primary=serde_json::to_string(&f.root.path().join("buffered-primary")).unwrap())).unwrap();
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
  test "$1" = /usr/bin/node || exit 98
  shift
  set -- '{}' '{}' "$@"
  exec "$@"
fi
echo 'SYNTHETIC_SECRET_NATIVE_ERROR' >&2
exit 9
"#,
                f.root.path().join("remote.json").display(),
                node.display(),
                adapter.display()
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
        c.process_group(0);
        c
    }
    fn machine(&self) -> Command {
        let mut c = self.command();
        c.arg("--config")
            .arg(&self.config)
            .args(["machine", "--schema", "1"]);
        c
    }
    fn spawn(&self, command: &mut Command) -> TestChild {
        TestChild(Some(command.spawn().unwrap()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(records) = fs::read_to_string(self.root.path().join("guest-processes.jsonl")) {
            for line in records.lines().rev() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                let Some(pid) = value["pid"].as_u64() else {
                    continue;
                };
                let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
                    continue;
                };
                let Some((_, fields)) = stat.rsplit_once(") ") else {
                    continue;
                };
                let fields = fields.split_whitespace().collect::<Vec<_>>();
                if fields.len() > 19
                    && Some(fields[19]) == value["start"].as_str()
                    && fields[2].parse::<u64>().ok() == value["group"].as_u64()
                {
                    unsafe {
                        let target = if value["group"].as_u64() == Some(pid) {
                            -(pid as i32)
                        } else {
                            pid as i32
                        };
                        libc::kill(target, libc::SIGKILL);
                    }
                }
            }
        }
        // Temporary guest controls belong solely to this fixture's private tree.
    }
}

#[test]
fn owned_exec_accepts_subprocess_stream_sockets_with_exact_bytes_and_exit() {
    let f = Fixture::new();
    let (reader, writer) = UnixStream::pair().unwrap();
    let output = std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut events = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            events.push(serde_json::from_str::<Value>(&line).unwrap());
        }
        events
    });
    use std::os::fd::OwnedFd;
    let child = f.spawn(
        f.machine()
            .args([
                "exec", "--owned", "--", "/usr/bin/node", "-e",
                "process.stdout.write(Buffer.from([0,255,10]));process.stderr.write('note');process.exitCode=23",
            ])
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::piped()),
    );
    let result = child.wait_with_output().unwrap();
    let events = output.join().unwrap();
    assert_eq!(result.status.code(), Some(23), "{events:?}");
    assert!(result.stderr.is_empty());
    assert_eq!(events[0]["event"], "started");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    for (seq, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], seq);
        if event["event"] == "output" {
            let bytes = data_encoding::BASE64
                .decode(event["data"].as_str().unwrap().as_bytes())
                .unwrap();
            if event["stream"] == "stdout" {
                stdout.extend(bytes);
            } else {
                stderr.extend(bytes);
            }
        }
    }
    assert_eq!(stdout, [0, 255, 10]);
    assert_eq!(stderr, b"note");
    let terminal = events.last().unwrap();
    assert_eq!(terminal["event"], "finished");
    assert_eq!(terminal["ok"], true);
    assert_eq!(terminal["exit_code"], 23);
    assert_eq!(terminal["cleanup"], "confirmed");
    assert_eq!(terminal["primary_transport_verified"], true);
}

#[test]
fn synthetic_guest_paths_and_cleanup_are_fixture_private() {
    let f = Fixture::new();
    let root = f.root.path().to_owned();
    let marker = root.join("private-workload");
    let app = format!(
        "const fs=require('fs');fs.writeFileSync({},JSON.stringify({{cwd:process.cwd(),pid:process.pid,group:+fs.readFileSync('/proc/self/stat','utf8').split(') ')[1].split(' ')[2]}}));console.log('ready');setInterval(()=>{{}},1000)",
        serde_json::to_string(&marker).unwrap()
    );
    let mut child = f.spawn(
        f.machine()
            .args(["exec", "--owned", "--", "/usr/bin/node", "-e", &app])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        let event: Value = serde_json::from_str(&line).unwrap();
        if event["event"] == "output" {
            break;
        }
        assert_ne!(event["event"], "finished");
    }
    let metadata: Value = serde_json::from_slice(&fs::read(marker).unwrap()).unwrap();
    assert_eq!(
        metadata["cwd"],
        root.join("guest-project").to_str().unwrap()
    );
    let group = metadata["group"].as_i64().unwrap() as i32;
    let controls = fs::read_dir(root.join("guest-controls"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1);
    assert!(
        !PathBuf::from("/tmp")
            .join(controls[0].file_name().unwrap())
            .exists()
    );
    // A pending initialization gate must not disable already-owned cleanup.
    let state_path = f.config.parent().unwrap().join("runtime.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["initial_upload_pending"] = json!(true);
    fs::write(&state_path, state.to_string()).unwrap();
    fs::set_permissions(&state_path, fs::Permissions::from_mode(0o600)).unwrap();
    let refused = f
        .machine()
        .args([
            "exec",
            "--owned",
            "--",
            "/usr/bin/node",
            "-e",
            "process.exit(99)",
        ])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    let refused: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refused["error"]["effects"], "none");
    assert_eq!(refused["error"]["code"], "operation_failed");
    // These guards also run during assertion unwinding, without a global guest directory.
    drop(child);
    assert_eq!(
        fs::read_dir(root.join("guest-controls")).unwrap().count(),
        0
    );
    assert_eq!(unsafe { libc::kill(-group, 0) }, -1);
    drop(f);
    assert_eq!(unsafe { libc::kill(-group, 0) }, -1);
    assert!(!root.exists());
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
    let mut child = f.spawn(
        f.machine()
            .args(["exec", "--", "/usr/bin/node", "-e", &app])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
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
    let final_event = final_event.unwrap();
    assert_eq!(final_event["exit_code"], 23);
    assert_eq!(final_event["cleanup"], "not_owned");
    assert_eq!(final_event["primary_transport_verified"], true);
    let mut child=f.spawn(f.machine().args(["exec","--stdin","--","/usr/bin/node","-e","process.stdin.on('data',b=>process.stdout.write(b));process.stdin.on('end',()=>process.exit(23))"]).stdin(Stdio::piped()).stdout(Stdio::piped()));
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
    let mut child = f.spawn(
        f.machine()
            .args(["exec", "--owned", "--", "/usr/bin/node", "-e", &app])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
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
    assert_eq!(
        fs::read_dir(f.root.path().join("guest-controls"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn malformed_primary_stream_cannot_be_overridden_by_a_clean_receipt() {
    let f = Fixture::new();
    fs::write(f.root.path().join("malformed-primary"), b"1").unwrap();
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
    assert_eq!(final_event["cleanup"], "confirmed");
    assert_eq!(final_event["primary_transport_verified"], true);
    assert_eq!(
        fs::read_dir(f.root.path().join("guest-controls"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn owned_term_with_buffered_output_confirms_cleanup() {
    for buffered in [false, true] {
        let f = Fixture::new();
        if buffered {
            fs::write(f.root.path().join("buffered-primary"), b"1").unwrap();
        }
        let mut child = f.spawn(
            f.machine()
                .args([
                    "exec",
                    "--owned",
                    "--",
                    "/usr/bin/node",
                    "-e",
                    "process.stdout.write(Buffer.alloc(4*1024*1024));setInterval(()=>{},1000)",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        );
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut next_seq = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let event: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(event["seq"], next_seq);
            next_seq += 1;
            if event["event"] == "started" {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
        let tail = std::thread::spawn(move || {
            reader
                .lines()
                .map(|line| serde_json::from_str::<Value>(&line.unwrap()).unwrap())
                .collect::<Vec<_>>()
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "buffered TERM cleanup hung"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(child.wait().unwrap().code(), Some(1));
        let events = tail.join().unwrap();
        for event in &events {
            assert_eq!(event["seq"], next_seq);
            next_seq += 1;
        }
        let terminal = events.last().unwrap();
        assert_eq!(terminal["event"], "finished");
        assert_eq!(terminal["ok"], false);
        assert_eq!(terminal["cleanup"], "confirmed");
        assert_eq!(terminal["primary_transport_verified"], true);
        assert_eq!(
            fs::read_dir(f.root.path().join("guest-controls"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn failed_primary_with_pending_signal_retains_unconfirmed_controls() {
    let f = Fixture::new();
    fs::write(f.root.path().join("primary-failure"), b"1").unwrap();
    let out = f
        .machine()
        .args([
            "exec",
            "--owned",
            "--",
            "/usr/bin/node",
            "-e",
            "process.exitCode=0",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    let events = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let terminal = events.last().unwrap();
    assert_eq!(terminal["event"], "finished");
    assert_eq!(terminal["ok"], false);
    assert!(terminal["exit_code"].is_null());
    assert_eq!(terminal["cleanup"], "unconfirmed");
    assert_eq!(terminal["primary_transport_verified"], false);
    let controls = fs::read_dir(f.root.path().join("guest-controls"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1);
    assert!(controls[0].join("result").is_file());
}

#[test]
fn guardian_death_with_retained_framed_pipes_is_bounded_cleanup_unconfirmed() {
    let f = Fixture::new();
    let registry = f.root.path().join("guest-processes.jsonl");
    let app = format!(
        "const fs=require('fs');const fields=fs.readFileSync('/proc/self/stat','utf8').split(') ')[1].split(' ');fs.appendFileSync({},JSON.stringify({{pid:process.pid,group:+fields[2],start:fields[19]}})+'\\n');process.on('SIGTERM',()=>{{}});process.kill(process.ppid,'SIGKILL');setInterval(()=>{{}},1000)",
        serde_json::to_string(&registry).unwrap()
    );
    let mut child = f.spawn(
        f.machine()
            .args(["exec", "--owned", "--", "/usr/bin/node", "-e", &app])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "guardian fault hung the actual CLI"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    let events = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events[0]["event"], "started");
    let final_event = events.last().unwrap();
    assert_eq!(final_event["event"], "finished");
    assert_eq!(final_event["ok"], false);
    assert!(final_event["exit_code"].is_null());
    assert_eq!(final_event["error"]["code"], "cleanup_unconfirmed");
    assert_eq!(final_event["error"]["effects"], "unknown");
    assert_eq!(final_event["cleanup"], "unconfirmed");
    assert_eq!(final_event["primary_transport_verified"], false);
    let controls = fs::read_dir(f.root.path().join("guest-controls"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 1);
    assert!(!controls[0].join("result").exists());
    // Fixture Drop kills only the independently recorded, identity-matching survivor.
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
    fs::remove_dir_all(
        f.root
            .path()
            .join("guest-controls")
            .join(format!("dungeon-owned-{token}")),
    )
    .unwrap();
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
