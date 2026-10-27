//! Local 0.0.116 gateway bootstrap. OpenShell owns registrations; Podman owns containers.
use super::{find_executable, host_environment};
use crate::{Error, OPEN_SHELL_VERSION, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[path = "uninstall.rs"]
mod removal;
pub(crate) use removal::binary_only as uninstall_binary;
pub(crate) use removal::run as uninstall;

pub(super) const GATEWAY: &str = "dungeon-local";
pub(super) const CONTAINER: &str = "openshell-gateway-dungeon-local";
const ENDPOINT: &str = "https://127.0.0.1:17670";
pub(super) const IMAGE: &str = "ghcr.io/nvidia/openshell/gateway@sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645";
pub(super) const AGENT_IMAGE: &str = "localhost/dungeon-agent:0.0.3-dev";
pub(super) const LABEL: &str = "agentarchaeology.ai/dungeon-gateway";
pub(super) const CONFIG: &str = "[openshell]\nversion = 1\n\n[openshell.drivers.podman]\nnetwork_name = \"dungeon-local\"\ngrpc_endpoint = \"https://openshell-gateway-dungeon-local:8080\"\nimage_pull_policy = \"missing\"\nenable_bind_mounts = false\n";
pub(super) const LIVE_CONFIG: &str = "[openshell]\nversion = 1\n\n[openshell.drivers.podman]\nnetwork_name = \"dungeon-local\"\ngrpc_endpoint = \"https://openshell-gateway-dungeon-local:8080\"\nimage_pull_policy = \"missing\"\nenable_bind_mounts = true\nuserns = \"keep-id:uid=998,gid=998\"\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BindConfig {
    pub(super) enabled: bool,
    pub(super) mapping: bool,
}

pub(super) fn parse_bind_config(text: &str) -> Option<BindConfig> {
    for enabled in [false, true] {
        let base = CONFIG.replace(
            "enable_bind_mounts = false",
            if enabled {
                "enable_bind_mounts = true"
            } else {
                "enable_bind_mounts = false"
            },
        );
        for mapping in [false, true] {
            let expected = if mapping {
                format!("{base}userns = \"keep-id:uid=998,gid=998\"\n")
            } else {
                base.clone()
            };
            if text == expected {
                return Some(BindConfig { enabled, mapping });
            }
        }
    }
    None
}

pub(crate) struct Reply {
    pub(crate) code: i32,
    pub(crate) text: String,
}

// One bounded native subprocess mechanism for setup and read-only diagnostics.
pub(crate) fn query(
    binary: &Path,
    env: &HashMap<OsString, OsString>,
    args: &[&str],
    timeout: Duration,
) -> Option<Reply> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir("/")
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let start = Instant::now();
    let child = loop {
        match command.spawn() {
            Ok(child) => break child,
            Err(error)
                if error.raw_os_error() == Some(libc::ETXTBSY) && start.elapsed() < timeout =>
            {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    };
    collect_query(child, timeout.saturating_sub(start.elapsed()))
}

pub(super) fn collect_query(mut child: std::process::Child, timeout: Duration) -> Option<Reply> {
    let mut pipe = child.stdout.take()?;
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    let nonblocking =
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0;
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut exited = false;
    let mut eof = false;
    let mut failed = !nonblocking;
    while !failed && !(eof && exited) {
        let mut buffer = [0u8; 8192];
        match pipe.read(&mut buffer) {
            Ok(0) => eof = true,
            Ok(n) => {
                bytes.extend_from_slice(&buffer[..n]);
                failed = bytes.len() > 262_144;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => failed = true,
        }
        if !exited {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                exited = unsafe { info.si_pid() } != 0;
            } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                failed = true;
            }
        }
        if start.elapsed() >= timeout {
            failed = true;
        }
        if !failed && !(eof && exited) {
            thread::sleep(Duration::from_millis(5));
        }
    }
    // Reserve the leader through teardown, even when descendants retain stdout.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let status = child.wait().ok()?;
    if failed {
        return None;
    }
    Some(Reply {
        code: status.code()?,
        text: String::from_utf8(bytes).ok()?,
    })
}

struct Tools {
    openshell: PathBuf,
    podman: PathBuf,
    podman_version: String,
    env: HashMap<OsString, OsString>,
    mappings: Value,
}

impl Tools {
    fn output(&self, binary: &Path, args: &[&str], context: &str) -> Result<Reply> {
        query(binary, &self.env, args, Duration::from_secs(5)).ok_or_else(|| {
            Error::new(format!(
                "{context}: bounded query unavailable; inspect prerequisites explicitly"
            ))
        })
    }

    fn checked(&self, binary: &Path, args: &[&str], context: &str) -> Result<String> {
        let output = self.output(binary, args, context)?;
        if output.code != 0 {
            return Err(Error::new(format!(
                "{context}: native command failed; inspect explicitly"
            )));
        }
        Ok(output.text)
    }

    fn podman(&self, args: &[&str], context: &str) -> Result<String> {
        self.checked(&self.podman, args, context)
    }

    fn openshell(&self, args: &[&str], context: &str) -> Result<String> {
        self.checked(&self.openshell, args, context)
    }

    fn mutate(&self, binary: &Path, args: &[&str], context: &str) -> Result<()> {
        let reply = query(binary, &self.env, args, Duration::from_secs(120));
        if !reply.is_some_and(|r| r.code == 0) {
            return Err(Error::new(format!(
                "{context}: failed or interrupted; partial setup may remain. Inspect Dungeon/OpenShell/Podman state explicitly before retrying; no automatic rollback"
            )));
        }
        Ok(())
    }
}

fn native_json(text: &str, context: &'static str) -> Result<Value> {
    crate::config::read_json_text(text).map_err(|_| Error::new(context))
}

fn require_linux(os: &str) -> Result<()> {
    if os != "linux" {
        return Err(Error::new(
            "dungeon setup supports Linux only (rootless Podman); no MicroVM fallback",
        ));
    }
    Ok(())
}

fn normalized_path(path: &Path) -> bool {
    use std::path::Component;
    path.is_absolute()
        && path.to_str().is_some_and(|s| {
            !s.contains(':')
                && !s.bytes().any(|b| b.is_ascii_control())
                && !s.contains("/./")
                && !s.ends_with("/.")
        })
        && !path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
}

fn directory(path: &Path, create: bool, private: bool) -> Result<fs::File> {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Component;
    if !normalized_path(path) {
        return Err(Error::new("setup paths must be absolute and normalized"));
    }
    let mut dir = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open("/")?;
    for component in path.components().filter_map(|c| match c {
        Component::Normal(n) => Some(n),
        _ => None,
    }) {
        let name = std::ffi::CString::new(component.as_bytes())
            .map_err(|_| Error::new("invalid setup path"))?;
        let flags = libc::O_RDONLY
            | libc::O_DIRECTORY
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | libc::O_CLOEXEC;
        let mut fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0
            && create
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        {
            if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
        }
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        dir = unsafe { fs::File::from_raw_fd(fd) };
        let m = dir.metadata()?;
        if (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() })
            || (m.mode() & 0o022 != 0 && !(m.uid() == 0 && m.mode() & libc::S_ISVTX != 0))
        {
            return Err(Error::new("unsafe setup path ownership or permissions"));
        }
    }
    if private {
        let m = dir.metadata()?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(Error::new(
                "setup state must be a private user-owned directory",
            ));
        }
    }
    Ok(dir)
}

