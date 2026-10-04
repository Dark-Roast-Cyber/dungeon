//! Native terminal ownership; non-TTY execution retains its separate contract.
use super::*;
use crate::config::read_json_text;
use std::io::{IsTerminal, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Status {
    code: Option<u8>,
    signal: Option<String>,
    spawn_error: bool,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Receipt {
    token: String,
    clean: bool,
    stopped: bool,
    fault: bool,
    status: Option<Status>,
}

fn receipt(bytes: &[u8], token: &str) -> Result<Receipt> {
    let value =
        read_json_text(std::str::from_utf8(bytes).map_err(|_| Error::new("invalid SSH receipt"))?)?;
    if value.as_object().is_none_or(|v| v.len() != 5)
        || value.get("status").is_none()
        || value["status"]
            .as_object()
            .is_some_and(|s| s.len() != 3 || !s.contains_key("code") || !s.contains_key("signal"))
    {
        return Err(Error::new("incomplete SSH receipt"));
    }
    let result: Receipt = serde_json::from_value(value)?;
    if result.token != token || !result.clean {
        return Err(Error::new("SSH receipt identity/cleanup mismatch"));
    }
    if let Some(status) = &result.status {
        let valid = match (status.code, status.signal.as_deref(), status.spawn_error) {
            (Some(_), None, false) | (None, None, true) => true,
            (None, Some(signal), false) => signal_number(signal).is_some(),
            _ => false,
        };
        if !valid {
            return Err(Error::new("inconsistent SSH application status"));
        }
    }
    Ok(result)
}

fn signal_number(name: &str) -> Option<i32> {
    Some(match name {
        "SIGHUP" => 1,
        "SIGINT" => 2,
        "SIGQUIT" => 3,
        "SIGILL" => 4,
        "SIGTRAP" => 5,
        "SIGABRT" => 6,
        "SIGBUS" => 7,
        "SIGFPE" => 8,
        "SIGKILL" => 9,
        "SIGUSR1" => 10,
        "SIGSEGV" => 11,
        "SIGUSR2" => 12,
        "SIGPIPE" => 13,
        "SIGALRM" => 14,
        "SIGTERM" => 15,
        "SIGSTKFLT" => 16,
        "SIGCHLD" => 17,
        "SIGCONT" => 18,
        "SIGSTOP" => 19,
        "SIGTSTP" => 20,
        "SIGTTIN" => 21,
        "SIGTTOU" => 22,
        "SIGURG" => 23,
        "SIGXCPU" => 24,
        "SIGXFSZ" => 25,
        "SIGVTALRM" => 26,
        "SIGPROF" => 27,
        "SIGWINCH" => 28,
        "SIGIO" => 29,
        "SIGPWR" => 30,
        "SIGSYS" => 31,
        _ => return None,
    })
}

fn returned(receipt: &Receipt, failure: bool, signal: i32) -> Result<i32> {
    if failure || receipt.fault {
        return Err(Error::new(
            "owned SSH cleanup confirmed but transport/guardian failed",
        ));
    }
    if signal != 0 {
        if !receipt.stopped {
            return Err(Error::new("owned SSH interrupt was not acknowledged"));
        }
        return Ok(128 + signal);
    }
    match &receipt.status {
        Some(Status {
            code: Some(code), ..
        }) => Ok(i32::from(*code)),
        Some(Status {
            signal: Some(signal),
            ..
        }) => Ok(128 + signal_number(signal).expect("validated signal")),
        Some(Status {
            spawn_error: true, ..
        }) => Ok(127),
        _ => Err(Error::new("owned SSH application exit status unavailable")),
    }
}

enum Completion {
    Unlaunched(i32),
    Launched(Receipt, bool),
}

fn final_result(completion: Result<Completion>) -> Result<i32> {
    match completion? {
        Completion::Unlaunched(signal) => Ok(128 + signal),
        Completion::Launched(receipt, failure) => {
            returned(&receipt, failure, SIGNAL.load(Ordering::SeqCst))
        }
    }
}

fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn ssh_config(runtime: &Runtime<'_>) -> Result<String> {
    let executable = runtime
        .binary
        .to_str()
        .ok_or_else(|| Error::new("SSH proxy path is not UTF-8"))?;
    // OpenSSH expands percent tokens even inside shell quotes.
    if executable.contains(['%', '\n', '\r', '\0']) {
        return Err(Error::new("unsupported SSH proxy path"));
    }
    Ok(format!(
        "Host openshell-{}.{}\n    User sandbox\n    StrictHostKeyChecking no\n    UserKnownHostsFile /dev/null\n    GlobalKnownHostsFile /dev/null\n    LogLevel ERROR\n    ServerAliveInterval 15\n    ServerAliveCountMax 3\n    ProxyCommand {} ssh-proxy --gateway-name {} --name {} --workspace {}\n",
        runtime.config.values.name,
        runtime.config.values.workspace,
        quote(executable),
        quote(&runtime.config.values.gateway),
        quote(&runtime.config.values.name),
        quote(&runtime.config.values.workspace)
    ))
}

struct Terminal {
    group: i32,
    saved: libc::termios,
    restored: bool,
}

fn terminal_foreground(group: i32, saved: Option<&libc::termios>) -> Result<()> {
    unsafe {
        let mut block = std::mem::zeroed();
        libc::sigemptyset(&mut block);
        libc::sigaddset(&mut block, libc::SIGTTOU);
        let mut old = std::mem::zeroed();
        if libc::sigprocmask(libc::SIG_BLOCK, &block, &mut old) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let fg = libc::tcsetpgrp(0, group);
        let attrs = saved.map_or(0, |s| libc::tcsetattr(0, libc::TCSANOW, s));
        let mask = libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
        if fg != 0 || attrs != 0 || mask != 0 {
            return Err(Error::new("owned SSH terminal/mask restoration failed"));
        }
        Ok(())
    }
}

impl Terminal {
    fn save() -> Result<Self> {
        if !std::io::stdin().is_terminal()
            || !std::io::stdout().is_terminal()
            || !std::io::stderr().is_terminal()
        {
            return Err(Error::new("owned SSH requires a real foreground terminal"));
        }
        unsafe {
            let group = libc::tcgetpgrp(0);
            let mut saved = std::mem::zeroed();
            if group < 0 || group != libc::getpgrp() || libc::tcgetattr(0, &mut saved) != 0 {
                return Err(Error::new(
                    "owned SSH requires the host foreground process group",
                ));
            }
            Ok(Self {
                group,
                saved,
                restored: false,
            })
        }
    }

    fn restore(&mut self) -> Result<()> {
        terminal_foreground(self.group, Some(&self.saved))?;
        unsafe {
            let mut now: libc::termios = std::mem::zeroed();
            if libc::tcgetpgrp(0) != self.group
                || libc::tcgetattr(0, &mut now) != 0
                || now.c_iflag != self.saved.c_iflag
                || now.c_oflag != self.saved.c_oflag
                || now.c_cflag != self.saved.c_cflag
                || now.c_lflag != self.saved.c_lflag
                || now.c_cc != self.saved.c_cc
                || now.c_ispeed != self.saved.c_ispeed
                || now.c_ospeed != self.saved.c_ospeed
            {
                return Err(Error::new("owned SSH terminal restoration unconfirmed"));
            }
        }
        self.restored = true;
        Ok(())
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}

fn foreground_spawn(mut command: Command) -> Result<Transport> {
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut block = std::mem::zeroed();
            libc::sigemptyset(&mut block);
            libc::sigaddset(&mut block, libc::SIGTTOU);
            let mut old = std::mem::zeroed();
            if libc::sigprocmask(libc::SIG_BLOCK, &block, &mut old) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let fg = libc::tcsetpgrp(0, libc::getpgrp());
            let error = std::io::Error::last_os_error();
            if libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if fg != 0 {
                return Err(error);
            }
            Ok(())
        });
    }
    Ok(Transport(command.spawn()?))
}

