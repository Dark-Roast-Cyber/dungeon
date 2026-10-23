use super::*;
use crate::machine;
use data_encoding::BASE64;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::process::ChildStdout;

const QUEUE: usize = 65536;
const FRAME: usize = 8192;
const DRAIN: Duration = Duration::from_secs(2);

pub(super) struct Stream {
    sink: File,
    socket: bool,
    pub(super) cleanup: &'static str,
    pub(super) primary_verified: bool,
    input: Option<ChildStdout>,
    bytes: Vec<u8>,
    pending: Vec<u8>,
    seq: u64,
    source_seq: u64,
    eof: bool,
    fault: bool,
    drain_deadline: Option<Instant>,
    stalled_since: Option<Instant>,
    launched: bool,
    #[cfg(test)]
    fail_started: bool,
}
impl Stream {
    fn new() -> Result<Self> {
        let stdout = File::from(std::io::stdout().as_fd().try_clone_to_owned()?);
        Self::from_stdout(stdout)
    }
    fn from_stdout(stdout: File) -> Result<Self> {
        let kind = stdout.metadata()?.file_type();
        let socket = kind.is_socket();
        if socket {
            let mut domain = 0;
            let mut kind = 0;
            let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let mut peer: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
            let mut peer_length = std::mem::size_of_val(&peer) as libc::socklen_t;
            let valid = unsafe {
                libc::getsockopt(
                    stdout.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_DOMAIN,
                    (&mut domain as *mut libc::c_int).cast(),
                    &mut length,
                ) == 0
                    && libc::getsockopt(
                        stdout.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_TYPE,
                        (&mut kind as *mut libc::c_int).cast(),
                        &mut length,
                    ) == 0
                    && libc::getpeername(
                        stdout.as_raw_fd(),
                        (&mut peer as *mut libc::sockaddr_storage).cast(),
                        &mut peer_length,
                    ) == 0
            };
            if !valid || domain != libc::AF_UNIX || kind != libc::SOCK_STREAM {
                return Err(Error::refusal(
                    crate::ErrorCode::Unsupported,
                    "Machine exec requires a connected Unix stream socket or FIFO.",
                ));
            }
        } else if !kind.is_fifo() {
            return Err(Error::refusal(
                crate::ErrorCode::Unsupported,
                "Machine exec requires a Linux stdout FIFO or Unix stream socket.",
            ));
        }
        let sink = if socket {
            stdout
        } else {
            OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open(format!("/proc/self/fd/{}", stdout.as_raw_fd()))?
        };
        Ok(Self {
            sink,
            socket,
            cleanup: "not_started",
            primary_verified: false,
            input: None,
            bytes: vec![],
            pending: vec![],
            seq: 0,
            source_seq: 0,
            eof: false,
            fault: false,
            drain_deadline: None,
            stalled_since: None,
            launched: false,
            #[cfg(test)]
            fail_started: false,
        })
    }
    fn enqueue(&mut self, mut value: Value) -> Result<()> {
        value["schema"] = json!(1);
        value["operation"] = json!("exec");
        value["seq"] = json!(self.seq);
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        if self.pending.len() + bytes.len() > QUEUE {
            return Err(Error::new("Output queue exceeded."));
        }
        self.seq += 1;
        self.pending.extend(bytes);
        Ok(())
    }
    pub(super) fn prepare_started(&mut self, identity: &Value) -> Result<()> {
        self.enqueue(json!({"event":"started","sandbox_id":identity["id"]}))
    }
    pub(super) fn started(&mut self, input: Option<ChildStdout>) -> Result<()> {
        self.launched = true;
        if self.cleanup != "not_owned" {
            self.cleanup = "unconfirmed";
        }
        #[cfg(test)]
        if self.fail_started {
            return Err(Error::new("Injected descriptor initialization failure."));
        }
        let input = input.ok_or_else(|| Error::new("Missing primary output descriptor."))?;
        if unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        self.input = Some(input);
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        let write = if self.socket {
            let n = unsafe {
                libc::send(
                    self.sink.as_raw_fd(),
                    self.pending.as_ptr().cast(),
                    self.pending.len(),
                    libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
                )
            };
            if n < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        } else {
            self.sink.write(&self.pending)
        };
        match write {
            Ok(n) => {
                self.pending.drain(..n);
                if n > 0 {
                    self.stalled_since = None;
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let since = *self.stalled_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= DRAIN {
                    Err(Error::new("Output sink stalled."))
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(e.into()),
        }
    }
    fn collect(&mut self) -> Result<()> {
        self.flush()?;
        if self.eof || self.pending.len() > QUEUE - 2 * FRAME {
            return Ok(());
        }
        let mut buffer = [0u8; 4096];
        match self
            .input
            .as_mut()
            .expect("started stream")
            .read(&mut buffer)
        {
            Ok(0) => self.eof = true,
            Ok(n) => self.bytes.extend_from_slice(&buffer[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        while let Some(end) = self.bytes.iter().position(|b| *b == b'\n') {
            if end > FRAME {
                return Err(Error::new("Oversized frame."));
            }
            let line = self.bytes.drain(..=end).collect::<Vec<_>>();
            let value = parse_frame(&line[..end], self.source_seq)?;
            self.source_seq += 1;
            self.enqueue(json!({"event":"output","stream":value["stream"],"encoding":"base64","data":value["data"]}))?;
        }
        if self.bytes.len() > FRAME || (self.eof && !self.bytes.is_empty()) {
            return Err(Error::new("Truncated or oversized frame."));
        }
        Ok(())
    }
    pub(super) fn poll(&mut self, finished: bool) {
        if self.fault {
            let _ = self.flush();
            self.bytes.clear();
            if let Some(input) = self.input.as_mut() {
                for _ in 0..16 {
                    let mut buffer = [0u8; 4096];
                    match input.read(&mut buffer) {
                        Ok(0) => {
                            self.eof = true;
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            return;
        }
        // Bound work per cancellation tick without throttling every frame to one tick.
        for _ in 0..16 {
            if self.collect().is_err() {
                self.fault = true;
                return;
            }
        }
        if finished && !self.complete() {
            let deadline = *self
                .drain_deadline
                .get_or_insert_with(|| Instant::now() + DRAIN);
            if Instant::now() >= deadline {
                self.fault = true;
            }
        }
    }
    pub(super) fn failed(&self) -> bool {
        self.fault
    }
    pub(super) fn interrupt(&mut self) {
        self.fault = true;
    }
    pub(super) fn complete(&self) -> bool {
        self.eof && self.bytes.is_empty() && self.pending.is_empty()
    }
    fn finish(&mut self, result: &Result<i32>) -> Result<()> {
        if !self.launched {
            self.pending.clear();
            self.seq = 0;
        }
        let cancelled = SIGNAL.load(Ordering::SeqCst) != 0;
        let cancellation = Err(Error::new("Machine execution cancelled."));
        let mut event = match if cancelled { &cancellation } else { result } {
            Ok(code) => json!({"event":"finished","ok":true,"exit_code":code,"error":null}),
            Err(error) => {
                let mut v = machine::error_json("exec", error, Value::Null);
                v["event"] = json!("finished");
                v["exit_code"] = Value::Null;
                v
            }
        };
        event["cleanup"] = json!(self.cleanup);
        event["primary_transport_verified"] = json!(self.primary_verified);
        self.enqueue(event)?;
        let mut written = false;
        let deadline = Instant::now() + DRAIN;
        while !self.pending.is_empty() {
            #[cfg(test)]
            phase("final_delivery");
            if SIGNAL.load(Ordering::SeqCst) != 0 && !cancelled {
                // Once any final bytes are written they cannot be safely rewritten.
                // A late cancellation instead invalidates completion via CLI exit 1.
                if written {
                    return Err(Error::new("Machine execution cancelled during delivery."));
                }
                self.pending.clear();
                let mut event = machine::error_json(
                    "exec",
                    &Error::new("Machine execution cancelled."),
                    Value::Null,
                );
                event["event"] = json!("finished");
                event["exit_code"] = Value::Null;
                event["cleanup"] = json!(self.cleanup);
                event["primary_transport_verified"] = json!(self.primary_verified);
                // Replace, do not publish a second sequence number.
                self.seq -= 1;
                self.enqueue(event)?;
            }
            let before = self.pending.len();
            self.flush()?;
            written |= self.pending.len() < before;
            if Instant::now() >= deadline {
                return Err(Error::new("Output sink stalled."));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}

fn parse_frame(bytes: &[u8], seq: u64) -> Result<Value> {
    let fail = || Error::new("Invalid workload frame.");
    let text = std::str::from_utf8(bytes).map_err(|_| fail())?;
    let v = crate::config::read_json_text(text)?;
    let object = v.as_object().ok_or_else(fail)?;
    if object.len() != 5
        || !["v", "seq", "event", "stream", "data"]
            .iter()
            .all(|key| object.contains_key(*key))
        || v["v"].as_u64() != Some(1)
        || v["seq"].as_u64() != Some(seq)
        || v["event"] != "output"
        || !matches!(v["stream"].as_str(), Some("stdout" | "stderr"))
    {
        return Err(fail());
    }
    let data = v["data"].as_str().ok_or_else(fail)?;
    let decoded = BASE64.decode(data.as_bytes()).map_err(|_| fail())?;
    if decoded.len() > 4096 || BASE64.encode(&decoded) != data {
        return Err(fail());
    }
    Ok(v)
}

pub(in crate::openshell) fn run(
    runtime: &Runtime<'_>,
    argv: &[String],
    cwd: &str,
    owned: bool,
    stdin: Option<Vec<u8>>,
) -> Result<i32> {
    machine::validate_exec(argv, cwd)?;
    let mut output = Stream::new()?;
    if !owned {
        output.cleanup = "not_owned";
    }
    let _lock = SIGNAL_LOCK
        .lock()
        .map_err(|_| Error::new("Signal owner unavailable."))?;
    let _signals = Signals::install()?;
    let result = runtime
        .work_ready()
        .and_then(|()| {
            if owned {
                run_with_output(runtime, argv, cwd, Some(&mut output))
            } else {
                unowned(runtime, argv, cwd, stdin, &mut output)
            }
        })
        .map_err(|error| runtime.output.error(error));
    let delivered = output.finish(&result);
    Ok(
        if delivered.is_err() || SIGNAL.load(Ordering::SeqCst) != 0 {
            1
        } else {
            result.unwrap_or(1)
        },
    )
}

fn unowned(
    runtime: &Runtime<'_>,
    argv: &[String],
    cwd: &str,
    stdin: Option<Vec<u8>>,
    output: &mut Stream,
) -> Result<i32> {
    let original = checked(runtime, Instant::now() + LIMIT)?;
    if SIGNAL.load(Ordering::SeqCst) != 0 {
        return Err(Error::new("Machine execution cancelled before launch."));
    }
    output.prepare_started(&original)?;
    let mut random = [0; 32];
    if unsafe { libc::getrandom(random.as_mut_ptr().cast(), random.len(), 0) }
        != random.len() as isize
    {
        return Err(Error::new("Randomness unavailable."));
    }
    let token = data_encoding::HEXLOWER.encode(&random);
    let mut launch = command_at(runtime, "stream-launch", &token, argv, cwd);
    launch
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(bytes) = stdin {
        let mut input = tempfile::tempfile()?;
        input.write_all(&bytes)?;
        use std::io::{Seek, SeekFrom};
        input.seek(SeekFrom::Start(0))?;
        launch.stdin(input);
    } else {
        launch.stdin(Stdio::null());
    }
    runtime.output.mutating();
    let mut child = Transport(launch.spawn()?);
    output.started(child.0.stdout.take())?;
    loop {
        let finished = exited(&child)?;
        output.poll(finished);
        if SIGNAL.load(Ordering::SeqCst) != 0 || output.failed() {
            return Err(Error::new(
                "Unowned transport interrupted; guest work may remain.",
            ));
        }
        if finished && output.complete() {
            checked(runtime, Instant::now() + LIMIT)?;
            let bytes = capture(
                command(runtime, "result", &token, &[]),
                Instant::now() + LIMIT,
            )?;
            let receipt: Receipt = serde_json::from_value(crate::config::read_json_text(
                std::str::from_utf8(&bytes).map_err(|_| Error::new("Invalid receipt."))?,
            )?)?;
            if receipt.token != token
                || receipt.output_complete != Some(true)
                || !primary_succeeded(&child, receipt.code)?
            {
                return Err(Error::new("Unowned output or transport incomplete."));
            }
            capture(
                command(runtime, "remove", &token, &[]),
                Instant::now() + LIMIT,
            )?;
            output.primary_verified = true;
            drop(child);
            if SIGNAL.load(Ordering::SeqCst) != 0 {
                return Err(Error::new("Unowned transport interrupted."));
            }
            return Ok(i32::from(receipt.code));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::FromRawFd;

    fn pipe_stream() -> (Stream, File) {
        let mut fds = [0; 2];
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) },
            0
        );
        let sink = unsafe { File::from_raw_fd(fds[1]) };
        let reader = unsafe { File::from_raw_fd(fds[0]) };
        (
            Stream {
                sink,
                socket: false,
                cleanup: "not_started",
                primary_verified: false,
                input: None,
                bytes: vec![],
                pending: vec![],
                seq: 0,
                source_seq: 0,
                eof: false,
                fault: false,
                drain_deadline: None,
                stalled_since: None,
                launched: false,
                fail_started: false,
            },
            reader,
        )
    }

    #[test]
    fn socket_sink_preserves_flags_and_bounds_stalls_and_disconnects() {
        use std::os::fd::OwnedFd;
        use std::os::unix::net::{UnixDatagram, UnixStream};
        let (reader, writer) = UnixStream::pair().unwrap();
        let original = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
        let observer = writer.try_clone().unwrap();
        let mut stream = Stream::from_stdout(File::from(OwnedFd::from(writer))).unwrap();
        assert_eq!(
            unsafe { libc::fcntl(observer.as_raw_fd(), libc::F_GETFL) },
            original
        );
        loop {
            stream.pending = vec![1; QUEUE];
            stream.flush().unwrap();
            if stream.pending.len() == QUEUE {
                break;
            }
        }
        stream.stalled_since = Some(Instant::now() - DRAIN);
        assert!(stream.flush().is_err());
        assert_eq!(
            unsafe { libc::fcntl(observer.as_raw_fd(), libc::F_GETFL) },
            original
        );
        drop(reader);
        assert!(stream.flush().is_err());
        let (datagram, _peer) = UnixDatagram::pair().unwrap();
        assert!(Stream::from_stdout(File::from(OwnedFd::from(datagram))).is_err());
        assert!(Stream::from_stdout(tempfile::tempfile().unwrap()).is_err());
    }

    #[test]
    fn cancellation_before_launch_and_during_final_drain_never_succeeds() {
        let _lock = SIGNAL_LOCK.lock().unwrap();
        let _signals = Signals::install().unwrap();
        let fixture = crate::openshell::tests::Fixture::new();
        fixture.runtime().create().unwrap();
        let (mut stream, mut reader) = pipe_stream();
        PHASE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|name| {
                if name == "prelaunch" {
                    assert_eq!(unsafe { libc::raise(libc::SIGINT) }, 0);
                }
            }))
        });
        let result = run_with_output(
            &fixture.runtime(),
            &["true".into()],
            GUEST_PROJECT,
            Some(&mut stream),
        );
        PHASE.with(|hook| *hook.borrow_mut() = None);
        assert!(result.is_err());
        assert_eq!(fixture.calls("exec"), 0);
        stream.finish(&result).unwrap();
        let mut bytes = vec![];
        let mut b = [0; 4096];
        let n = reader.read(&mut b).unwrap();
        bytes.extend_from_slice(&b[..n]);
        let event: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(event["event"], "finished");
        assert_eq!(event["ok"], false);
        assert_eq!(event["seq"], 0);

        SIGNAL.store(0, Ordering::SeqCst);
        let (mut stream, mut reader) = pipe_stream();
        stream.launched = true;
        while stream.sink.write(&[0; 4096]).is_ok() {}
        let mut calls = 0;
        PHASE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |name| {
                if name == "final_delivery" {
                    calls += 1;
                    if calls == 2 {
                        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
                    }
                }
            }))
        });
        assert!(stream.finish(&Ok(23)).is_err());
        PHASE.with(|hook| *hook.borrow_mut() = None);
        assert_ne!(SIGNAL.load(Ordering::SeqCst), 0);
        // The sink was already full: no successful completion was written.
        let mut bytes = vec![];
        loop {
            match reader.read(&mut b) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&b[..n]),
                Err(_) => break,
            }
        }
        assert!(bytes.iter().all(|b| *b == 0));
    }

    #[test]
    fn oversized_identity_refuses_before_launch_and_postspawn_fault_uses_cleanup_owner() {
        let (mut stream, _reader) = pipe_stream();
        assert!(
            stream
                .prepare_started(&json!({"id":"x".repeat(QUEUE)}))
                .is_err()
        );
        assert!(!stream.launched && stream.pending.is_empty());

        let _lock = SIGNAL_LOCK.lock().unwrap();
        let _signals = Signals::install().unwrap();
        let fixture = crate::openshell::tests::Fixture::new();
        let node =
            crate::openshell::find_executable("node", std::env::var_os("PATH").as_ref()).unwrap();
        std::os::unix::fs::symlink(node, fixture.root.path().join("host-node")).unwrap();
        fixture.runtime().create().unwrap();
        let (mut stream, _reader) = pipe_stream();
        stream.fail_started = true;
        let mut sentinel = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let result = run_with_output(
            &fixture.runtime(),
            &[
                "/usr/bin/node".into(),
                "-e".into(),
                "setInterval(()=>{},1000)".into(),
            ],
            GUEST_PROJECT,
            Some(&mut stream),
        );
        assert!(result.is_err());
        assert!(fixture.calls("exec") >= 4);
        let calls = std::fs::read_to_string(fixture.root.path().join("calls")).unwrap();
        // The token is argv following machine-launch; only invocation-private controls authorize cleanup.
        let line = calls
            .lines()
            .find(|line| line.contains("-- machine-launch "))
            .unwrap();
        let token = line
            .split("-- machine-launch ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert!(!std::path::Path::new(&format!("/tmp/dungeon-owned-{token}")).exists());
        assert!(sentinel.try_wait().unwrap().is_none());
        sentinel.kill().unwrap();
        sentinel.wait().unwrap();
        assert!(
            !std::fs::read_dir("/proc")
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| std::fs::read(e.path().join("cmdline"))
                    .is_ok_and(|bytes| bytes.windows(token.len()).any(|b| b == token.as_bytes())))
        );
    }

    #[test]
    fn teardown_signal_overrides_a_verified_successful_receipt() {
        let _lock = SIGNAL_LOCK.lock().unwrap();
        let _signals = Signals::install().unwrap();
        let fixture = crate::openshell::tests::Fixture::new();
        let node =
            crate::openshell::find_executable("node", std::env::var_os("PATH").as_ref()).unwrap();
        std::os::unix::fs::symlink(node, fixture.root.path().join("host-node")).unwrap();
        fixture.runtime().create().unwrap();
        let (mut stream, _reader) = pipe_stream();
        PHASE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|name| {
                if name == "teardown" {
                    assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
                }
            }))
        });
        let result = run_with_output(
            &fixture.runtime(),
            &[
                "/usr/bin/node".into(),
                "-e".into(),
                "process.exit(23)".into(),
            ],
            GUEST_PROJECT,
            Some(&mut stream),
        );
        PHASE.with(|hook| *hook.borrow_mut() = None);
        assert!(result.is_err());
        assert!(stream.complete());
        stream.finish(&result).unwrap();
    }
    #[test]
    fn strict_frames_reject_authority_and_ambiguous_bytes() {
        assert!(
            parse_frame(
                br#"{"v":1,"seq":0,"event":"output","stream":"stderr","data":"AP8="}"#,
                0
            )
            .is_ok()
        );
        for text in [
            r#"{"v":1,"seq":0,"event":"output","stream":"stderr","data":"AP8=","code":0}"#,
            r#"{"v":1,"seq":0,"seq":0,"event":"output","stream":"stderr","data":"AP8="}"#,
            r#"{"v":1,"seq":0,"event":"output","stream":"stderr","data":"AP9="}"#,
        ] {
            assert!(parse_frame(text.as_bytes(), 0).is_err());
        }
        let oversized = json!({"v":1,"seq":0,"event":"output","stream":"stdout","data":BASE64.encode(&vec![0;4097])});
        assert!(parse_frame(oversized.to_string().as_bytes(), 0).is_err());
        assert!(
            parse_frame(
                br#"{"v":1,"seq":0,"event":"output","stream":"stdout","data":""#,
                0
            )
            .is_err()
        );
    }
}