fn safe_path(path: &Path, excluded: &[&Path]) -> bool {
    if !normalized_path(path) || excluded.iter().any(|p| path.starts_with(p)) {
        return false;
    }
    let Some(ancestor) = path.ancestors().find(|p| fs::symlink_metadata(p).is_ok()) else {
        return false;
    };
    directory(ancestor, false, false).is_ok()
}

pub(super) fn storage_ready(env: &HashMap<OsString, OsString>, excluded: &[&Path]) -> bool {
    let Some(home) = env.get(&OsString::from("HOME")) else {
        return false;
    };
    let data = env
        .get(&OsString::from("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".local/share"));
    let config = env
        .get(&OsString::from("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"));
    let store = data.join("containers/storage");
    safe_path(&store, excluded)
        && safe_path(&config, excluded)
        && fs::symlink_metadata(config.join("containers/storage.conf"))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        && directory(&store, false, false).is_ok()
        && fs::symlink_metadata(store.join("db.sql"))
            .is_ok_and(|m| m.is_file() && m.uid() == unsafe { libc::geteuid() } && m.len() > 0)
}

pub(super) fn socket_ready(socket: &Path) -> bool {
    socket
        .parent()
        .is_some_and(|p| directory(p, false, false).is_ok())
        && fs::symlink_metadata(socket)
            .is_ok_and(|m| m.file_type().is_socket() && m.uid() == unsafe { libc::geteuid() })
}

fn private_file(
    directory: &fs::File,
    name: &str,
    limit: u64,
    public_certificate: bool,
) -> Result<fs::File> {
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new(name).map_err(|_| Error::new("invalid private filename"))?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { fs::File::from_raw_fd(fd) };
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || !(m.mode() & 0o7777 == 0o600 || (public_certificate && m.mode() & 0o7777 == 0o644))
        || m.nlink() != 1
        || m.len() > limit
    {
        return Err(Error::new(
            "setup input must be a bounded private user-owned regular file",
        ));
    }
    Ok(file)
}

fn private_read(
    directory: &fs::File,
    name: &str,
    limit: u64,
    public_certificate: bool,
) -> Result<(Vec<u8>, fs::Metadata)> {
    let file = private_file(directory, name, limit, public_certificate)?;
    let m = file.metadata()?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(Error::new("setup input exceeds limit"));
    }
    Ok((bytes, m))
}

fn setup_environment(project: Option<&Path>) -> Result<HashMap<OsString, OsString>> {
    let mut env = host_environment(project)?;
    env.retain(|k, _| {
        matches!(
            k.to_str(),
            Some(
                "PATH"
                    | "HOME"
                    | "USER"
                    | "LOGNAME"
                    | "XDG_CONFIG_HOME"
                    | "XDG_STATE_HOME"
                    | "XDG_DATA_HOME"
                    | "XDG_CACHE_HOME"
                    | "LANG"
                    | "LC_ALL"
                    | "NO_COLOR"
            )
        )
    });
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ] {
        if env
            .get(&OsString::from(key))
            .is_some_and(|p| !safe_path(Path::new(p), &project.into_iter().collect::<Vec<_>>()))
        {
            return Err(Error::new(
                "setup HOME/XDG paths must be normalized, symlink-free and outside the project",
            ));
        }
    }
    Ok(env)
}

pub(super) fn require_version(version: &str) -> Result<()> {
    if version.trim() != format!("openshell {OPEN_SHELL_VERSION}")
        && version.trim() != format!("openshell v{OPEN_SHELL_VERSION}")
    {
        return Err(Error::new(format!(
            "OpenShell {OPEN_SHELL_VERSION} required; unrecognized version output. Install the pinned CLI explicitly"
        )));
    }
    Ok(())
}

pub(super) fn executable(
    name: &str,
    env: &HashMap<OsString, OsString>,
    project: Option<&Path>,
) -> Result<PathBuf> {
    let path = find_executable(name, env.get(&OsString::from("PATH"))).ok_or_else(|| {
        Error::new(format!(
            "{name} not found outside the current directory; install {name} and add it to PATH"
        ))
    })?;
    if project.is_some_and(|project| path.starts_with(project)) {
        return Err(Error::new(format!(
            "refusing a project-owned {name} executable"
        )));
    }
    Ok(path)
}

pub(super) fn rootless(info: &Value) -> Result<()> {
    if info.pointer("/host/security/rootless") != Some(&Value::Bool(true)) {
        return Err(Error::new(
            "rootless Podman required; check podman info as your normal user (no sudo)",
        ));
    }
    Ok(())
}

pub(super) fn valid_podman_version(text: &str) -> bool {
    text.trim()
        .strip_prefix("podman version ")
        .is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
        })
}

fn prerequisites(
    os: &str,
    project: Option<&Path>,
    env: HashMap<OsString, OsString>,
) -> Result<Tools> {
    require_linux(os)?;
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return Err(Error::new(
            "run dungeon setup as a non-root user, not with sudo",
        ));
    }
    let openshell = executable("openshell", &env, project)?;
    let podman = executable("podman", &env, project)?;
    let mut tools = Tools {
        openshell,
        podman,
        podman_version: String::new(),
        env,
        mappings: Value::Null,
    };
    require_version(&tools.openshell(&["--version"], "OpenShell version check failed")?)?;
    let version = tools.podman(&["--version"], "Podman version check failed")?;
    if !valid_podman_version(&version) {
        return Err(Error::new("unexpected Podman version output"));
    }
    tools.podman_version = version
        .trim()
        .trim_start_matches("podman version ")
        .to_owned();
    if !storage_ready(
        &tools.env,
        project.into_iter().collect::<Vec<_>>().as_slice(),
    ) {
        return Err(Error::new(
            "established default rootless Podman storage required; missing/empty/custom storage is not queried or initialized. Prepare Podman and the pinned image explicitly",
        ));
    }
    let info: Value = native_json(
        &tools.podman(
            &["info", "--format", "json"],
            "rootless Podman unavailable (check podman info)",
        )?,
        "invalid Podman info output",
    )?;
    rootless(&info)?;
    tools.mappings = native_json(
        &tools.podman(
            &["info", "--format", "{{json .Host.IDMappings}}"],
            "Podman identity mapping query failed",
        )?,
        "invalid Podman identity mapping output",
    )?;
    let socket = format!("/run/user/{uid}/podman/podman.sock");
    if !socket_ready(Path::new(&socket)) {
        return Err(Error::new(
            "rootless Podman API socket unavailable; run `systemctl --user enable --now podman.socket` yourself, then retry",
        ));
    }
    // Gateway container must be able to use the rootless engine through the API socket.
    if tools
        .podman(
            &[
                "--url",
                &format!("unix://{socket}"),
                "info",
                "--format",
                "{{.Host.Security.Rootless}}",
            ],
            "rootless Podman API socket is not usable",
        )?
        .trim()
        != "true"
    {
        return Err(Error::new("rootless Podman API socket is not usable"));
    }
    let api_maps = native_json(
        &tools.podman(
            &[
                "--url",
                &format!("unix://{socket}"),
                "info",
                "--format",
                "{{json .Host.IDMappings}}",
            ],
            "Podman API identity query failed",
        )?,
        "invalid Podman API identity output",
    )?;
    if api_maps != tools.mappings {
        return Err(Error::new(
            "Podman API identity does not match the local rootless engine",
        ));
    }
    let graph_root = tools.podman(
        &["info", "--format", "{{.Store.GraphRoot}}"],
        "Podman storage identity query failed",
    )?;
    let api_graph_root = tools.podman(
        &[
            "--url",
            &format!("unix://{socket}"),
            "info",
            "--format",
            "{{.Store.GraphRoot}}",
        ],
        "Podman API storage identity query failed",
    )?;
    if graph_root.trim().is_empty() || graph_root.trim() != api_graph_root.trim() {
        return Err(Error::new(
            "Podman API storage does not match the local rootless engine",
        ));
    }
    Ok(tools)
}

