//! Foreground OpenCode attachment. The guest owns its service and password;
//! OpenShell remains the only sandbox transport and forward.
use super::{GUEST_PROJECT, Runtime, find_executable};
use crate::{Error, Result};
use data_encoding::BASE64;
use std::fs::File;
use std::io::{self, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const GUEST_PORT: u16 = 4096;
static INTERRUPTED: AtomicI32 = AtomicI32::new(0);

extern "C" fn interrupted(signal: libc::c_int) {
    INTERRUPTED.store(signal, Ordering::SeqCst);
}

struct Signals {
    int: libc::sigaction,
    term: libc::sigaction,
}

impl Signals {
    fn install() -> Result<Self> {
        INTERRUPTED.store(0, Ordering::SeqCst);
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = interrupted as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut int = std::mem::zeroed();
            let mut term = std::mem::zeroed();
            if libc::sigaction(libc::SIGINT, &action, &mut int) != 0 {
                return Err(io::Error::last_os_error().into());
            }
            if libc::sigaction(libc::SIGTERM, &action, &mut term) != 0 {
                libc::sigaction(libc::SIGINT, &int, std::ptr::null_mut());
                return Err(io::Error::last_os_error().into());
            }
            Ok(Self { int, term })
        }
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGINT, &self.int, std::ptr::null_mut());
            libc::sigaction(libc::SIGTERM, &self.term, std::ptr::null_mut());
        }
    }
}

struct Session {
    master: File,
    service: Child,
    forward: Option<Child>,
    port: Option<u16>,
    password: Option<String>,
    cleaned: bool,
}

fn pty() -> Result<(File, File)> {
    let mut master = -1;
    let mut slave = -1;
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) })
}

fn password_line(line: &[u8]) -> Option<String> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let candidate = line.strip_prefix(b"server password ")?;
    if candidate.len() < 16 || candidate.len() > 128 || !candidate.iter().all(u8::is_ascii_graphic)
    {
        return None;
    }
    String::from_utf8(candidate.to_vec()).ok()
}

fn read_password(session: &mut Session) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut output = Vec::new();
    while Instant::now() < deadline {
        if INTERRUPTED.load(Ordering::SeqCst) != 0 {
            return Err(Error::new("OpenCode service startup interrupted"));
        }
        if session.service.try_wait()?.is_some() {
            break;
        }
        let mut poll = libc::pollfd {
            fd: session.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 300) } < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error().into());
        }
        if poll.revents & libc::POLLIN == 0 {
            continue;
        }
        let mut chunk = [0u8; 1024];
        let count = match session.master.read(&mut chunk) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        output.extend_from_slice(&chunk[..count]);
        if output.len() > 8192 {
            break;
        }
        while let Some(end) = output.iter().position(|byte| *byte == b'\n') {
            let line: Vec<_> = output.drain(..=end).collect();
            if let Some(password) = password_line(&line[..line.len() - 1]) {
                return Ok(password);
            }
        }
    }
    Err(Error::new(
        "guest OpenCode did not produce a generated server password; no credentials or output were saved",
    ))
}

fn request(port: u16, password: Option<&str>) -> Option<Vec<u8>> {
    let address: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    let auth = password
        .map(|password| {
            format!(
                "Authorization: Basic {}\r\n",
                BASE64.encode(format!("opencode:{password}").as_bytes())
            )
        })
        .unwrap_or_default();
    stream
        .write_all(
            format!("GET /api/info HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n")
                .as_bytes(),
        )
        .ok()?;
    let mut response = Vec::new();
    match stream.take(8192).read_to_end(&mut response) {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) && !response.is_empty() => {}
        Err(_) => return None,
    }
    Some(response)
}

fn status(response: &[u8], code: u16) -> bool {
    response.starts_with(format!("HTTP/1.1 {code} ").as_bytes())
        || response.starts_with(format!("HTTP/1.0 {code} ").as_bytes())
}

fn local_port() -> Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

impl Session {
    fn start(runtime: &Runtime<'_>, port: u16) -> Result<Self> {
        let (master, slave) = pty()?;
        let mut command = runtime.command(
            &[
                "sandbox".into(),
                "exec".into(),
                "--name".into(),
                runtime.config.values.name.clone(),
                "--workdir".into(),
                GUEST_PROJECT.into(),
                "--tty".into(),
                "--".into(),
                "opencode".into(),
                "serve".into(),
                "--hostname".into(),
                "127.0.0.1".into(),
                "--port".into(),
                GUEST_PORT.to_string(),
            ],
            true,
        );
        command
            .process_group(0)
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        let service = command.spawn()?;
        Ok(Self {
            master,
            service,
            forward: None,
            port: Some(port),
            password: None,
            cleaned: false,
        })
    }

