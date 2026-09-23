//! Local 0.0.116 gateway bootstrap. OpenShell owns registrations; Podman owns containers.
use super::{find_executable, host_environment};
use crate::config::create_private_dir;
use crate::{Error, OPEN_SHELL_VERSION, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

const GATEWAY: &str = "dungeon-local";
const CONTAINER: &str = "openshell-gateway-dungeon-local";
const ENDPOINT: &str = "https://127.0.0.1:17670";
const IMAGE: &str = "ghcr.io/nvidia/openshell/gateway@sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645";
const AGENT_IMAGE: &str = "localhost/dungeon-agent:0.0.3-dev";
const LABEL: &str = "agentarchaeology.ai/dungeon-gateway";
const CONFIG: &str = "[openshell]\nversion = 1\n\n[openshell.drivers.podman]\nnetwork_name = \"dungeon-local\"\ngrpc_endpoint = \"https://openshell-gateway-dungeon-local:8080\"\nimage_pull_policy = \"missing\"\nenable_bind_mounts = false\n";

struct Tools {
    openshell: PathBuf,
    podman: PathBuf,
    podman_version: String,
    env: HashMap<OsString, OsString>,
}

impl Tools {
    fn command(&self, binary: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir("/")
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null());
        command
    }

    fn output(&self, binary: &Path, args: &[&str], context: &str) -> Result<Output> {
        self.command(binary, args)
            .output()
            .map_err(|e| Error::new(format!("{context}: {e}")))
    }

    fn checked(&self, binary: &Path, args: &[&str], context: &str) -> Result<String> {
        let output = self.output(binary, args, context)?;
        if !output.status.success() {
            return Err(Error::new(format!(
                "{context}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| Error::new(format!("{context}: non-UTF-8 output")))
    }

    fn podman(&self, args: &[&str], context: &str) -> Result<String> {
        self.checked(&self.podman, args, context)
    }

    fn openshell(&self, args: &[&str], context: &str) -> Result<String> {
        self.checked(&self.openshell, args, context)
    }
}

fn require_linux(os: &str) -> Result<()> {
    if os != "linux" {
        return Err(Error::new(
            "dungeon setup supports Linux only (rootless Podman); no MicroVM fallback",
        ));
    }
    Ok(())
}

fn require_version(version: &str) -> Result<()> {
    if version.trim() != format!("openshell {OPEN_SHELL_VERSION}")
        && version.trim() != format!("openshell v{OPEN_SHELL_VERSION}")
    {
        return Err(Error::new(format!(
            "OpenShell {OPEN_SHELL_VERSION} required; found {}. Install the pinned CLI explicitly",
            version.trim()
        )));
    }
    Ok(())
}

fn executable(
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

fn rootless(info: &Value) -> Result<()> {
    if info.pointer("/host/security/rootless") != Some(&Value::Bool(true)) {
        return Err(Error::new(
            "rootless Podman required; check podman info as your normal user (no sudo)",
        ));
    }
    Ok(())
}

fn prerequisites(
    os: &str,
    project: Option<&Path>,
    env: HashMap<OsString, OsString>,
) -> Result<Tools> {
    require_linux(os)?;
    let openshell = executable("openshell", &env, project)?;
    let podman = executable("podman", &env, project)?;
    let mut tools = Tools {
        openshell,
        podman,
        podman_version: String::new(),
        env,
    };
    require_version(&tools.openshell(&["--version"], "OpenShell version check failed")?)?;
    let version = tools.podman(&["--version"], "Podman version check failed")?;
    if !version.trim().starts_with("podman version ") {
        return Err(Error::new("unexpected Podman version output"));
    }
    tools.podman_version = version
        .trim()
        .trim_start_matches("podman version ")
        .to_owned();
    let info: Value = serde_json::from_str(&tools.podman(
        &["info", "--format", "json"],
        "rootless Podman unavailable (check podman info)",
    )?)?;
    rootless(&info)?;
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return Err(Error::new(
            "run dungeon setup as a non-root user, not with sudo",
        ));
    }
    let socket = format!("/run/user/{uid}/podman/podman.sock");
    if !Path::new(&socket).exists() {
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
    Ok(tools)
}

fn registrations(tools: &Tools) -> Result<(Option<Value>, Option<String>)> {
    let list: Value = serde_json::from_str(&tools.openshell(
        &["gateway", "list", "--output", "json"],
        "OpenShell gateway list failed",
    )?)?;
    let list = list
        .as_array()
        .ok_or_else(|| Error::new("invalid OpenShell gateway list"))?;
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

fn compatible_registration(record: &Value) -> Result<()> {
    for (key, expected) in [
        ("endpoint", ENDPOINT),
        ("type", "local"),
        ("auth", "mtls"),
        ("source", "user"),
    ] {
        if record[key] != expected {
            return Err(Error::new(format!(
                "gateway {GATEWAY} conflicts: {key} is {}, expected {expected}; resolve it explicitly in OpenShell",
                record[key]
            )));
        }
    }
    Ok(())
}

fn gateway_health(info: &Value) -> Result<()> {
    if info["gateway"] != GATEWAY
        || info["version"] != OPEN_SHELL_VERSION
        || info["status"] != "healthy"
        || info["compute_drivers"].as_array().is_none_or(|drivers| {
            drivers.len() != 1
                || drivers[0]["name"] != "podman"
                || drivers[0]["capabilities"]["driver_version"] != OPEN_SHELL_VERSION
        })
    {
        return Err(Error::new(format!(
            "gateway unhealthy or incompatible: {info}"
        )));
    }
    Ok(())
}

fn container_arguments(state: &Path, socket: &str) -> Vec<String> {
    let config = state.join("gateway.toml");
    let certs = state.join("certs");
    let db = state.join("db");
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

fn check_container(tools: &Tools, state: &Path, socket: &str) -> Result<bool> {
    let output = tools.output(
        &tools.podman,
        &["container", "exists", CONTAINER],
        "Podman container lookup failed",
    )?;
    if output.status.code() == Some(1) {
        return Ok(false);
    }
    if !output.status.success() {
        return Err(Error::new("Podman container lookup failed"));
    }
    let inspect: Value = serde_json::from_str(&tools.podman(
        &["inspect", CONTAINER],
        "gateway container inspection failed",
    )?)?;
    let c = &inspect[0];
    let expected_mounts = [
        state.join("gateway.toml"),
        state.join("certs"),
        state.join("db"),
        PathBuf::from(socket),
    ];
    let mounts = c["Mounts"]
        .as_array()
        .ok_or_else(|| Error::new("invalid Podman inspect mounts"))?;
    if c["Config"]["Labels"][LABEL] != GATEWAY
        || c["Image"]
            != tools
                .podman(
                    &["image", "inspect", IMAGE, "--format", "{{.Id}}"],
                    "gateway image lookup failed",
                )?
                .trim()
        || c["NetworkSettings"]["Networks"][GATEWAY].is_null()
        || c["HostConfig"]["PortBindings"]["8080/tcp"][0]["HostIp"] != "127.0.0.1"
        || c["HostConfig"]["PortBindings"]["8080/tcp"][0]["HostPort"] != "17670"
        || c["HostConfig"]["SecurityOpt"] != serde_json::json!(["label=disable"])
        || c["Config"]["Cmd"] != serde_json::json!(["--bind-address", "0.0.0.0", "--port", "8080"])
        || expected_mounts.iter().any(|p| {
            !mounts
                .iter()
                .any(|m| m["Source"] == p.to_string_lossy().as_ref())
        })
        || ![
            "OPENSHELL_DRIVERS=podman",
            "OPENSHELL_PODMAN_SOCKET=/var/run/podman.sock",
            "OPENSHELL_DB_URL=sqlite:/var/openshell/openshell.db",
        ]
        .iter()
        .all(|v| {
            c["Config"]["Env"]
                .as_array()
                .is_some_and(|e| e.contains(&Value::String((*v).into())))
        })
    {
        return Err(Error::new(format!(
            "gateway container {CONTAINER} differs from Dungeon rootless-Podman configuration; resolve it explicitly, no overwrite"
        )));
    }
    if c["State"]["Running"] != true {
        return Err(Error::new(format!(
            "gateway container {CONTAINER} is stopped; inspect/start it explicitly with Podman"
        )));
    }
    Ok(true)
}

fn private_file_copy(source: &Path, target: &Path) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let bytes = fs::read(source)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)?;
    file.write_all(&bytes)?;
    Ok(())
}

fn private_text(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

pub(crate) fn run() -> Result<i32> {
    let cwd = env::current_dir()?.canonicalize()?;
    let home = env::var_os("HOME")
        .ok_or_else(|| Error::new("HOME required for local OpenShell gateway"))?;
    let home_path = PathBuf::from(&home).canonicalize()?;
    let project = (cwd != home_path).then_some(cwd.as_path());
    let tools = prerequisites(env::consts::OS, project, host_environment(project)?)?;
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
    let (registration, previous_gateway) = registrations(&tools)?;
    if let Some(ref record) = registration {
        compatible_registration(record)?;
    }
    let uid = unsafe { libc::geteuid() };
    let socket = format!("/run/user/{uid}/podman/podman.sock");
    let existing = check_container(&tools, &state, &socket)?;
    if registration.is_some() != existing {
        return Err(Error::new(
            "incomplete Dungeon gateway state: OpenShell registration and Podman container disagree; inspect and resolve explicitly",
        ));
    }
    if existing {
        if fs::read_to_string(state.join("gateway.toml"))? != CONFIG {
            return Err(Error::new(
                "Dungeon gateway config differs; refusing to adopt it",
            ));
        }
    } else {
        println!(
            "Setting up {GATEWAY}: Linux, rootless Podman, OpenShell {OPEN_SHELL_VERSION}, workspace default (mTLS; no sandbox or project upload)."
        );
        // Do not download a gateway image implicitly. The pinned upstream image is an explicit prerequisite.
        tools.podman(&["image", "inspect", IMAGE, "--format", "{{.Id}}"], &format!("pinned OpenShell gateway image unavailable; run `podman pull {IMAGE}` explicitly and retry"))?;
        if state.exists()
            || config_root
                .join("openshell/gateways")
                .join(GATEWAY)
                .exists()
        {
            return Err(Error::new(
                "existing Dungeon/OpenShell gateway files without a matching running gateway; inspect and resolve explicitly",
            ));
        }
        create_private_dir(&state.join("certs"))?;
        create_private_dir(&state.join("db"))?;
        private_text(&state.join("gateway.toml"), CONFIG)?;
        let certs = state.join("certs");
        tools.podman(
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
        create_private_dir(&mtls)?;
        for (from, to) in [
            ("client/tls.crt", "tls.crt"),
            ("client/tls.key", "tls.key"),
            ("ca.crt", "ca.crt"),
        ] {
            private_file_copy(&certs.join(from), &mtls.join(to))?;
        }
        let network = tools.output(
            &tools.podman,
            &["network", "exists", GATEWAY],
            "Podman network lookup failed",
        )?;
        if network.status.code() == Some(1) {
            tools.podman(
                &["network", "create", GATEWAY],
                "Podman network creation failed",
            )?;
        } else if network.status.success() {
            return Err(Error::new(
                "Podman network dungeon-local already exists without a Dungeon gateway; inspect and resolve explicitly",
            ));
        } else {
            return Err(Error::new("Podman network lookup failed"));
        }
        let args = container_arguments(&state, &socket);
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        tools.podman(&refs, "gateway creation failed")?;
        tools.openshell(
            &["gateway", "add", ENDPOINT, "--local", "--name", GATEWAY],
            "gateway registration failed",
        )?;
        if let Some(previous) = previous_gateway.filter(|name| name != GATEWAY) {
            tools.openshell(
                &["gateway", "select", &previous],
                "could not restore previously active OpenShell gateway",
            )?;
        }
    }
    let mut failure = None;
    for _ in 0..10 {
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
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(Error::from))
            .and_then(|info| gateway_health(&info))
        {
            Ok(()) => {
                failure = None;
                break;
            }
            Err(error) => failure = Some(error),
        }
        thread::sleep(Duration::from_secs(1));
    }
    if let Some(error) = failure {
        return Err(Error::new(format!("gateway health failed: {error}")));
    }
    println!(
        "{}: {GATEWAY} healthy; runtime rootless Podman {}; workspace default (OpenShell {OPEN_SHELL_VERSION}).",
        if existing {
            "Already configured"
        } else {
            "Local containment ready"
        },
        tools.podman_version
    );
    if tools
        .output(
            &tools.podman,
            &["image", "exists", AGENT_IMAGE],
            "agent image lookup failed",
        )?
        .status
        .success()
    {
        println!(
            "Reviewed local development image found: {AGENT_IMAGE} (mutable local tag, not published)."
        );
    } else {
        println!(
            "Reviewed local development image not found. Build from the repository root:\n  podman build --pull=never -t {AGENT_IMAGE} -f images/agent/Containerfile images/agent"
        );
    }
    println!(
        "Next: dungeon init --directory \"$HOME/.config/dungeon/demo\" --project \"$PWD\" --gateway {GATEWAY} --image {AGENT_IMAGE}"
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_path_is_exact_and_never_selects_vm_or_sudo() {
        assert!(require_linux("linux").is_ok());
        assert!(require_linux("macos").is_err());
        assert!(require_version("openshell 0.0.116\n").is_ok());
        assert!(require_version("openshell 0.0.117").is_err());
        let args = container_arguments(
            Path::new("/tmp/dungeon"),
            "/run/user/1000/podman/podman.sock",
        );
        assert!(args.windows(2).any(|p| p == ["--network", "dungeon-local"]));
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