fn registrations(tools: &Tools) -> Result<(Option<Value>, Option<String>)> {
    let home = tools
        .env
        .get(&OsString::from("HOME"))
        .ok_or_else(|| Error::new("setup HOME required"))?;
    let registry = tools
        .env
        .get(&OsString::from("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"))
        .join("openshell/gateways");
    if !present(&registry)? {
        return Ok((None, None));
    }
    directory(&registry, false, true)?;
    let list: Value = native_json(
        &tools.openshell(
            &["gateway", "list", "--output", "json"],
            "OpenShell gateway list failed",
        )?,
        "invalid OpenShell gateway list output",
    )?;
    let list = list
        .as_array()
        .ok_or_else(|| Error::new("invalid OpenShell gateway list"))?;
    let mut names = std::collections::HashSet::new();
    for item in list {
        let name = item["name"]
            .as_str()
            .ok_or_else(|| Error::new("invalid gateway registration name"))?;
        if crate::config::safe_name(name, "gateway").is_err()
            || !names.insert(name)
            || !item["active"].is_boolean()
        {
            return Err(Error::new(
                "ambiguous or malformed gateway registration inventory",
            ));
        }
    }
    if list.iter().filter(|item| item["active"] == true).count() > 1 {
        return Err(Error::new("ambiguous active gateway inventory"));
    }
    let active = list
        .iter()
        .find(|item| item["active"] == true)
        .and_then(|item| item["name"].as_str())
        .map(str::to_owned);
    Ok((
        list.iter().find(|item| item["name"] == GATEWAY).cloned(),
        active,
    ))
}

pub(super) fn compatible_registration(record: &Value) -> Result<()> {
    for (key, expected) in [
        ("endpoint", ENDPOINT),
        ("type", "local"),
        ("auth", "mtls"),
        ("source", "user"),
    ] {
        if record[key] != expected {
            return Err(Error::new(format!(
                "gateway {GATEWAY} conflicts: {key} does not match {expected}; resolve it explicitly in OpenShell"
            )));
        }
    }
    Ok(())
}

pub(super) fn gateway_health(info: &Value) -> Result<()> {
    if info["gateway"] != GATEWAY
        || info["version"] != OPEN_SHELL_VERSION
        || info["status"] != "healthy"
        || info["compute_drivers"].as_array().is_none_or(|drivers| {
            drivers.len() != 1
                || drivers[0]["name"] != "podman"
                || drivers[0]["capabilities"]["driver_version"] != OPEN_SHELL_VERSION
        })
    {
        return Err(Error::new(
            "gateway unhealthy or incompatible; inspect explicitly",
        ));
    }
    Ok(())
}

fn container_arguments(state: &Path, socket: &str) -> Vec<String> {
    let config = state.join("gateway.toml");
    let certs = state.join("certs");
    let db = state.join("db");
    let driver_data = state.join("driver-data");
    vec![
        "run",
        "-d",
        "--name",
        CONTAINER,
        "--label",
        &format!("{LABEL}={GATEWAY}"),
        "--userns",
        "keep-id",
        "--network",
        GATEWAY,
        "-p",
        "127.0.0.1:17670:8080",
        "--security-opt",
        "label=disable",
        "-v",
        &format!("{}:/etc/openshell/gateway.toml:ro", config.display()),
        "-v",
        &format!("{0}:{0}:ro", certs.display()),
        "-v",
        &format!("{0}:{0}", driver_data.display()),
        "-v",
        &format!("{}:/var/openshell", db.display()),
        "-v",
        &format!("{socket}:/var/run/podman.sock"),
        "-e",
        "OPENSHELL_GATEWAY_CONFIG=/etc/openshell/gateway.toml",
        "-e",
        "OPENSHELL_DRIVERS=podman",
        "-e",
        "OPENSHELL_PODMAN_SOCKET=/var/run/podman.sock",
        "-e",
        "OPENSHELL_DB_URL=sqlite:/var/openshell/openshell.db",
        "-e",
        "HOME=/var/openshell",
        "-e",
        "XDG_STATE_HOME=/var/openshell",
        "-e",
        &format!("XDG_DATA_HOME={}", driver_data.display()),
        "-e",
        &format!("OPENSHELL_LOCAL_TLS_DIR={}", certs.display()),
        "-e",
        &format!("OPENSHELL_TLS_CERT={}/server/tls.crt", certs.display()),
        "-e",
        &format!("OPENSHELL_TLS_KEY={}/server/tls.key", certs.display()),
        "-e",
        &format!("OPENSHELL_TLS_CLIENT_CA={}/ca.crt", certs.display()),
        IMAGE,
        "--bind-address",
        "0.0.0.0",
        "--port",
        "8080",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn exists(tools: &Tools, kind: &str, name: &str) -> Result<bool> {
    let output = tools.output(
        &tools.podman,
        &[kind, "exists", name],
        "Podman container lookup failed",
    )?;
    match output.code {
        0 => Ok(true),
        1 => Ok(false),
        _ => Err(Error::new(
            "Podman resource lookup failed; inspect explicitly",
        )),
    }
}

fn environment(values: &Value) -> Option<HashMap<String, String>> {
    let mut map = HashMap::new();
    for value in values.as_array()? {
        let (key, value) = value.as_str()?.split_once('=')?;
        if key.is_empty() || map.insert(key.into(), value.into()).is_some() {
            return None;
        }
    }
    Some(map)
}

fn keep_id_maps(host: &Value) -> Option<Value> {
    let mut result = serde_json::Map::new();
    for (key, target, id) in [
        ("uidmap", "UidMap", unsafe { libc::geteuid() }),
        ("gidmap", "GidMap", unsafe { libc::getegid() }),
    ] {
        let maps = host[key].as_array()?;
        if maps.len() != 2
            || maps[0]["container_id"] != 0
            || maps[0]["host_id"] != id
            || maps[0]["size"] != 1
            || maps[1]["container_id"] != 1
        {
            return None;
        }
        let size = maps[1]["size"].as_u64()?;
        let tail = size.checked_sub(u64::from(id))?;
        if id == 0 || tail == 0 {
            return None;
        }
        result.insert(
            target.into(),
            serde_json::json!([
                format!("0:1:{id}"),
                format!("{id}:0:1"),
                format!("{}:{}:{tail}", id + 1, id + 1)
            ]),
        );
    }
    Some(Value::Object(result))
}

fn compatible_container(
    c: &Value,
    image: &Value,
    mappings: &Value,
    state: &Path,
    socket: &str,
) -> bool {
    let Some(id) = c["Id"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return false;
    };
    let Some(mut expected_env) = environment(&image["Config"]["Env"]) else {
        return false;
    };
    for pair in container_arguments(state, socket)
        .windows(2)
        .filter(|p| p[0] == "-e")
    {
        let Some((key, value)) = pair[1].split_once('=') else {
            return false;
        };
        expected_env.insert(key.into(), value.into());
    }
    expected_env.insert("container".into(), "podman".into());
    expected_env.insert("HOSTNAME".into(), id[..12].into());
    let config = &c["Config"];
    let host = &c["HostConfig"];
    let Some(mounts) = c["Mounts"].as_array() else {
        return false;
    };
    let expected_mounts = [
        (
            state.join("gateway.toml"),
            PathBuf::from("/etc/openshell/gateway.toml"),
            false,
        ),
        (state.join("certs"), state.join("certs"), false),
        (state.join("driver-data"), state.join("driver-data"), true),
        (state.join("db"), PathBuf::from("/var/openshell"), true),
        (
            PathBuf::from(socket),
            PathBuf::from("/var/run/podman.sock"),
            true,
        ),
    ];
    let mounts_ok = mounts.len() == expected_mounts.len()
        && expected_mounts.iter().all(|(source, destination, rw)| {
            mounts
                .iter()
                .filter(|m| {
                    m["Source"] == source.to_string_lossy().as_ref()
                        && m["Destination"] == destination.to_string_lossy().as_ref()
                        && m["Type"] == "bind"
                        && m["RW"] == *rw
                        && m["Propagation"] == "rprivate"
                        && m["Options"]
                            == if source == Path::new(socket) {
                                serde_json::json!(["nosuid", "nodev", "rbind"])
                            } else {
                                serde_json::json!(["rbind"])
                            }
                })
                .count()
                == 1
        });
    let networks = c["NetworkSettings"]["Networks"].as_object();
    c["Name"] == CONTAINER
        && c["Image"] == image["Id"]
        && image["Id"].as_str().is_some_and(|s| !s.is_empty())
        && config["Labels"] == serde_json::json!({LABEL: GATEWAY})
        && config["User"]
            == format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
                libc::getegid()
            })
        && config["Hostname"] == id[..12]
        && config["Entrypoint"] == image["Config"]["Entrypoint"]
        && config["Entrypoint"] == serde_json::json!(["/usr/local/bin/openshell-gateway"])
        && config["WorkingDir"] == image["Config"]["WorkingDir"]
        && config["Cmd"] == serde_json::json!(["--bind-address", "0.0.0.0", "--port", "8080"])
        && environment(&config["Env"]) == Some(expected_env)
        && networks.is_some_and(|n| n.len() == 1 && n.contains_key(GATEWAY))
        && host["PortBindings"]
            == serde_json::json!({"8080/tcp": [{"HostIp":"127.0.0.1", "HostPort":"17670"}]})
        && host["SecurityOpt"] == serde_json::json!(["label=disable"])
        && host["UsernsMode"] == "private"
        && keep_id_maps(mappings).is_some_and(|m| host["IDMappings"] == m)
        && host["NetworkMode"] == "bridge"
        && host["PidMode"] == "private"
        && host["IpcMode"] == "shareable"
        && host["UTSMode"] == "private"
        && host["Privileged"] == false
        && host["ReadonlyRootfs"] == false
        && ["CapAdd", "CapDrop", "Devices", "ExtraHosts"]
            .iter()
            .all(|key| host[key] == serde_json::json!([]))
        && mounts_ok
}