fn primary(child: &Transport, failed: &mut bool) -> Result<bool> {
    unsafe {
        // Consume only state transitions, never the terminal exit/reap event.
        let mut transition: libc::siginfo_t = std::mem::zeroed();
        if libc::waitid(
            libc::P_PID,
            child.0.id(),
            &mut transition,
            libc::WSTOPPED | libc::WCONTINUED | libc::WNOHANG,
        ) != 0
        {
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD) {
                return Err(std::io::Error::last_os_error().into());
            }
        } else if transition.si_pid() != 0 {
            *failed = true;
        }
        let mut info: libc::siginfo_t = std::mem::zeroed();
        if libc::waitid(
            libc::P_PID,
            child.0.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        ) != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.si_pid() == 0 {
            return Ok(false);
        }
        *failed |= info.si_code != libc::CLD_EXITED || info.si_status() != 1;
        Ok(true)
    }
}

fn same(runtime: &Runtime<'_>, original: &serde_json::Value, deadline: Instant) -> Result<()> {
    let now = checked(runtime, deadline)?;
    if now["id"] != original["id"]
        || now["policy"] != original["policy"]
        || now["labels"] != original["labels"]
    {
        return Err(Error::new(
            "original owned SSH binding changed; cleanup refused",
        ));
    }
    Ok(())
}

