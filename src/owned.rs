use super::{GUEST_PROJECT, Runtime};
use crate::{Error, Result};
use serde::Deserialize;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

const SHIM: &str = include_str!("owned.cjs");
const LIMIT: Duration = Duration::from_secs(10);
#[path = "owned_ssh.rs"]
mod interactive;
pub(super) use interactive::run as ssh;
#[path = "stream.rs"]
mod streaming;
pub(super) use streaming::run as stream;
static SIGNAL: AtomicI32 = AtomicI32::new(0);
static SIGNAL_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
type PhaseHook = Option<Box<dyn FnMut(&str)>>;
#[cfg(test)]
thread_local! { static PHASE: std::cell::RefCell<PhaseHook> = std::cell::RefCell::new(None); }
#[cfg(test)]
fn phase(name: &str) {
    PHASE.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(name);
        }
    });
}

extern "C" fn signal(number: libc::c_int) {
    let _ = SIGNAL.compare_exchange(0, number, Ordering::SeqCst, Ordering::SeqCst);
}

struct Signals {
    old: Vec<(i32, libc::sigaction)>,
}

impl Signals {
    fn install() -> Result<Self> {
        Self::install_for(&[libc::SIGINT, libc::SIGTERM])
    }

    fn install_for(numbers: &[i32]) -> Result<Self> {
        SIGNAL.store(0, Ordering::SeqCst);
        // Handlers perform only a lock-free atomic write, never allocation or IO.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = signal as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            let mut old: Vec<_> = numbers
                .iter()
                .map(|number| (*number, std::mem::zeroed()))
                .collect();
            for index in 0..old.len() {
                if libc::sigaction(old[index].0, &action, &mut old[index].1) != 0 {
                    let error = std::io::Error::last_os_error();
                    let mut failed = false;
                    for (number, previous) in &old[..index] {
                        failed |= libc::sigaction(*number, previous, std::ptr::null_mut()) != 0;
                    }
                    return Err(if failed {
                        Error::new("owned partial signal installation restoration failed")
                    } else {
                        error.into()
                    });
                }
            }
            Ok(Self { old })
        }
    }

    fn restore(&mut self) -> Result<()> {
        let mut failed = false;
        for (number, previous) in &self.old {
            failed |= unsafe { libc::sigaction(*number, previous, std::ptr::null_mut()) } != 0;
        }
        if failed {
            Err(Error::new("owned signal restoration failed"))
        } else {
            self.old.clear();
            Ok(())
        }
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

// Keep each transport leader unreaped until its private group is terminated.
struct Transport(Child);
impl Drop for Transport {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn spawn(mut command: Command, capture: bool) -> Result<Transport> {
    command.stdin(Stdio::null()).process_group(0);
    if capture {
        command.stdout(Stdio::piped()).stderr(Stdio::null());
    }
    Ok(Transport(command.spawn()?))
}

fn exited(child: &Transport) -> Result<bool> {
    // WNOWAIT observes without reaping: PID/group identity remains reserved.
    unsafe {
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
        Ok(info.si_pid() != 0)
    }
}

fn primary_succeeded(child: &Transport, code: u8) -> Result<bool> {
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        if libc::waitid(
            libc::P_PID,
            child.0.id(),
            &mut info,
            libc::WEXITED | libc::WNOWAIT,
        ) != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(info.si_code == libc::CLD_EXITED && info.si_status() == i32::from(code))
    }
}

fn capture(command: Command, deadline: Instant) -> Result<Vec<u8>> {
    let mut child = spawn(command, true)?;
    let mut output = child.0.stdout.take().expect("captured stdout");
    // Nonblocking reads keep malicious structured/control output bounded.
    use std::os::fd::AsRawFd;
    unsafe {
        let fd = output.as_raw_fd();
        if libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        match output.read(&mut buffer) {
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }
        if bytes.len() > 1024 * 1024 {
            return Err(Error::new("owned control output exceeded limit"));
        }
        if exited(&child)? {
            loop {
                let mut buffer = [0; 4096];
                match output.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error.into()),
                }
                if bytes.len() > 1024 * 1024 {
                    break;
                }
            }
            if bytes.len() > 1024 * 1024 {
                return Err(Error::new("owned control output exceeded limit"));
            }
            unsafe {
                let mut info: libc::siginfo_t = std::mem::zeroed();
                if libc::waitid(
                    libc::P_PID,
                    child.0.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                ) != 0
                    || info.si_code != libc::CLD_EXITED
                    || info.si_status() != 0
                {
                    return Err(Error::new("owned control transport failed"));
                }
            }
            // Drop retains/reaps transport; caller requires valid structured data.
            return Ok(bytes);
        }
        if Instant::now() >= deadline {
            return Err(Error::new("owned control timed out"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn checked(runtime: &Runtime<'_>, deadline: Instant) -> Result<serde_json::Value> {
    runtime.bound_state()?;
    let bytes = capture(
        runtime.command(
            &[
                "sandbox".into(),
                "get".into(),
                runtime.config.values.name.clone(),
                "--output".into(),
                "json".into(),
            ],
            true,
        ),
        deadline,
    )?;
    let remote = crate::config::read_json_text(
        std::str::from_utf8(&bytes).map_err(|_| Error::new("invalid sandbox output"))?,
    )?;
    runtime.check_remote(remote, true, true)
}

fn command(runtime: &Runtime<'_>, mode: &str, token: &str, argv: &[String]) -> Command {
    command_at(runtime, mode, token, argv, GUEST_PROJECT)
}

fn command_at(
    runtime: &Runtime<'_>,
    mode: &str,
    token: &str,
    argv: &[String],
    cwd: &str,
) -> Command {
    let mut args = vec![
        "sandbox".into(),
        "exec".into(),
        "--name".into(),
        runtime.config.values.name.clone(),
        "--workdir".into(),
        cwd.into(),
        "--no-tty".into(),
        "--".into(),
        // Absolute reviewed system path; never resolve a project-owned node/shim.
        "/usr/bin/node".into(),
        "-e".into(),
        SHIM.into(),
        "--".into(),
        mode.into(),
        token.into(),
    ];
    args.extend_from_slice(argv);
    runtime.command(&args, true)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    token: String,
    code: u8,
    #[serde(default)]
    output_complete: Option<bool>,
}

pub(super) fn run(runtime: &Runtime<'_>, argv: &[String]) -> Result<i32> {
    let _lock = SIGNAL_LOCK
        .lock()
        .map_err(|_| Error::new("owned signal lock unavailable"))?;
    let _signals = Signals::install()?;
    run_with_output(runtime, argv, GUEST_PROJECT, None)
}

fn run_with_output(
    runtime: &Runtime<'_>,
    argv: &[String],
    cwd: &str,
    mut output: Option<&mut streaming::Stream>,
) -> Result<i32> {
    if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|arg| arg.contains('\0')) {
        return Err(Error::new("an executable and valid arguments are required"));
    }
    let original = checked(runtime, Instant::now() + LIMIT)?;
    #[cfg(test)]
    phase("prelaunch");
    let pending = SIGNAL.load(Ordering::SeqCst);
    if pending != 0 {
        return if output.is_some() {
            Err(Error::new("Machine execution cancelled before launch."))
        } else {
            Ok(128 + pending)
        };
    }
    if let Some(output) = output.as_mut() {
        output.prepare_started(&original)?;
    }
    let mut random = [0u8; 32];
    unsafe {
        if libc::getrandom(random.as_mut_ptr().cast(), random.len(), 0) != random.len() as isize {
            return Err(Error::new("owned invocation randomness unavailable"));
        }
    }
    let token = data_encoding::HEXLOWER.encode(&random);
    let mut launch = command_at(
        runtime,
        if output.is_some() {
            "machine-launch"
        } else {
            "launch"
        },
        &token,
        argv,
        cwd,
    );
    if output.is_some() {
        launch.stdout(Stdio::piped()).stderr(Stdio::null());
    }
    if SIGNAL.load(Ordering::SeqCst) != 0 {
        return if output.is_some() {
            Err(Error::new("Machine execution cancelled before launch."))
        } else {
            Ok(128 + SIGNAL.load(Ordering::SeqCst))
        };
    }
    if output.is_some() {
        runtime.output.mutating();
    }
    let mut child = spawn(launch, false)?;
    if let Some(output) = output.as_mut() {
        // Initialization failure is an output fault handled by the same STOP/receipt owner.
        if output.started(child.0.stdout.take()).is_err() {
            output.interrupt();
        }
    }
    let mut deadline = None;
    loop {
        let interrupt = SIGNAL.load(Ordering::SeqCst);
        let finished = exited(&child)?;
        if let Some(output) = output.as_mut() {
            output.poll(finished);
            if interrupt != 0 {
                output.interrupt();
            }
        }
        let output_failed = output.as_ref().is_some_and(|s| s.failed());
        if interrupt != 0 || finished || output_failed {
            let until = *deadline.get_or_insert_with(|| Instant::now() + LIMIT);
            let mut transport_failed = false;
            let result = (|| {
                let current = checked(runtime, until)?;
                if current["id"] != original["id"]
                    || current["policy"] != original["policy"]
                    || current["labels"] != original["labels"]
                {
                    return Err(Error::new("original owned sandbox binding changed"));
                }
                if (interrupt != 0 || output_failed) && !finished {
                    // Endpoint absence is retried, never interpreted as stopped.
                    let _ = capture(command(runtime, "stop", &token, &[]), until);
                }
                let bytes = match capture(command(runtime, "result", &token, &[]), until) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        // A dead primary transport may have left a silent guest command.
                        // Ask its live guardian to stop, even without a host signal.
                        if finished {
                            let _ = capture(command(runtime, "stop", &token, &[]), until);
                        }
                        return Err(error);
                    }
                };
                let receipt: Receipt = serde_json::from_value(crate::config::read_json_text(
                    std::str::from_utf8(&bytes).map_err(|_| Error::new("Invalid receipt."))?,
                )?)?;
                if receipt.token != token {
                    return Err(Error::new("owned receipt identity mismatch"));
                }
                if let Some(output) = output.as_mut()
                    && !output.failed()
                    && !output.complete()
                {
                    return Err(Error::new("Output still draining."));
                }
                if !output_failed && !exited(&child)? {
                    return Err(Error::new("owned guest wrapper has not exited"));
                }
                let current = checked(runtime, until)?;
                if current["id"] != original["id"]
                    || current["policy"] != original["policy"]
                    || current["labels"] != original["labels"]
                {
                    return Err(Error::new("original owned sandbox binding changed"));
                }
                capture(command(runtime, "remove", &token, &[]), until)?;
                let received = SIGNAL.load(Ordering::SeqCst);
                if output_failed {
                    transport_failed = true;
                    return Err(Error::new("Owned cleanup confirmed; output incomplete."));
                }
                // A pending host signal never overrides a failed primary transport.
                if !primary_succeeded(&child, receipt.code)? {
                    transport_failed = true;
                    return Err(Error::new(
                        "owned cleanup confirmed but primary transport failed",
                    ));
                }
                if output_failed
                    || output
                        .as_ref()
                        .is_some_and(|_| receipt.output_complete != Some(true))
                {
                    transport_failed = true;
                    return Err(Error::new("Machine output incomplete."));
                }
                Ok(if received == 0 {
                    i32::from(receipt.code)
                } else {
                    128 + received
                })
            })();
            if let Ok(code) = result {
                drop(child);
                #[cfg(test)]
                phase("teardown");
                let tail = SIGNAL.load(Ordering::SeqCst);
                if output.is_some() && tail != 0 {
                    return Err(Error::new("Machine execution cancelled during teardown."));
                }
                return Ok(if tail == 0 { code } else { 128 + tail });
            }
            if transport_failed {
                return Err(Error::new(
                    "owned cleanup confirmed but primary transport failed",
                ));
            }
            if Instant::now() >= until {
                return Err(Error::classified(
                    crate::ErrorCode::CleanupUnconfirmed,
                    "owned cleanup unconfirmed; original guest command/control state may remain; inspect the bound sandbox (guest requires /usr/bin/node and Linux /proc)",
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_capture_reaps_a_silent_transport() {
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        let before = Instant::now();
        assert!(capture(command, before + Duration::from_millis(80)).is_err());
        assert!(before.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn control_transport_failure_is_not_success() {
        assert!(
            capture(
                Command::new("/bin/false"),
                Instant::now() + Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn primary_transport_signal_is_not_a_matching_receipt_exit() {
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        let child = spawn(command, false).unwrap();
        unsafe {
            libc::kill(child.0.id() as i32, libc::SIGKILL);
        }
        // The same validation is used whether host INT/TERM is pending or not.
        assert!(!primary_succeeded(&child, 143).unwrap());
    }
}