fn check_container(
    tools: &Tools,
    state: &Path,
    socket: &str,
    image: &Value,
    metadata: &fs::Metadata,
) -> Result<bool> {
    if !exists(tools, "container", CONTAINER)? {
        return Ok(false);
    }
    let inspect = native_json(
        &tools.podman(
            &["container", "inspect", CONTAINER],
            "gateway container inspection failed",
        )?,
        "invalid gateway container inspection output",
    )?;
    let containers = inspect
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(|| Error::new("invalid gateway container inspection"))?;
    let c = &containers[0];
    if !compatible_container(c, image, &tools.mappings, state, socket) {
        return Err(Error::new(
            "gateway container differs from exact Dungeon configuration (including legacy driver-data layout); no adoption or overwrite. Inspect explicitly",
        ));
    }
    if c["State"]["Running"] != true {
        return Err(Error::new(
            "Dungeon gateway is stopped; inspect/start it explicitly with Podman. Setup never starts existing containers",
        ));
    }
    // Use Podman's native numeric time, rather than interpreting a timestamp string.
    let start = tools
        .podman(
            &[
                "container",
                "inspect",
                CONTAINER,
                "--format",
                "{{.State.StartedAt.UnixNano}}",
            ],
            "gateway start time query failed",
        )?
        .trim()
        .parse::<i128>()
        .map_err(|_| Error::new("invalid gateway start time"))?;
    if start <= 0
        || start < i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec())
    {
        return Err(Error::new(
            "gateway configuration is newer than the running container; inspect explicitly",
        ));
    }
    Ok(true)
}

fn private_create(dir: &fs::File, name: &str, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new(name).map_err(|_| Error::new("invalid private filename"))?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { fs::File::from_raw_fd(fd) };
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn verify_import_gateway(
    config: &crate::config::Config,
    output: &crate::output::Output,
) -> Result<()> {
    if config.values.gateway != GATEWAY || config.values.workspace != "default" {
        return Err(Error::new(
            "imports require the recognized local Dungeon bind-enabled gateway; foreign/remote gateways unsupported",
        ));
    }
    let home = env::var_os("HOME").ok_or_else(|| Error::new("HOME required"))?;
    let state = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".local/state"))
        .join("dungeon/local-setup");
    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".config"));
    if !safe_path(&state, &[&config.values.project])
        || !safe_path(&config_root, &[&config.values.project])
    {
        return Err(Error::new(
            "import gateway state must be outside the project",
        ));
    }
    let tools = prerequisites(
        env::consts::OS,
        Some(&config.values.project),
        setup_environment(Some(&config.values.project))?,
    )?;
    if !exists(&tools, "container", CONTAINER)? {
        return Err(Error::new(
            "existing exact bind-enabled local gateway required; imports never invoke setup",
        ));
    }
    configure_with_output(
        &tools,
        &state,
        &config_root,
        &format!("/run/user/{}/podman/podman.sock", unsafe {
            libc::geteuid()
        }),
        true,
        true,
        output,
    )?;
    Ok(())
}

pub(crate) fn run(enable_live: bool, dry_run: bool) -> Result<i32> {
    let cwd = env::current_dir()?.canonicalize()?;
    let home = env::var_os("HOME")
        .ok_or_else(|| Error::new("HOME required for local OpenShell gateway"))?;
    let home_path = PathBuf::from(&home).canonicalize()?;
    let project = (cwd != home_path).then_some(cwd.as_path());
    let tools = prerequisites(env::consts::OS, project, setup_environment(project)?)?;
    let state_root = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".local/state"));
    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".config"));
    if !state_root.is_absolute()
        || !config_root.is_absolute()
        || (cwd != home_path && (state_root.starts_with(&cwd) || config_root.starts_with(&cwd)))
    {
        return Err(Error::new(
            "setup state and OpenShell config must be absolute paths outside the current project",
        ));
    }
    let state = state_root.join("dungeon/local-setup");
    configure(
        &tools,
        &state,
        &config_root,
        &format!("/run/user/{}/podman/podman.sock", unsafe {
            libc::geteuid()
        }),
        enable_live,
        dry_run,
    )
}

fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn selected_config(enable_live: bool) -> &'static str {
    if enable_live { LIVE_CONFIG } else { CONFIG }
}

fn existing_mode(text: &str, enable_live: bool) -> Result<bool> {
    match text {
        CONFIG if enable_live => Err(Error::new(
            "existing copy-only gateway cannot be enabled by setup. Use `dungeon setup --copy-only` to verify it and `dungeon init --project-transfer copy` for new projects, or provision a fresh bind-capable gateway after explicit cleanup. No gateway restart or state change was attempted",
        )),
        CONFIG => Ok(false),
        LIVE_CONFIG => Ok(true),
        _ => Err(Error::new(
            "gateway configuration is not an exact Dungeon copy/live mode; refusing adoption or overwrite (including partial bind/mapping configurations)",
        )),
    }
}