pub(in crate::openshell) fn run(runtime: &Runtime<'_>, argv: &[String]) -> Result<i32> {
    if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|s| s.contains('\0')) {
        return Err(Error::new("an executable and valid arguments are required"));
    }
    let _lock = SIGNAL_LOCK
        .lock()
        .map_err(|_| Error::new("owned signal lock unavailable"))?;
    let saved_terminal = Terminal::save()?;
    let mut signals = Signals::install_for(&[libc::SIGHUP, libc::SIGINT, libc::SIGTERM])?;
    let mut terminal = saved_terminal;
    // Inner scope drops every private host group before terminal and handler restoration.
    let result = (|| {
        let original = checked(runtime, Instant::now() + LIMIT)?;
        let ssh =
            super::super::find_executable("ssh", runtime.env.get(std::ffi::OsStr::new("PATH")))
                .filter(|path| !path.starts_with(&runtime.config.values.project))
                .ok_or_else(|| Error::new("owned SSH requires host OpenSSH outside the project"))?;
        let expected = ssh_config(runtime)?;
        let emitted = capture(
            runtime.command(
                &[
                    "sandbox".into(),
                    "ssh-config".into(),
                    runtime.config.values.name.clone(),
                ],
                true,
            ),
            Instant::now() + LIMIT,
        )?;
        if emitted != expected.as_bytes() {
            return Err(Error::new(
                "OpenShell SSH configuration differs from the reviewed pinned template",
            ));
        }
        let private = tempfile::Builder::new()
            .prefix("dungeon-ssh-")
            .tempdir_in(runtime.config.path.parent().expect("config parent"))?;
        std::fs::set_permissions(private.path(), std::fs::Permissions::from_mode(0o700))?;
        let path = private.path().join("ssh-config");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(expected.as_bytes())?;
        file.write_all(b"    IdentityAgent none\n    IdentityFile none\n    IdentitiesOnly yes\n    ForwardAgent no\n    ForwardX11 no\n    ClearAllForwardings yes\n    BatchMode yes\n    PasswordAuthentication no\n    KbdInteractiveAuthentication no\n    PubkeyAuthentication no\n    ControlMaster no\n    ControlPath none\n    PermitLocalCommand no\n    EscapeChar none\n")?;
        let mut random = [0; 32];
        unsafe {
            if libc::getrandom(random.as_mut_ptr().cast(), random.len(), 0) != random.len() as isize
            {
                return Err(Error::new("owned invocation randomness unavailable"));
            }
        }
        let token = data_encoding::HEXLOWER.encode(&random);
        let pending = SIGNAL.load(Ordering::SeqCst);
        if pending != 0 {
            return Ok(Completion::Unlaunched(pending));
        }
        let mut remote = vec![
            "/usr/bin/node".to_owned(),
            "-e".into(),
            SHIM.into(),
            "--".into(),
            "tty-launch".into(),
            token.clone(),
        ];
        remote.extend_from_slice(argv);
        let remote = format!(
            "exec {}",
            remote
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut command = Command::new(ssh);
        command
            .args([
                "-F",
                path.to_str()
                    .ok_or_else(|| Error::new("SSH config path is not UTF-8"))?,
                "-tt",
                &format!(
                    "openshell-{}.{}",
                    runtime.config.values.name, runtime.config.values.workspace
                ),
                &remote,
            ])
            .current_dir(private.path())
            .env_clear()
            .envs(&runtime.env)
            .env_remove("SSH_AUTH_SOCK");
        runtime.output.mutating();
        let child = foreground_spawn(command)?;
        let startup = Instant::now() + LIMIT;
        let mut ready = false;
        let mut failure = false;
        let mut cleanup = None;
        loop {
            let mut terminal_event = libc::pollfd {
                fd: 0,
                events: 0,
                revents: 0,
            };
            let polled = unsafe { libc::poll(&mut terminal_event, 1, 0) };
            if polled < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                failure = true;
            }
            failure |=
                terminal_event.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0;
            let finished = primary(&child, &mut failure)?;
            let signal = SIGNAL.load(Ordering::SeqCst);
            if !ready && cleanup.is_none() && !finished && signal == 0 && !failure {
                same(runtime, &original, startup)?;
                if let Ok(bytes) =
                    capture(super::command(runtime, "tty-ready", &token, &[]), startup)
                {
                    let value = read_json_text(
                        std::str::from_utf8(&bytes)
                            .map_err(|_| Error::new("invalid SSH readiness"))?,
                    )?;
                    ready =
                        value.as_object().is_some_and(|v| v.len() == 1) && value["token"] == token;
                }
                if Instant::now() >= startup {
                    failure = true;
                }
            }
            if finished || signal != 0 || failure {
                let deadline = *cleanup.get_or_insert_with(|| Instant::now() + LIMIT);
                same(runtime, &original, deadline)?;
                // Startup absence is retried; never treated as stopped.
                let _ = capture(super::command(runtime, "stop", &token, &[]), deadline);
                if let Ok(bytes) =
                    capture(super::command(runtime, "tty-result", &token, &[]), deadline)
                {
                    let receipt = receipt(&bytes, &token)?;
                    let finished = primary(&child, &mut failure)?;
                    if finished || failure {
                        same(runtime, &original, deadline)?;
                        capture(super::command(runtime, "tty-remove", &token, &[]), deadline)?;
                        return Ok(Completion::Launched(receipt, failure));
                    }
                }
                if Instant::now() >= deadline {
                    return Err(Error::new(
                        "owned SSH cleanup unconfirmed; original guest work/control state may remain",
                    ));
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    let restored = terminal.restore();
    let result = restored.and_then(|()| final_result(result));
    let handlers = signals.restore();
    handlers?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::fd::{FromRawFd, OwnedFd};

    #[test]
    fn interactive_postlaunch_drift_keeps_code_but_not_no_effects() {
        let fixture = crate::openshell::tests::Fixture::new();
        fixture.runtime().create().unwrap();
        let runtime = fixture.runtime().machine();
        let original = checked(&runtime, Instant::now() + LIMIT).unwrap();
        fixture.flag("mismatch");
        let preflight = runtime
            .output
            .error(same(&runtime, &original, Instant::now() + LIMIT).unwrap_err());
        assert_eq!(preflight.effects, "none");
        // Classifier seam used after foreground_spawn, without claiming a new PTY run.
        runtime.output.mutating();
        let postlaunch = runtime
            .output
            .error(same(&runtime, &original, Instant::now() + LIMIT).unwrap_err());
        let event = crate::machine::error_json("interactive", &postlaunch, serde_json::Value::Null);
        assert_eq!(event["error"]["code"], "identity_mismatch");
        assert_eq!(event["error"]["effects"], "unknown");
    }

    fn node() -> std::path::PathBuf {
        let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let environment = super::super::super::host_environment(Some(project)).unwrap();
        super::super::super::find_executable("node", environment.get(std::ffi::OsStr::new("PATH")))
            .filter(|path| !path.starts_with(project))
            .expect("owned SSH tests require Node on a trusted sanitized host PATH")
    }

    fn encoded(status: serde_json::Value) -> Vec<u8> {
        json!({"token":"test", "clean":true, "stopped":false, "fault":false, "status":status})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn strict_receipts_and_sticky_failure() {
        let bytes = encoded(json!({"code":23,"signal":null,"spawn_error":false}));
        let r = receipt(&bytes, "test").unwrap();
        assert_eq!(returned(&r, false, 0).unwrap(), 23);
        assert!(returned(&r, true, 0).is_err());
        assert!(returned(&r, true, libc::SIGINT).is_err());
        for status in [
            json!({"code":256,"signal":null,"spawn_error":false}),
            json!({"code":-1,"signal":null,"spawn_error":false}),
            json!({"code":0,"signal":"SIGTERM","spawn_error":false}),
            json!({"code":null,"signal":"unknown","spawn_error":false}),
            json!({"code":null,"signal":null,"spawn_error":false}),
            json!({"code":0,"spawn_error":false}),
            json!({"code":0,"signal":null,"spawn_error":false,"pid":1}),
        ] {
            assert!(receipt(&encoded(status), "test").is_err());
        }
        for text in [
            r#"{"token":"test","token":"test","clean":true,"stopped":false,"fault":false,"status":null}"#,
            r#"{"token":"test","clean":true,"stopped":false,"fault":false}"#,
            r#"{"token":"test","clean":true,"stopped":false,"fault":false,"status":null,"pid":1}"#,
            r#"{"token":"test","clean":true,"stopped":false,"fault":false,"status":{"code":1,"code":1,"signal":null,"spawn_error":false}}"#,
        ] {
            assert!(receipt(text.as_bytes(), "test").is_err());
        }
        let mut r = receipt(&encoded(serde_json::Value::Null), "test").unwrap();
        assert!(returned(&r, false, 0).is_err());
        r.stopped = true;
        assert_eq!(returned(&r, false, libc::SIGTERM).unwrap(), 143);
        r.fault = true;
        assert!(returned(&r, false, libc::SIGTERM).is_err());
    }

    #[test]
    fn tail_signal_cannot_return_an_unacknowledged_normal_result() {
        let _lock = SIGNAL_LOCK.lock().unwrap();
        SIGNAL.store(0, Ordering::SeqCst);
        let r = receipt(
            &encoded(json!({"code":23,"signal":null,"spawn_error":false})),
            "test",
        )
        .unwrap();
        let completion = Ok(Completion::Launched(r, false));
        // Completion chosen, then transport teardown/terminal restoration receives INT.
        super::super::signal(libc::SIGINT);
        assert!(final_result(completion).is_err());
        let mut r = receipt(&encoded(serde_json::Value::Null), "test").unwrap();
        r.stopped = true;
        assert_eq!(
            final_result(Ok(Completion::Launched(r, false))).unwrap(),
            130
        );
        SIGNAL.store(0, Ordering::SeqCst);
    }

    #[test]
    fn late_guardian_fault_supersedes_a_pending_normal_acknowledgement() {
        let root = tempfile::tempdir().unwrap();
        let marker = serde_json::to_string(&root.path().join("ack-pending")).unwrap();
        let shim = SHIM.replace("process.chdir('/sandbox/project')", &format!("process.chdir({})", serde_json::to_string(root.path()).unwrap()))
            .replace("send({ finish_ack: value.revision });", &format!("fs.writeFileSync({marker}, 'pending'); setTimeout(() => send({{ finish_ack: value.revision }}), 400);"));
        for (index, fault_signal) in [libc::SIGHUP, libc::SIGTERM].into_iter().enumerate() {
            let token = format!(
                "{:064x}",
                u128::from(std::process::id()) * 1000 + index as u128
            );
            let _ = std::fs::remove_file(root.path().join("ack-pending"));
            let (mut child, _master) = guest_pty(
                &shim,
                &token,
                &[
                    node().to_str().unwrap().into(),
                    "-e".into(),
                    "process.exit(23)".into(),
                ],
            );
            let deadline = Instant::now() + Duration::from_secs(6);
            while !root.path().join("ack-pending").exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(root.path().join("ack-pending").exists());
            unsafe {
                libc::kill(child.id() as i32, fault_signal);
            }
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(child.try_wait().unwrap().is_some());
            let r = loop {
                if let Ok(bytes) = local_control(&shim, &token, "tty-result") {
                    break receipt(&bytes, &token).unwrap();
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(r.fault);
            assert!(returned(&r, false, 0).is_err());
            local_control(&shim, &token, "tty-remove").unwrap();
        }
    }

    #[test]
    fn quoting_keeps_arguments_data() {
        let args = ["", "a b", "a\nb", "'\";$(exit 99)", "--flag"];
        let output = Command::new("/bin/sh")
            .args([
                "-c",
                &format!(
                    "printf '%s\\0' {}",
                    args.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
                ),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            args.iter()
                .flat_map(|s| s.bytes().chain([0]))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn guest_tty_refuses_absent_terminal() {
        let mut command = Command::new(node());
        command.args([
            "-e",
            SHIM,
            "--",
            "tty-launch",
            &format!("{:064x}", std::process::id()),
            "/bin/true",
        ]);
        let output = command.stdin(Stdio::null()).output().unwrap();
        assert_eq!(output.status.code(), Some(125));
        assert_eq!(
            output.stderr,
            b"owned guest control unavailable or invalid\n"
        );
    }

    #[test]
    fn stopped_continued_transport_is_permanently_failed() {
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        let child = spawn(command, false).unwrap();
        unsafe {
            libc::kill(child.0.id() as i32, libc::SIGSTOP);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut failed = false;
        while !failed && Instant::now() < deadline {
            primary(&child, &mut failed).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(failed);
        unsafe {
            libc::kill(child.0.id() as i32, libc::SIGCONT);
            libc::kill(child.0.id() as i32, libc::SIGTERM);
        }
        while !exited(&child).unwrap() && Instant::now() < deadline {
            primary(&child, &mut failed).unwrap();
        }
        assert!(failed);
    }

    fn local_control(shim: &str, token: &str, mode: &str) -> Result<Vec<u8>> {
        let mut command = Command::new(node());
        command.args(["-e", shim, "--", mode, token]);
        capture(command, Instant::now() + Duration::from_secs(3))
    }

    fn guest_pty(shim: &str, token: &str, args: &[String]) -> (Child, std::fs::File) {
        unsafe {
            let mut master = -1;
            let mut slave = -1;
            assert_eq!(
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null()
                ),
                0
            );
            let master = std::fs::File::from_raw_fd(master);
            let slave = std::fs::File::from(OwnedFd::from_raw_fd(slave));
            let mut command = Command::new(node());
            command
                .args(["-e", shim, "--", "tty-launch", token])
                .args(args)
                .stdin(slave.try_clone().unwrap())
                .stdout(slave.try_clone().unwrap())
                .stderr(slave);
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
            (command.spawn().unwrap(), master)
        }
    }

    #[test]
    fn synthetic_guest_pty_status_stop_and_faults() {
        let root = tempfile::tempdir().unwrap();
        // Synthetic workspace only; runtime always uses the fixed guest path.
        let shim = SHIM.replace(
            "process.chdir('/sandbox/project')",
            &format!(
                "process.chdir({})",
                serde_json::to_string(root.path()).unwrap()
            ),
        );
        for (index, fault) in [None, Some("guardian"), Some("observer"), Some("ack")]
            .into_iter()
            .enumerate()
        {
            let token = format!(
                "{:064x}",
                u128::from(std::process::id()) * 100 + index as u128
            );
            let directory = format!("/tmp/dungeon-owned-{token}");
            let args = vec![
                node().to_str().unwrap().into(),
                "-e".into(),
                if fault.is_none() || fault == Some("ack") {
                    "require('child_process').spawn(process.execPath,['-e','process.on(\"SIGTERM\",()=>{});setInterval(()=>{},1000)'],{stdio:'inherit'});setTimeout(()=>process.exit(23),200)".into()
                } else {
                    "setInterval(()=>{},1000)".into()
                },
            ];
            let active_shim = if fault == Some("ack") {
                shim.replace(
                    "send({ finish_ack: value.revision });",
                    "/* synthetic withheld acknowledgement */",
                )
            } else {
                shim.clone()
            };
            let (mut child, _master) = guest_pty(&active_shim, &token, &args);
            let deadline = Instant::now() + Duration::from_secs(7);
            while local_control(&shim, &token, "tty-ready").is_err() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(100));
            let fixture_children = std::fs::read_to_string(format!(
                "/proc/{}/task/{}/children",
                child.id(),
                child.id()
            ))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|pid| {
                let pid: i32 = pid.parse().ok()?;
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let start = stat
                    .rsplit_once(") ")?
                    .1
                    .split_whitespace()
                    .nth(19)?
                    .to_owned();
                Some((pid, start))
            })
            .collect::<Vec<_>>();
            if let Some(fault @ ("guardian" | "observer")) = fault {
                std::thread::sleep(Duration::from_millis(100));
                let target = if fault == "guardian" {
                    child.id() as i32
                } else {
                    let children = std::fs::read_to_string(format!(
                        "/proc/{}/task/{}/children",
                        child.id(),
                        child.id()
                    ))
                    .unwrap();
                    children.split_whitespace().next().unwrap().parse().unwrap()
                };
                unsafe {
                    libc::kill(target, libc::SIGKILL);
                }
            }
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                child.try_wait().unwrap().is_some(),
                "guardian must exit boundedly"
            );
            let mut result = None;
            while Instant::now() < deadline {
                if let Ok(bytes) = local_control(&shim, &token, "tty-result") {
                    result = Some(receipt(&bytes, &token).unwrap());
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if fault.is_none() {
                let r = result.expect("normal cleanup must be confirmed");
                assert_eq!(returned(&r, false, 0).unwrap(), 23);
                local_control(&shim, &token, "tty-remove").unwrap();
            } else {
                assert!(
                    result
                        .as_ref()
                        .is_none_or(|r| returned(r, false, 0).is_err())
                );
                // Only directly observed test-created children with unchanged start identity.
                for (pid, start) in fixture_children {
                    if std::fs::read_to_string(format!("/proc/{pid}/stat"))
                        .ok()
                        .and_then(|s| {
                            s.rsplit_once(") ")
                                .map(|(_, f)| f.split_whitespace().nth(19).unwrap_or("").to_owned())
                        })
                        .as_deref()
                        == Some(&start)
                    {
                        unsafe {
                            libc::kill(pid, libc::SIGKILL);
                        }
                    }
                }
                for name in ["stop", "result"] {
                    let _ = std::fs::remove_file(format!("{directory}/{name}"));
                }
                let _ = std::fs::remove_dir(directory);
            }
        }
    }
}