    fn connect(&mut self, runtime: &Runtime<'_>, expected: &str) -> Result<String> {
        let password = read_password(self)?;
        self.password = Some(password);
        let port = self.port.expect("selected before service start");
        let mut command = runtime.command(
            &[
                "forward".into(),
                "service".into(),
                runtime.config.values.name.clone(),
                "--target-host".into(),
                "127.0.0.1".into(),
                "--target-port".into(),
                GUEST_PORT.to_string(),
                "--local".into(),
                format!("127.0.0.1:{port}"),
            ],
            true,
        );
        self.forward = Some(
            command
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if INTERRUPTED.load(Ordering::SeqCst) != 0 {
                return Err(Error::new("OpenCode attachment interrupted"));
            }
            if self.service.try_wait()?.is_some() {
                return Err(Error::new(
                    "guest OpenCode service exited before attachment",
                ));
            }
            if let Some(status) = self.forward.as_mut().unwrap().try_wait()? {
                return Err(Error::new(format!(
                    "OpenShell loopback forward exited before attachment ({status})"
                )));
            }
            if let Some(response) = request(port, None) {
                if !status(&response, 401) {
                    return Err(Error::new(
                        "guest OpenCode service is not enforcing authentication",
                    ));
                }
                let response = request(port, self.password.as_deref()).ok_or_else(|| {
                    Error::new("guest OpenCode authentication could not be verified")
                })?;
                if !status(&response, 200)
                    || !response
                        .windows(expected.len())
                        .any(|bytes| bytes == expected.as_bytes())
                {
                    return Err(Error::new(
                        "authenticated guest OpenCode service/version mismatch",
                    ));
                }
                return Ok(format!("http://127.0.0.1:{port}"));
            }
            thread::sleep(Duration::from_millis(150));
        }
        Err(Error::new("guest OpenCode forward did not become ready"))
    }

    fn cleanup(&mut self) -> bool {
        if self.cleaned {
            return true;
        }
        let _ = self.master.write_all(b"\x03");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exited = false;
        while Instant::now() < deadline {
            if matches!(self.service.try_wait(), Ok(Some(_))) {
                exited = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        if !exited {
            let _ = self.service.kill();
            let _ = self.service.wait();
        }
        // A dead host exec alone is not proof: its guest service can survive.
        let mut gone = false;
        let forward_live = self
            .forward
            .as_mut()
            .is_some_and(|forward| matches!(forward.try_wait(), Ok(None)));
        if let (Some(port), Some(password), true) =
            (self.port, self.password.as_deref(), forward_live)
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if exited
                    && request(port, Some(password)).is_none_or(|response| !status(&response, 200))
                    && self
                        .forward
                        .as_mut()
                        .is_some_and(|forward| matches!(forward.try_wait(), Ok(None)))
                {
                    gone = true;
                    break;
                }
                thread::sleep(Duration::from_millis(150));
            }
        }
        if let Some(mut forward) = self.forward.take() {
            let _ = forward.kill();
            let _ = forward.wait();
        }
        self.cleaned = gone;
        gone
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.cleaned && !self.cleanup() {
            eprintln!(
                "Dungeon could not confirm guest service shutdown; the sandbox was preserved. Inspect it before reattaching."
            );
        }
    }
}

fn prompt_recovery(runtime: &Runtime<'_>, session: &mut Session) -> Result<()> {
    eprintln!(
        "Guest OpenCode may still be running. The forward is closed; sandbox data is preserved."
    );
    if !io::stdin().is_terminal() {
        return Err(Error::new(
            "guest service shutdown unconfirmed; run `dungeon down` to stop the sandbox (preserves guest state)",
        ));
    }
    loop {
        eprint!("[s]top sandbox (preserves state) or [l]eave running? ");
        io::stderr().flush()?;
        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;
        match choice.trim().to_ascii_lowercase().as_str() {
            "s" => {
                eprint!(
                    "Type `{}` to confirm stopping the entire sandbox: ",
                    runtime.config.values.name
                );
                io::stderr().flush()?;
                let mut name = String::new();
                io::stdin().read_line(&mut name)?;
                if name.trim() == runtime.config.values.name && runtime.down()? == 0 {
                    session.cleaned = true;
                    return Ok(());
                }
            }
            "l" | "" => break,
            _ => continue,
        }
    }
    Err(Error::new(
        "guest service shutdown unconfirmed; sandbox preserved. Run `dungeon down` to stop it",
    ))
}