fn setup_lock(parent: &Path) -> Result<fs::File> {
    use std::os::fd::FromRawFd;
    let dir = directory(parent, true, true)?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c"setup.lock".as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { fs::File::from_raw_fd(fd) };
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err(Error::new(
            "setup lock must be a private user-owned regular file",
        ));
    }
    file.try_lock().map_err(|_| {
        Error::new("another setup owns the local gateway lock; retry after it finishes")
    })?;
    Ok(file)
}

fn configure(
    tools: &Tools,
    state: &Path,
    config_root: &Path,
    socket: &str,
    enable_live: bool,
    dry_run: bool,
) -> Result<i32> {
    configure_with_output(
        tools,
        state,
        config_root,
        socket,
        enable_live,
        dry_run,
        &crate::output::Output::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn configure_with_output(
    tools: &Tools,
    state: &Path,
    config_root: &Path,
    socket: &str,
    enable_live: bool,
    dry_run: bool,
    output: &crate::output::Output,
) -> Result<i32> {
    let image = native_json(
        &tools.podman(
            &["image", "inspect", IMAGE, "--format", "{{json .}}"],
            "pinned gateway image unavailable; pull the documented immutable image explicitly",
        )?,
        "invalid gateway image inspection output",
    )?;
    if image["Id"]
        .as_str()
        .is_none_or(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        || keep_id_maps(&tools.mappings).is_none()
    {
        return Err(Error::new(
            "pinned image or rootless identity mapping is unrecognized",
        ));
    }
    let registry = config_root.join("openshell/gateways");
    if !safe_path(state, &[]) || !safe_path(&registry, &[]) {
        return Err(Error::new("unsafe setup/registration paths"));
    }
    let (registration, previous_gateway) = registrations(tools)?;
    if let Some(ref record) = registration {
        compatible_registration(record)?;
    }
    let existing = exists(tools, "container", CONTAINER)?;
    if registration.is_some() != existing {
        return Err(Error::new(
            "incomplete Dungeon gateway state: OpenShell registration and Podman container disagree; inspect and resolve explicitly",
        ));
    }
    let network = exists(tools, "network", GATEWAY)?;
    if existing {
        let dir = directory(state, false, true)?;
        let (bytes, metadata) = private_read(&dir, "gateway.toml", 8192, false)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::new("gateway configuration must be UTF-8"))?;
        let live = existing_mode(&text, false)?;
        for path in [
            state.join("certs"),
            state.join("db"),
            state.join("driver-data"),
            registry.join(GATEWAY),
            registry.join(GATEWAY).join("mtls"),
        ] {
            directory(&path, false, true)?;
        }
        for (base, names) in [
            (
                state.join("certs"),
                vec![("ca.crt", true), ("ca.key", false)],
            ),
            (
                state.join("certs/server"),
                vec![("tls.crt", true), ("tls.key", false)],
            ),
            (
                state.join("certs/client"),
                vec![("tls.crt", true), ("tls.key", false)],
            ),
            (
                state.join("certs/jwt"),
                vec![("kid", true), ("public.pem", true), ("signing.pem", false)],
            ),
            (
                registry.join(GATEWAY).join("mtls"),
                vec![("tls.crt", false), ("tls.key", false), ("ca.crt", false)],
            ),
            (registry.join(GATEWAY), vec![("metadata.json", true)]),
        ] {
            let dir = directory(&base, false, true)?;
            for (name, public) in names {
                private_file(&dir, name, 1_048_576, public)?;
            }
        }
        private_file(
            &directory(&state.join("db"), false, true)?,
            "openshell.db",
            u64::MAX,
            false,
        )?;
        if !network
            || tools
                .podman(
                    &["network", "inspect", GATEWAY, "--format", "{{.Driver}}"],
                    "gateway network query failed",
                )?
                .trim()
                != "bridge"
        {
            return Err(Error::new(
                "gateway network is missing or unrecognized; no overwrite",
            ));
        }
        check_container(tools, state, socket, &image, &metadata)?;
        existing_mode(&text, enable_live)?;
        verify_health(tools, 1)?;
        if !output.machine {
            print_capability(live, dry_run, true);
        }
        return Ok(0);
    }
    if network || present(state)? || present(&registry.join(GATEWAY))? {
        return Err(Error::new(
            "partial/conflicting Dungeon gateway files, registration or network; inspect explicitly. No adoption or overwrite",
        ));
    }
    if !output.machine {
        print_capability(enable_live, dry_run, false);
    }
    if dry_run {
        return Ok(0);
    }
    let _lock = setup_lock(
        state
            .parent()
            .ok_or_else(|| Error::new("setup parent required"))?,
    )?;
    // Recheck all named resources after taking the fresh-setup lock.
    if registrations(tools)?.0.is_some()
        || exists(tools, "container", CONTAINER)?
        || exists(tools, "network", GATEWAY)?
        || present(state)?
        || present(&registry.join(GATEWAY))?
    {
        return Err(Error::new(
            "local gateway state changed during preflight; refusing setup",
        ));
    }
    {
        println!(
            "Setting up {GATEWAY}: Linux, rootless Podman, OpenShell {OPEN_SHELL_VERSION}, workspace default (mTLS; no sandbox or project upload)."
        );
        let state_dir = directory(state, true, true)?;
        directory(&state.join("certs"), true, true)?;
        directory(&state.join("db"), true, true)?;
        directory(&state.join("driver-data"), true, true)?;
        private_create(
            &state_dir,
            "gateway.toml",
            selected_config(enable_live).as_bytes(),
        )?;
        let certs = state.join("certs");
        tools.mutate(
            &tools.podman,
            &[
                "run",
                "--rm",
                "--network",
                "none",
                "--userns",
                "keep-id",
                "--security-opt",
                "label=disable",
                "-v",
                &format!("{}:/certs", certs.display()),
                IMAGE,
                "generate-certs",
                "--output-dir",
                "/certs",
                "--server-san",
                CONTAINER,
                "--server-san",
                "127.0.0.1",
            ],
            "OpenShell certificate generation failed",
        )?;
        let mtls = config_root
            .join("openshell/gateways")
            .join(GATEWAY)
            .join("mtls");
        let mtls_dir = directory(&mtls, true, true)?;
        for (from, to) in [
            ("client/tls.crt", "tls.crt"),
            ("client/tls.key", "tls.key"),
            ("ca.crt", "ca.crt"),
        ] {
            let source = certs.join(from);
            let source_dir = directory(source.parent().expect("certificate parent"), false, true)?;
            let (bytes, _) = private_read(
                &source_dir,
                source
                    .file_name()
                    .expect("certificate name")
                    .to_str()
                    .ok_or_else(|| Error::new("invalid certificate filename"))?,
                1_048_576,
                from.ends_with(".crt"),
            )?;
            private_create(&mtls_dir, to, &bytes)?;
        }
        if !exists(tools, "network", GATEWAY)? {
            tools.mutate(
                &tools.podman,
                &["network", "create", GATEWAY],
                "Podman network creation failed",
            )?;
        } else {
            return Err(Error::new(
                "Podman network dungeon-local already exists without a Dungeon gateway; inspect and resolve explicitly",
            ));
        }
        let args = container_arguments(state, socket);
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        tools.mutate(&tools.podman, &refs, "gateway creation failed")?;
        tools.mutate(
            &tools.openshell,
            &["gateway", "add", ENDPOINT, "--local", "--name", GATEWAY],
            "gateway registration failed",
        )?;
        if let Some(previous) = previous_gateway.filter(|name| name != GATEWAY) {
            tools.mutate(
                &tools.openshell,
                &["gateway", "select", &previous],
                "could not restore previously active OpenShell gateway",
            )?;
        }
    }
    let dir = directory(state, false, true)?;
    let (_, metadata) = private_read(&dir, "gateway.toml", 8192, false)?;
    check_container(tools, state, socket, &image, &metadata)?;
    verify_health(tools, 10)?;
    println!(
        "{GATEWAY}: healthy rootless Podman {}; OpenShell {OPEN_SHELL_VERSION}. Setup health is not containment validation; support remains limited to recorded combinations.",
        tools.podman_version
    );
    println!(
        "Next: review a private project configuration; new projects default to a single live read/write project mount. Use init --project-transfer copy to opt out."
    );
    Ok(0)
}

fn print_capability(live: bool, dry_run: bool, existing: bool) {
    println!(
        "{}: {} {GATEWAY}; driver enable_bind_mounts={live}; driver userns={}; gateway/certificate userns=keep-id unchanged.",
        if dry_run {
            "Read-only setup plan"
        } else {
            "Setup"
        },
        if existing { "verify" } else { "create" },
        if live {
            "keep-id:uid=998,gid=998"
        } else {
            "unset"
        }
    );
    println!(
        "Gateway capability ceiling only; no project mounts, sandbox creation, upload, policy widening or project configuration changes. New projects default to a reviewed single-project live mount; existing private bindings are unchanged."
    );
    if !live {
        println!(
            "This copy-only gateway cannot create new default-live projects; initialize them with --project-transfer copy or explicitly provision a fresh bind-capable gateway."
        );
    }
}

fn verify_health(tools: &Tools, attempts: usize) -> Result<()> {
    let mut failure = None;
    for attempt in 0..attempts {
        match tools
            .openshell(
                &[
                    "--gateway",
                    GATEWAY,
                    "--workspace",
                    "default",
                    "gateway",
                    "info",
                    "--output",
                    "json",
                ],
                "gateway health check failed",
            )
            .and_then(|text| native_json(&text, "invalid gateway health output"))
            .and_then(|info| gateway_health(&info))
        {
            Ok(()) => {
                failure = None;
                break;
            }
            Err(error) => failure = Some(error),
        }
        if attempt + 1 < attempts {
            thread::sleep(Duration::from_secs(1));
        }
    }
    if let Some(error) = failure {
        return Err(Error::new(format!("gateway health failed: {error}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    pub(super) fn host_maps() -> Value {
        serde_json::json!({"uidmap":[{"container_id":0,"host_id":unsafe { libc::geteuid() },"size":1},{"container_id":1,"host_id":524288,"size":65536}],"gidmap":[{"container_id":0,"host_id":unsafe { libc::getegid() },"size":1},{"container_id":1,"host_id":524288,"size":65536}]})
    }

    pub(super) fn image() -> Value {
        serde_json::json!({"Id":"b".repeat(64),"Config":{"Env":["PATH=/usr/bin:/bin","SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt"],"Entrypoint":["/usr/local/bin/openshell-gateway"],"WorkingDir":"/app"}})
    }

    pub(super) fn container(state: &Path, socket: &str) -> Value {
        let mut env = image()["Config"]["Env"].as_array().unwrap().clone();
        env.extend(
            container_arguments(state, socket)
                .windows(2)
                .filter(|p| p[0] == "-e")
                .map(|p| Value::String(p[1].clone())),
        );
        env.extend([
            Value::String("container=podman".into()),
            Value::String(format!("HOSTNAME={}", "a".repeat(12))),
        ]);
        let mounts = [(state.join("gateway.toml"), PathBuf::from("/etc/openshell/gateway.toml"), false), (state.join("certs"), state.join("certs"), false), (state.join("db"), PathBuf::from("/var/openshell"), true), (state.join("driver-data"), state.join("driver-data"), true), (PathBuf::from(socket), PathBuf::from("/var/run/podman.sock"), true)].map(|(source, destination, rw)| serde_json::json!({"Type":"bind","Source":source,"Destination":destination,"RW":rw,"Propagation":"rprivate","Options":if source == Path::new(socket) { serde_json::json!(["nosuid","nodev","rbind"]) } else { serde_json::json!(["rbind"]) }}));
        serde_json::json!({"Id":"a".repeat(64),"Name":CONTAINER,"Image":image()["Id"],"Config":{"User":format!("{}:{}",unsafe {libc::geteuid()},unsafe {libc::getegid()}),"Hostname":"a".repeat(12),"Labels":{LABEL:GATEWAY},"Entrypoint":["/usr/local/bin/openshell-gateway"],"WorkingDir":"/app","Cmd":["--bind-address","0.0.0.0","--port","8080"],"Env":env},"Mounts":mounts,"NetworkSettings":{"Networks":{GATEWAY:{}}},"HostConfig":{"IDMappings":keep_id_maps(&host_maps()).unwrap(),"UsernsMode":"private","NetworkMode":"bridge","PidMode":"private","IpcMode":"shareable","UTSMode":"private","Privileged":false,"ReadonlyRootfs":false,"CapAdd":[],"CapDrop":[],"Devices":[],"ExtraHosts":[],"SecurityOpt":["label=disable"],"PortBindings":{"8080/tcp":[{"HostIp":"127.0.0.1","HostPort":"17670"}]}},"State":{"Running":true}})
    }

    struct Fixture {
        root: tempfile::TempDir,
        tools: Tools,
        state: PathBuf,
        config: PathBuf,
        socket: String,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let config = root.path().join("config");
            let state = root.path().join("state/dungeon/local-setup");
            let socket = root.path().join("podman.sock").display().to_string();
            let script = root.path().join("native-fixture");
            fs::write(&script, r#"#!/bin/sh
set -eu
case "$1 $2" in
  'image inspect') cat "$FIXTURE/image.json" ;;
  'container exists') test -f "$FIXTURE/container.json" ;;
  'container inspect') if [ "$#" -gt 3 ]; then printf '9223372036854775807\n'; else cat "$FIXTURE/container.json"; fi ;;
  'network exists') test -f "$FIXTURE/network" ;;
  'network inspect') printf 'bridge\n' ;;
  'gateway list') cat "$FIXTURE/registrations.json" ;;
  '--gateway dungeon-local') cat "$FIXTURE/health.json" ;;
  'network create') printf 'network create\n' >> "$FIXTURE/mutations"; touch "$FIXTURE/network" ;;
  'run --rm')
    printf 'generate-certs\n' >> "$FIXTURE/mutations"
    for arg do case "$arg" in */certs:/certs) certs="${arg%:/certs}";; esac; done
    mkdir -m 700 "$certs/client" "$certs/server" "$certs/jwt"
    printf 'synthetic cert' > "$certs/client/tls.crt"
    printf 'synthetic key' > "$certs/client/tls.key"
    printf 'synthetic CA' > "$certs/ca.crt"
    chmod 644 "$certs/client/tls.crt" "$certs/ca.crt"
    chmod 600 "$certs/client/tls.key" ;;
  'run -d')
    printf 'gateway create\n' >> "$FIXTURE/mutations"
    for arg do case "$arg" in */db:/var/openshell) db="${arg%:/var/openshell}";; */certs:*/certs:ro) certs="${arg%%:*}";; esac; done
    for file in ca.key server/tls.crt server/tls.key jwt/kid jwt/public.pem jwt/signing.pem; do printf 'synthetic material' > "$certs/$file"; chmod 600 "$certs/$file"; done
    printf 'synthetic database' > "$db/openshell.db"; chmod 600 "$db/openshell.db"
    cp "$FIXTURE/new-container.json" "$FIXTURE/container.json" ;;
  'gateway add') printf 'gateway add\n' >> "$FIXTURE/mutations"; cp "$FIXTURE/new-registrations.json" "$FIXTURE/registrations.json"; printf 'synthetic metadata' > "$XDG_CONFIG_HOME/openshell/gateways/dungeon-local/metadata.json" ;;
  *) printf 'forbidden invocation' >&2; exit 9 ;;
esac
"#).unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
            let write = |name: &str, value: Value| {
                fs::write(root.path().join(name), serde_json::to_vec(&value).unwrap()).unwrap()
            };
            write("image.json", image());
            write(
                "new-container.json",
                serde_json::json!([container(&state, &socket)]),
            );
            write("registrations.json", serde_json::json!([]));
            write(
                "new-registrations.json",
                serde_json::json!([{"name":GATEWAY,"active":true,"endpoint":ENDPOINT,"type":"local","auth":"mtls","source":"user"}]),
            );
            write(
                "health.json",
                serde_json::json!({"gateway":GATEWAY,"version":OPEN_SHELL_VERSION,"status":"healthy","compute_drivers":[{"name":"podman","capabilities":{"driver_version":OPEN_SHELL_VERSION}}]}),
            );
            let tools = Tools {
                openshell: script.clone(),
                podman: script,
                podman_version: "5.8.4".into(),
                mappings: host_maps(),
                env: HashMap::from([
                    (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
                    (OsString::from("HOME"), root.path().as_os_str().into()),
                    (OsString::from("XDG_CONFIG_HOME"), config.as_os_str().into()),
                    (OsString::from("FIXTURE"), root.path().as_os_str().into()),
                ]),
            };
            Self {
                root,
                tools,
                state,
                config,
                socket,
            }
        }

        fn run(&self, live: bool, dry: bool) -> Result<i32> {
            configure(
                &self.tools,
                &self.state,
                &self.config,
                &self.socket,
                live,
                dry,
            )
        }

        fn establish(&self, live: bool) {
            directory(&self.state, true, true).unwrap();
            for path in [
                self.state.join("certs"),
                self.state.join("db"),
                self.state.join("driver-data"),
                self.config.join("openshell/gateways/dungeon-local/mtls"),
            ] {
                directory(&path, true, true).unwrap();
            }
            let dir = directory(&self.state, false, true).unwrap();
            private_create(&dir, "gateway.toml", selected_config(live).as_bytes()).unwrap();
            for name in [
                "certs/ca.crt",
                "certs/ca.key",
                "certs/server/tls.crt",
                "certs/server/tls.key",
                "certs/client/tls.crt",
                "certs/client/tls.key",
                "certs/jwt/kid",
                "certs/jwt/public.pem",
                "certs/jwt/signing.pem",
                "db/openshell.db",
            ] {
                let path = self.state.join(name);
                let dir = directory(path.parent().unwrap(), true, true).unwrap();
                private_create(
                    &dir,
                    path.file_name().unwrap().to_str().unwrap(),
                    b"synthetic material",
                )
                .unwrap();
            }
            let gateway = self.config.join("openshell/gateways/dungeon-local");
            private_create(
                &directory(&gateway, false, true).unwrap(),
                "metadata.json",
                b"synthetic metadata",
            )
            .unwrap();
            let mtls = directory(&gateway.join("mtls"), false, true).unwrap();
            for name in ["tls.crt", "tls.key", "ca.crt"] {
                private_create(&mtls, name, b"synthetic material").unwrap();
            }
            fs::write(self.state.join("db/synthetic-db"), "preserve database").unwrap();
            fs::write(self.state.join("certs/synthetic-key"), "preserve TLS").unwrap();
            fs::copy(
                self.root.path().join("new-container.json"),
                self.root.path().join("container.json"),
            )
            .unwrap();
            fs::copy(
                self.root.path().join("new-registrations.json"),
                self.root.path().join("registrations.json"),
            )
            .unwrap();
            fs::write(self.root.path().join("network"), "").unwrap();
        }
    }

    #[test]
    fn fresh_setup_selects_only_driver_capability_and_repeats_without_downgrade() {
        for live in [false, true] {
            let f = Fixture::new();
            assert_eq!(f.run(live, false).unwrap(), 0);
            assert_eq!(
                fs::read_to_string(f.state.join("gateway.toml")).unwrap(),
                selected_config(live)
            );
            let before = fs::read(f.root.path().join("mutations")).unwrap();
            assert_eq!(f.run(false, false).unwrap(), 0);
            if live {
                assert_eq!(f.run(true, false).unwrap(), 0);
            }
            assert_eq!(fs::read(f.root.path().join("mutations")).unwrap(), before);
            assert_eq!(
                fs::read_to_string(f.state.join("gateway.toml")).unwrap(),
                selected_config(live)
            );
            assert!(
                container_arguments(&f.state, &f.socket)
                    .windows(2)
                    .any(|p| p == ["--userns", "keep-id"])
            );
        }
    }

    #[test]
    fn copy_migration_refuses_without_touching_owned_state() {
        let f = Fixture::new();
        f.establish(false);
        let paths = [
            f.state.join("gateway.toml"),
            f.state.join("db/synthetic-db"),
            f.state.join("certs/synthetic-key"),
            f.root.path().join("container.json"),
            f.root.path().join("registrations.json"),
        ];
        let before = paths.each_ref().map(|p| fs::read(p).unwrap());
        for dry in [false, true] {
            assert!(
                f.run(true, dry)
                    .unwrap_err()
                    .to_string()
                    .contains("copy-only gateway cannot be enabled")
            );
        }
        assert_eq!(paths.each_ref().map(|p| fs::read(p).unwrap()), before);
        assert!(!f.root.path().join("mutations").exists());
        assert!(!f.state.parent().unwrap().join("setup.lock").exists());
    }

    #[test]
    fn dry_run_creates_neither_setup_nor_registration_nor_lock() {
        let f = Fixture::new();
        for live in [false, true] {
            assert_eq!(f.run(live, true).unwrap(), 0);
        }
        assert!(!f.config.exists());
        assert!(!f.state.parent().unwrap().exists());
        assert!(!f.root.path().join("mutations").exists());
        f.establish(true);
        assert_eq!(f.run(false, true).unwrap(), 0);
        assert!(!f.root.path().join("mutations").exists());
    }

    #[test]
    fn exact_container_refuses_extra_duplicate_or_changed_authority() {
        let state = Path::new("/private/state");
        let socket = "/run/user/1000/podman/podman.sock";
        let good = container(state, socket);
        assert!(compatible_container(
            &good,
            &image(),
            &host_maps(),
            state,
            socket
        ));
        let mut cases = Vec::new();
        let mut c = good.clone();
        c["Mounts"]
            .as_array_mut()
            .unwrap()
            .push(good["Mounts"][0].clone());
        cases.push(c);
        let mut c = good.clone();
        c["Mounts"][0]["Destination"] = serde_json::json!("/elsewhere");
        cases.push(c);
        let mut c = good.clone();
        c["Mounts"][0]["RW"] = serde_json::json!(true);
        cases.push(c);
        let mut c = good.clone();
        c["Config"]["Env"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("UNREVIEWED=secret"));
        cases.push(c);
        let mut c = good.clone();
        c["Config"]["Env"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("HOME=/var/openshell"));
        cases.push(c);
        for (pointer, value) in [
            ("/Config/User", serde_json::json!("998:998")),
            ("/Config/Cmd", serde_json::json!(["sh"])),
            ("/HostConfig/UsernsMode", serde_json::json!("host")),
            ("/HostConfig/SecurityOpt", serde_json::json!([])),
            ("/HostConfig/Privileged", serde_json::json!(true)),
            (
                "/HostConfig/PortBindings/8080~1tcp/0/HostIp",
                serde_json::json!("0.0.0.0"),
            ),
            ("/Image", serde_json::json!("unknown")),
        ] {
            let mut c = good.clone();
            *c.pointer_mut(pointer).unwrap() = value;
            cases.push(c);
        }
        for c in cases {
            assert!(!compatible_container(
                &c,
                &image(),
                &host_maps(),
                state,
                socket
            ));
        }
    }

    #[test]
    fn configuration_modes_are_closed_but_diagnostics_keep_partial_settings_separate() {
        assert!(!existing_mode(CONFIG, false).unwrap());
        assert!(existing_mode(CONFIG, true).is_err());
        for enable in [false, true] {
            assert!(existing_mode(LIVE_CONFIG, enable).unwrap());
        }
        for text in [
            CONFIG.replace("false", "true"),
            format!("{CONFIG}userns = \"keep-id:uid=998,gid=998\"\n"),
            format!("{LIVE_CONFIG}extra = true\n"),
            LIVE_CONFIG.replace("userns =", "userns_mode ="),
        ] {
            assert!(existing_mode(&text, false).is_err());
        }
        assert_eq!(
            parse_bind_config(&CONFIG.replace("false", "true")),
            Some(BindConfig {
                enabled: true,
                mapping: false
            })
        );
        assert_eq!(
            parse_bind_config(LIVE_CONFIG),
            Some(BindConfig {
                enabled: true,
                mapping: true
            })
        );
    }

    #[test]
    fn native_json_failures_expose_only_fixed_context() {
        for text in [
            r#"{"secret-\u001b[31m":1,"secret-\u001b[31m":2}"#,
            "\u{1b}[31msensitive invalid output",
            "{\"key\":",
        ] {
            assert_eq!(
                native_json(text, "invalid native query output")
                    .unwrap_err()
                    .to_string(),
                "invalid native query output"
            );
        }
        let f = Fixture::new();
        fs::write(
            f.root.path().join("image.json"),
            r#"{"secret-\u001b[31m":1,"secret-\u001b[31m":2}"#,
        )
        .unwrap();
        assert_eq!(
            f.run(true, true).unwrap_err().to_string(),
            "invalid gateway image inspection output"
        );
        assert!(!f.root.path().join("mutations").exists());
    }

    #[test]
    fn private_inputs_refuse_fifo_links_permissions_and_unsafe_ancestors() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let dir = directory(root.path(), false, true).unwrap();
        let file = root.path().join("gateway.toml");
        let name = std::ffi::CString::new(file.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(private_read(&dir, "gateway.toml", 8192, false).is_err());
        fs::remove_file(&file).unwrap();
        private_create(&dir, "real", CONFIG.as_bytes()).unwrap();
        symlink("real", &file).unwrap();
        assert!(private_read(&dir, "gateway.toml", 8192, false).is_err());
        fs::remove_file(&file).unwrap();
        fs::hard_link(root.path().join("real"), &file).unwrap();
        assert!(private_read(&dir, "gateway.toml", 8192, false).is_err());
        fs::remove_file(&file).unwrap();
        private_create(&dir, "gateway.toml", CONFIG.as_bytes()).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(private_read(&dir, "gateway.toml", 8192, false).is_err());
        symlink(root.path(), root.path().join("alias")).unwrap();
        assert!(directory(&root.path().join("alias"), false, true).is_err());
        assert!(directory(&root.path().join("bad:path"), true, true).is_err());
        assert!(!socket_ready(&file));
    }

    #[test]
    fn fresh_setup_lock_excludes_competing_creators() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let first = setup_lock(root.path()).unwrap();
        assert!(setup_lock(root.path()).is_err());
        first.unlock().unwrap();
        drop(first);
        assert!(setup_lock(root.path()).is_ok());
    }

    #[test]
    fn partial_or_foreign_setup_is_not_adopted_or_repaired() {
        let fresh = Fixture::new();
        fs::write(fresh.root.path().join("network"), "").unwrap();
        assert!(fresh.run(true, false).is_err());
        assert!(!fresh.state.exists());
        assert!(!fresh.root.path().join("mutations").exists());
        let f = Fixture::new();
        f.establish(true);
        let mut c = container(&f.state, &f.socket);
        c["Config"]["Env"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("UNKNOWN=untrusted-secret"));
        fs::write(
            f.root.path().join("container.json"),
            serde_json::to_vec(&serde_json::json!([c])).unwrap(),
        )
        .unwrap();
        let error = f.run(false, false).unwrap_err().to_string();
        assert!(!error.contains("untrusted-secret"));
        assert!(!f.root.path().join("mutations").exists());
        fs::set_permissions(&f.state, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(f.run(false, true).is_err());
        assert!(!f.state.parent().unwrap().join("setup.lock").exists());
    }

    #[test]
    fn supported_path_is_exact_and_never_selects_vm_or_sudo() {
        assert!(require_linux("linux").is_ok());
        assert!(require_linux("macos").is_err());
        assert!(require_version("openshell 0.0.116\n").is_ok());
        assert!(require_version("openshell 0.0.117").is_err());
        let state = Path::new("/tmp/dungeon");
        let args = container_arguments(state, "/run/user/1000/podman/podman.sock");
        let driver_data = state.join("driver-data").display().to_string();
        let driver_data_mount = format!("{driver_data}:{driver_data}");
        let driver_data_env = format!("XDG_DATA_HOME={driver_data}");
        assert!(args.windows(2).any(|p| p == ["--network", "dungeon-local"]));
        assert!(
            args.windows(2)
                .any(|p| p == ["-v", driver_data_mount.as_str()])
        );
        assert!(
            args.windows(2)
                .any(|p| p == ["-e", driver_data_env.as_str()])
        );
        assert!(
            args.windows(2)
                .any(|p| p == ["-e", "OPENSHELL_DRIVERS=podman"])
        );
        for forbidden in ["sudo", "microvm", "--privileged", "sh", "-c"] {
            assert!(!args.iter().any(|a| a == forbidden));
        }
    }

    #[test]
    fn incompatible_registration_refused() {
        let valid = serde_json::json!({"endpoint": ENDPOINT, "type":"local", "auth":"mtls", "source":"user"});
        assert!(compatible_registration(&valid).is_ok());
        let mut conflict = valid;
        conflict["auth"] = "plaintext".into();
        assert!(compatible_registration(&conflict).is_err());
        assert!(gateway_health(&serde_json::json!({"status":"unhealthy"})).is_err());
        assert!(gateway_health(&serde_json::json!({"gateway":GATEWAY,"version":OPEN_SHELL_VERSION,"status":"healthy","compute_drivers":[{"name":"microvm","capabilities":{"driver_version":OPEN_SHELL_VERSION}}]})).is_err());
    }

    #[test]
    fn missing_tools_and_rootless_failure_are_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let env = HashMap::from([(
            OsString::from("PATH"),
            temp.path().as_os_str().to_os_string(),
        )]);
        assert!(
            executable("openshell", &env, Some(temp.path()))
                .unwrap_err()
                .to_string()
                .contains("not found")
        );
        assert!(
            executable("podman", &env, Some(temp.path()))
                .unwrap_err()
                .to_string()
                .contains("not found")
        );
        assert!(rootless(&serde_json::json!({"host":{"security":{"rootless":false}}})).is_err());
    }
}