pub(crate) fn attach_opencode(runtime: &Runtime<'_>) -> Result<i32> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(Error::new(
            "host OpenCode attachment needs a terminal; use `dungeon run opencode` for guest CLI execution",
        ));
    }
    runtime.work_ready()?;
    runtime.checked(true, true)?;
    let binary = find_executable("opencode", runtime.env.get(std::ffi::OsStr::new("PATH")))
        .ok_or_else(|| Error::new("install the host OpenCode client outside the project"))?;
    let host = Command::new(&binary).arg("--version").output()?;
    if !host.status.success() || host.stdout.len() > 128 {
        return Err(Error::new("host OpenCode version is unavailable"));
    }
    let host = std::str::from_utf8(&host.stdout)
        .unwrap_or("")
        .trim()
        .to_owned();
    if !version_valid(&host) {
        return Err(Error::new("host OpenCode version is unavailable"));
    }
    let guest = runtime.capture(
        &[
            "sandbox".into(),
            "exec".into(),
            "--name".into(),
            runtime.config.values.name.clone(),
            "--workdir".into(),
            GUEST_PROJECT.into(),
            "--no-tty".into(),
            "--".into(),
            "opencode".into(),
            "--version".into(),
        ],
        true,
    )?;
    if guest.trim() != host {
        return Err(Error::new(format!(
            "host {host} and guest {} differ; select a matching reviewed image/client before attachment",
            guest.trim()
        )));
    }
    let port = local_port()?;
    let _signals = Signals::install()?;
    let mut session = Session::start(runtime, port)?;
    let result = (|| {
        let url = session.connect(runtime, host.trim_start_matches("opencode v"))?;
        if INTERRUPTED.load(Ordering::SeqCst) != 0 {
            return Ok(128 + INTERRUPTED.load(Ordering::SeqCst));
        }
        eprintln!(
            "Connected to guest OpenCode at {url}. Leave the host UI to stop the service and forward; sandbox state is kept."
        );
        io::stdout().flush()?;
        let proceed = wait_for_ui(io::stdin().as_raw_fd(), &mut io::stderr().lock())?;
        if INTERRUPTED.load(Ordering::SeqCst) != 0 {
            return Ok(128 + INTERRUPTED.load(Ordering::SeqCst));
        }
        if !proceed {
            return Ok(0);
        }
        let mut child = Command::new(binary)
            .args(["--server", &url])
            .env("OPENCODE_PASSWORD", session.password.as_deref().unwrap())
            .spawn()?;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status.code().unwrap_or(128)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error.into());
                }
                Ok(None) => {}
            }
            let forward_ended = session
                .forward
                .as_mut()
                .is_some_and(|forward| !matches!(forward.try_wait(), Ok(None)));
            let service_ended = !matches!(session.service.try_wait(), Ok(None));
            if forward_ended || service_ended {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Error::new(
                    "OpenCode guest service or loopback forward ended while the host UI was open",
                ));
            }
            if INTERRUPTED.load(Ordering::SeqCst) != 0 {
                let _ = child.kill();
                let _ = child.wait();
                break Ok(128 + INTERRUPTED.load(Ordering::SeqCst));
            }
            thread::sleep(Duration::from_millis(100));
        }
    })();
    if !session.cleanup() {
        prompt_recovery(runtime, &mut session)?;
    }
    result
}

fn wait_for_ui(input: RawFd, output: &mut impl Write) -> io::Result<bool> {
    write!(output, "Press Enter to open OpenCode (Ctrl-C to cancel): ")?;
    output.flush()?;
    while INTERRUPTED.load(Ordering::SeqCst) == 0 {
        let mut poll = libc::pollfd {
            fd: input,
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 200) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if poll.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) == 0 {
            continue;
        }
        let mut byte = 0u8;
        let count = unsafe { libc::read(input, (&mut byte as *mut u8).cast(), 1) };
        if count == 0 {
            return Ok(false);
        }
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if byte == b'\n' {
            return Ok(true);
        }
    }
    Ok(false)
}

fn version_valid(value: &str) -> bool {
    let Some(text) = value.strip_prefix("opencode v") else {
        return false;
    };
    let parts: Vec<_> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ui_waits_for_enter_and_eof_cancels() {
        let (input, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        writer.write_all(b"\n").unwrap();
        let mut output = Vec::new();
        assert!(wait_for_ui(input.as_raw_fd(), &mut output).unwrap());
        assert!(String::from_utf8(output).unwrap().contains("Press Enter"));
        drop(writer);
        assert!(!wait_for_ui(input.as_raw_fd(), &mut Vec::new()).unwrap());
    }

    #[test]
    fn accepts_only_a_complete_generated_password_line() {
        assert_eq!(
            password_line(b"server password abcdefghijklmnop"),
            Some("abcdefghijklmnop".into())
        );
        assert!(password_line(b"server password short").is_none());
        assert!(password_line(b"untrusted password abcdefghijklmnop").is_none());
        assert!(password_line(b"server password abcdefghijklmnop\nmore").is_none());
    }

    #[test]
    fn version_match_requires_exact_numeric_release() {
        assert!(version_valid("opencode v2.1.2"));
        for value in [
            "opencode v2.1.2-beta",
            "opencode v2.1",
            "opencode v2.1.2.3",
            "other v2.1.2",
        ] {
            assert!(!version_valid(value));
        }
    }
}
