//! Read-only diagnostics inside the single OpenShell subprocess boundary.
use crate::Result;
use crate::config::Config;
use serde::Serialize;

const GUIDE: &str = "https://github.com/Dark-Roast-Cyber/dungeon/blob/main/docs/configuration.md#read-only-diagnostics";

#[derive(Debug, Serialize)]
struct Finding {
    check: &'static str,
    state: &'static str,
    blocking: bool,
    detail: &'static str,
    remediation: &'static str,
    reference: &'static str,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: u32,
    supported_openshell: &'static str,
    read_only: bool,
    containment_verified: bool,
    exit_code: i32,
    checks: Vec<Finding>,
}

impl Report {
    fn new() -> Self {
        Self {
            schema: 1,
            supported_openshell: crate::OPEN_SHELL_VERSION,
            read_only: true,
            containment_verified: false,
            exit_code: 0,
            checks: Vec::new(),
        }
    }

    fn add(
        &mut self,
        check: &'static str,
        state: &'static str,
        blocking: bool,
        detail: &'static str,
        remediation: &'static str,
    ) {
        if blocking {
            self.exit_code = 1;
        }
        self.checks.push(Finding {
            check,
            state,
            blocking,
            detail,
            remediation,
            reference: GUIDE,
        });
    }
}

pub(crate) fn run(explicit: Option<std::path::PathBuf>, json: bool) -> Result<i32> {
    #[cfg(target_os = "linux")]
    let selected = linux::select(explicit);
    #[cfg(not(target_os = "linux"))]
    let selected: Result<Option<Config>> = {
        let _ = explicit;
        Err(crate::Error::new(
            "Private diagnostic file inspection requires Linux",
        ))
    };
    let mut report = Report::new();
    let config = match selected {
        Ok(Some(config)) => {
            report.add(
                "configuration",
                "healthy",
                false,
                "Selected private configuration and policy passed local validation.",
                "Review dungeon plan before runtime operations.",
            );
            if !config.values.imports.is_empty() {
                report.add("imports", "blocked", true, "Configuration imports are plan-only; runtime refuses them.", "Remove the declaration explicitly for runtime use; see Issue #5. No import was executed.");
            }
            Some(config)
        }
        Ok(None) => {
            report.add("configuration", "missing", false, "No existing private binding selected; machine prerequisites only.", "Use --config PATH to diagnose an explicit existing environment. Doctor never initializes one.");
            None
        }
        Err(_) => {
            report.add("configuration", "incompatible", true, "Selected configuration or private binding failed validation.", "Inspect the private dungeon.json and adjacent policy.json explicitly; doctor suppresses untrusted error contents.");
            None
        }
    };
    #[cfg(target_os = "linux")]
    linux::collect(&mut report, config.as_ref());
    #[cfg(not(target_os = "linux"))]
    {
        let _ = config;
        report.add(
            "platform",
            "unsupported",
            true,
            "Doctor's runtime boundary is Linux/rootless Podman only; no runtime was invoked.",
            "Use the recorded Linux/OpenShell 0.0.116 combination; there is no weaker fallback.",
        );
    }
    report.add("integration_gates", "recorded-not-verified", false, "Claude account, matched native clients, nested Codex sandbox and editor gates are recorded evidence, not dynamically verified.", "Consult docs/validation.md and Issues #11, #29 and #30. Doctor performs no authentication or AI tests.");
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Dungeon doctor (read-only; OpenShell {}; not containment attestation)",
            report.supported_openshell
        );
        for finding in &report.checks {
            println!(
                "{}: {}{} — {}\n  {}\n  {}",
                finding.check,
                finding.state,
                if finding.blocking { " (blocking)" } else { "" },
                finding.detail,
                finding.remediation,
                finding.reference
            );
        }
    }
    Ok(report.exit_code)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::super::{host_environment, local_setup as setup};
    use super::*;
    use crate::config::ProjectTransfer;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(5);

    pub(super) fn select(explicit: Option<PathBuf>) -> Result<Option<Config>> {
        let project = if explicit.is_none() {
            Some(crate::config::canonical_project(Path::new("."))?)
        } else {
            None
        };
        let path = match explicit {
            Some(path) => path,
            None => {
                let path =
                    crate::binding::config_path(project.as_deref().expect("binding project"))?;
                if !path.parent().expect("config directory").exists() {
                    return Ok(None);
                }
                path
            }
        };
        let config = private_config(&path)?;
        if project
            .as_ref()
            .is_some_and(|p| *p != config.values.project)
        {
            return Err(crate::Error::new("private binding project mismatch"));
        }
        Ok(Some(config))
    }

    fn private_config(path: &Path) -> Result<Config> {
        use std::os::unix::fs::OpenOptionsExt;
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        let parent = absolute
            .parent()
            .ok_or_else(|| crate::Error::new("config directory required"))?
            .canonicalize()?;
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&parent)?;
        private_metadata(&directory, true)?;
        let name = absolute
            .file_name()
            .ok_or_else(|| crate::Error::new("config filename required"))?;
        let path = parent.join(name);
        let config = private_json(&directory, name)?;
        crate::config::load_config_data(path, config, |policy_path| {
            let policy = private_json(&directory, std::ffi::OsStr::new("policy.json"))?;
            Ok((policy_path.to_owned(), policy))
        })
    }

    fn private_metadata(file: &fs::File, directory: bool) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata()?;
        if m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || if directory {
                !m.is_dir()
            } else {
                !m.is_file() || m.nlink() != 1 || m.len() > 1_048_576
            }
        {
            return Err(crate::Error::new(
                "diagnostic input must be private user-owned ordinary config/policy files",
            ));
        }
        Ok(())
    }

    fn private_json(directory: &fs::File, name: &std::ffi::OsStr) -> Result<Value> {
        use std::os::fd::FromRawFd;
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(name.as_bytes())
            .map_err(|_| crate::Error::new("invalid filename"))?;
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
        private_metadata(&file, false)?;
        let mut bytes = Vec::new();
        file.take(1_048_577).read_to_end(&mut bytes)?;
        if bytes.len() > 1_048_576 {
            return Err(crate::Error::new("configuration exceeds limit"));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| crate::Error::new("configuration must be UTF-8"))?;
        crate::config::read_json_text(&text)
    }
    // Deliberately excludes environment, certificates and credentials.
    const INSPECT: &str = r#"{"label":{{json (index .Config.Labels "agentarchaeology.ai/dungeon-gateway")}},"running":{{json .State.Running}},"started_ns":{{.State.StartedAt.UnixNano}},"image":{{json .Image}},"networks":{{json .NetworkSettings.Networks}},"ports":{{json .HostConfig.PortBindings}},"mounts":{{json .Mounts}},"config_path":{{range .Config.Env}}{{if eq . "OPENSHELL_GATEWAY_CONFIG=/etc/openshell/gateway.toml"}}true{{end}}{{end}},"podman_driver":{{range .Config.Env}}{{if eq . "OPENSHELL_DRIVERS=podman"}}true{{end}}{{end}},"driver_data":[{{$comma := ""}}{{range .Config.Env}}{{if and (ge (len .) 14) (eq (slice . 0 14) "XDG_DATA_HOME=")}}{{$comma}}{{eq . DUNGEON_DRIVER_DATA_ASSIGNMENT}}{{$comma = ","}}{{end}}{{end}}]}"#;

    use setup::{BindConfig, Reply, parse_bind_config, query};

    struct Queries {
        binary: PathBuf,
        env: HashMap<OsString, OsString>,
    }
    impl Queries {
        fn get(&self, args: &[&str]) -> Option<Reply> {
            query(&self.binary, &self.env, args, TIMEOUT)
        }
        fn json(&self, args: &[&str]) -> Option<Value> {
            let reply = self.get(args)?;
            (reply.code == 0)
                .then(|| serde_json::from_str(&reply.text).ok())
                .flatten()
        }
        fn exists(&self, report: &mut Report, check: &'static str, args: &[&str]) -> bool {
            let (state, found) = match self.get(args).map(|r| r.code) {
                Some(0) => ("healthy", true),
                Some(1) => ("missing", false),
                _ => ("unknown", false),
            };
            report.add(check, state, !found, "Read-only local runtime inventory lookup.", "Inspect this named resource explicitly with Podman; doctor does not pull images or create resources.");
            found
        }
    }

    fn tools(name: &str, env: &HashMap<OsString, OsString>, excluded: &[&Path]) -> Option<Queries> {
        let binary = setup::executable(name, env, excluded.first().copied()).ok()?;
        if excluded.iter().any(|p| binary.starts_with(p)) {
            return None;
        }
        Some(Queries {
            binary,
            env: env.clone(),
        })
    }

    pub(super) fn collect(report: &mut Report, config: Option<&Config>) {
        report.add(
            "platform",
            "healthy",
            false,
            "Linux diagnostics; supported OpenShell version is 0.0.116, rootless Podman only.",
            "Other runtime combinations require separate validation.",
        );
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.canonicalize().ok());
        let mut excluded = Vec::new();
        if let Some(ref cwd) = cwd {
            excluded.push(cwd.as_path());
        }
        if let Some(config) = config {
            excluded.push(config.values.project.as_path());
        }
        let Ok(mut env) = host_environment(cwd.as_deref()) else {
            report.add(
                "environment",
                "incompatible",
                true,
                "Cannot construct sanitized host environment.",
                "Use an absolute PATH outside project directories.",
            );
            return;
        };
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
        inventory(report, config, &env, &excluded);
    }

    fn inventory(
        report: &mut Report,
        config: Option<&Config>,
        env: &HashMap<OsString, OsString>,
        excluded: &[&Path],
    ) {
        let mut env = env.clone();
        let paths: Vec<_> = env
            .get(&OsString::from("PATH"))
            .into_iter()
            .flat_map(std::env::split_paths)
            .filter(|p| p.is_absolute())
            .filter_map(|p| p.canonicalize().ok())
            .filter(|p| p.is_dir() && !excluded.iter().any(|project| p.starts_with(project)))
            .collect();
        env.insert(
            OsString::from("PATH"),
            std::env::join_paths(paths).unwrap_or_default(),
        );
        let env = &env;
        if [
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
        ]
        .iter()
        .any(|key| {
            env.get(&OsString::from(key))
                .is_some_and(|p| !safe_root(Path::new(p), excluded))
        }) {
            report.add(
                "environment",
                "incompatible",
                true,
                "Host HOME/XDG roots must be absolute, normalized and outside projects.",
                "Select private host roots explicitly; no runtime queries were invoked.",
            );
            return;
        }
        let podman = tools("podman", env, excluded);
        let openshell = tools("openshell", env, excluded);
        let shell_ok = if let Some(ref shell) = openshell {
            let reply = shell.get(&["--version"]);
            let valid = reply
                .as_ref()
                .is_some_and(|r| r.code == 0 && setup::require_version(&r.text).is_ok());
            report.add(
                "openshell",
                if valid { "healthy" } else { "incompatible" },
                !valid,
                "Only exact OpenShell 0.0.116 version output is accepted.",
                "Review and select the pinned OpenShell CLI explicitly; doctor installs nothing.",
            );
            valid
        } else {
            report.add("openshell", "missing", true, "No trusted OpenShell executable found outside the project.", "Make the reviewed OpenShell 0.0.116 CLI available on an absolute non-project PATH.");
            false
        };
        let podman_found = podman.is_some();
        if let Some(ref podman) = podman {
            let version_ok = podman
                .get(&["--version"])
                .is_some_and(|r| r.code == 0 && setup::valid_podman_version(&r.text));
            report.add(
                "podman_version",
                if version_ok { "healthy" } else { "unknown" },
                !version_ok,
                "Podman version query validated; version output is not echoed.",
                "Check podman --version explicitly; runtime validation is combination-specific.",
            );
        }
        let storage_ready = setup::storage_ready(env, excluded);
        report.add("podman_storage", if storage_ready { "healthy" } else { "unknown" }, !storage_ready, "Only established default rootless storage with a user-owned regular db.sql marker and no custom user storage.conf is queried.", "Missing, empty, custom or unfamiliar storage is unknown and never initialized/adopted; inspect rootless Podman explicitly. Doctor never reads engine database contents.");
        let podman = podman.filter(|_| storage_ready);
        let uid = unsafe { libc::geteuid() };
        let socket = format!("/run/user/{uid}/podman/podman.sock");
        let socket_found = setup::socket_ready(Path::new(&socket));
        let api = podman.as_ref().filter(|_| socket_found).and_then(|p| {
            p.get(&[
                "--url",
                &format!("unix://{socket}"),
                "info",
                "--format",
                "{{.Host.Security.Rootless}}",
            ])
        });
        let socket_ok = uid != 0 && api.is_some_and(|r| r.code == 0 && r.text.trim() == "true");
        report.add("podman_socket", if socket_ok { "healthy" } else if !socket_found { "missing" } else { "unknown" }, !socket_ok, "Rootless API socket existence and bounded read-only API readiness query.", "As the normal user, inspect podman info and the user podman.socket service; enable/start that service explicitly only if approved.");
        let state = env
            .get(&OsString::from("XDG_STATE_HOME"))
            .map(PathBuf::from)
            .or_else(|| {
                env.get(&OsString::from("HOME"))
                    .map(|h| PathBuf::from(h).join(".local/state"))
            })
            .map(|p| p.join("dungeon/local-setup"));
        let expected_data = state
            .as_ref()
            .map(|p| format!("XDG_DATA_HOME={}", p.join("driver-data").display()))
            .unwrap_or_default();
        let inspect_template = INSPECT.replace(
            "DUNGEON_DRIVER_DATA_ASSIGNMENT",
            &serde_json::to_string(&expected_data).expect("string serialization"),
        );
        let mut container = None;
        let mut image_id = None;
        if let Some(ref podman) = podman {
            let info = podman.json(&["info", "--format", "json"]);
            let rootless = uid != 0 && info.as_ref().is_some_and(|v| setup::rootless(v).is_ok());
            report.add(
                "podman",
                if rootless { "healthy" } else { "incompatible" },
                !rootless,
                "Rootless runtime info query; no rootful fallback.",
                "Run Podman as your normal non-root user and inspect podman info explicitly.",
            );
            podman.exists(report, "gateway_image", &["image", "exists", setup::IMAGE]);
            let image = config
                .map(|c| c.values.image.as_str())
                .unwrap_or(setup::AGENT_IMAGE);
            podman.exists(report, "agent_image", &["image", "exists", image]);
            let present = podman.exists(
                report,
                "gateway_container",
                &["container", "exists", setup::CONTAINER],
            );
            let network_present = podman.exists(
                report,
                "gateway_network",
                &["network", "exists", setup::GATEWAY],
            );
            if network_present {
                let driver = podman.get(&[
                    "network",
                    "inspect",
                    setup::GATEWAY,
                    "--format",
                    "{{.Driver}}",
                ]);
                let bridge = driver
                    .as_ref()
                    .is_some_and(|r| r.code == 0 && r.text.trim() == "bridge");
                report.add("gateway_network_driver", if bridge { "healthy" } else if driver.is_some() { "incompatible" } else { "unknown" }, !bridge, "The local setup network must use Podman's bridge driver; existence alone is not compatibility.", "Inspect the named Podman network explicitly; doctor never replaces it.");
            }
            if present {
                image_id = podman
                    .get(&["image", "inspect", setup::IMAGE, "--format", "{{.Id}}"])
                    .filter(|r| r.code == 0 && !r.text.trim().is_empty())
                    .map(|r| r.text.trim().to_owned());
                container = podman.json(&[
                    "container",
                    "inspect",
                    setup::CONTAINER,
                    "--format",
                    &inspect_template,
                ]);
                let c = container.as_ref();
                let recognized = c.is_some_and(|c| {
                    image_id
                        .as_ref()
                        .is_some_and(|id| c["image"] == id.as_str())
                        && c["label"] == setup::GATEWAY
                        && c["config_path"] == true
                        && c["podman_driver"] == true
                        && !c["networks"][setup::GATEWAY].is_null()
                        && c["ports"]["8080/tcp"][0]["HostIp"] == "127.0.0.1"
                        && c["ports"]["8080/tcp"][0]["HostPort"] == "17670"
                });
                let running = c.is_some_and(|c| c["running"] == true);
                let state = if c.is_none() {
                    "unknown"
                } else if !recognized {
                    "incompatible"
                } else if !running {
                    "partial"
                } else {
                    "healthy"
                };
                report.add("gateway_container_health", state, !recognized || !running, "Recognized local pinned image identity, label, configuration path, driver, network, loopback port and running state only; not containment attestation.", "Inspect conflicting or stopped container state explicitly with Podman; doctor neither adopts nor starts it.");
            }
        } else {
            report.add("podman", if podman_found { "unknown" } else { "missing" }, true, "Trusted Podman or existing default storage unavailable; inventory queries skipped to avoid initialization.", "Make rootless Podman available explicitly and inspect its storage; doctor does not install or initialize it.");
            for check in [
                "gateway_image",
                "agent_image",
                "gateway_container",
                "gateway_network",
            ] {
                report.add(
                    check,
                    "unknown",
                    true,
                    "Podman unavailable; this independent inventory check cannot run.",
                    "Resolve rootless Podman, then repeat doctor.",
                );
            }
        }
        let gateway = config
            .map(|c| c.values.gateway.as_str())
            .unwrap_or(setup::GATEWAY);
        let local = gateway == setup::GATEWAY;
        let mut registered = false;
        let mut healthy = false;
        if shell_ok {
            let shell = openshell.as_ref().expect("version checked");
            let registry = env
                .get(&OsString::from("XDG_CONFIG_HOME"))
                .map(PathBuf::from)
                .or_else(|| {
                    env.get(&OsString::from("HOME"))
                        .map(|h| PathBuf::from(h).join(".config"))
                })
                .map(|p| p.join("openshell/gateways"));
            let list = if registry
                .as_ref()
                .is_some_and(|p| p.is_dir() && safe_root(p, excluded))
            {
                shell.json(&["gateway", "list", "--output", "json"])
            } else {
                Some(serde_json::json!([]))
            };
            let record = list
                .as_ref()
                .and_then(Value::as_array)
                .and_then(|v| v.iter().find(|r| r["name"] == gateway));
            registered = record.is_some();
            let compatible =
                local && record.is_some_and(|r| setup::compatible_registration(r).is_ok());
            let state = if list.as_ref().and_then(Value::as_array).is_none() {
                "unknown"
            } else if !registered {
                "missing"
            } else if !local {
                "unsupported"
            } else if compatible {
                "healthy"
            } else {
                "incompatible"
            };
            report.add("gateway_registration", state, local && !compatible, "Selected registration inspected without adopting or selecting it. Foreign/remote settings remain unknown.", "Inspect openshell gateway list explicitly; resolve conflicts manually. Dungeon never adds/selects/migrates a gateway in doctor.");
            if compatible {
                let workspace = config
                    .map(|c| c.values.workspace.as_str())
                    .unwrap_or("default");
                healthy = shell
                    .json(&[
                        "--gateway",
                        gateway,
                        "--workspace",
                        workspace,
                        "gateway",
                        "info",
                        "--output",
                        "json",
                    ])
                    .as_ref()
                    .is_some_and(|v| setup::gateway_health(v).is_ok());
            }
        } else {
            report.add("gateway_registration", "unknown", true, "Supported OpenShell unavailable; registration query skipped, other checks retained.", "Resolve the pinned CLI prerequisite and repeat doctor.");
        }
        report.add("gateway_health", if !local { "unknown" } else if healthy { "healthy" } else { "partial" }, local && !healthy, "Gateway health/version/Podman-driver query is bounded and is not containment verification.", "Inspect the selected gateway explicitly; unfamiliar remote gateways are not inferred from local state.");
        if local && registered != container.is_some() {
            report.add(
                "gateway_consistency",
                "partial",
                true,
                "Registration and inspectable local container disagree.",
                "Inspect partial local state explicitly; do not overwrite or rerun setup blindly.",
            );
        }
        let bind = if local {
            local_bind(
                state.as_deref(),
                container.as_ref(),
                image_id.as_deref(),
                excluded,
            )
        } else {
            None
        };
        let mapping = bind.map(|b| b.mapping);
        let live = config.is_some_and(|c| c.values.project_transfer == ProjectTransfer::Live);
        let compatible =
            local && bind.is_some_and(|b| b.enabled && b.mapping) && healthy && socket_ok;
        report.add("keep_id_mapping", if !local || mapping.is_none() { "unknown" } else if mapping == Some(true) { "healthy" } else { "incompatible" }, live && mapping != Some(true), "Live requires Podman driver userns=keep-id:uid=998,gid=998; the gateway container's own user namespace is not this setting.", "Review the dedicated live gateway recipe in docs/live-validation.md. Doctor never changes user namespaces.");
        report.add("live_bind", bind_state(live, compatible, bind.is_some_and(|b| !b.enabled), healthy), live && !compatible, "Recognized local gateway configuration observation only; no bind, writeability or containment test is performed.", "Copy remains the generated default. Live needs explicit reviewed gateway capability and mapping; remote settings are unknown, not inferred. No automatic migration.");
    }

    fn safe_root(path: &Path, excluded: &[&Path]) -> bool {
        use std::path::Component;
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        {
            return false;
        }
        let Some(parent) = path.ancestors().find(|p| p.exists()) else {
            return false;
        };
        let Ok(canonical) = parent.canonicalize() else {
            return false;
        };
        let resolved = canonical.join(path.strip_prefix(parent).expect("ancestor"));
        !excluded.iter().any(|p| resolved.starts_with(p))
    }

    fn bind_state(live: bool, compatible: bool, disabled: bool, healthy: bool) -> &'static str {
        if live && !compatible {
            "requested-but-unavailable"
        } else if compatible {
            "enabled-compatible"
        } else if disabled && healthy {
            "disabled-but-healthy"
        } else {
            "unknown-incompatible"
        }
    }

    fn local_bind(
        state: Option<&Path>,
        container: Option<&Value>,
        image_id: Option<&str>,
        excluded: &[&Path],
    ) -> Option<BindConfig> {
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let state = state?.canonicalize().ok()?;
        if !state.is_absolute() || excluded.iter().any(|p| state.starts_with(p)) {
            return None;
        }
        let directory = fs::metadata(&state).ok()?;
        if directory.uid() != unsafe { libc::geteuid() } || directory.mode() & 0o077 != 0 {
            return None;
        }
        let path = state.join("gateway.toml");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
            .ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
            || metadata.len() > 8192
        {
            return None;
        }
        let mut text = String::new();
        file.by_ref().take(8193).read_to_string(&mut text).ok()?;
        let observed = parse_bind_config(&text)?;
        let c = container?;
        let mounts = c["mounts"].as_array()?;
        if c["image"] != image_id?
            || c["label"] != setup::GATEWAY
            || c["running"] != true
            || c["config_path"] != true
            || c["podman_driver"] != true
            || c["driver_data"] != serde_json::json!([true])
            || c["started_ns"].as_i64().is_none_or(|start| {
                start <= 0
                    || i128::from(start)
                        < i128::from(metadata.mtime()) * 1_000_000_000
                            + i128::from(metadata.mtime_nsec())
            })
            || c["networks"][setup::GATEWAY].is_null()
            || c["ports"]["8080/tcp"][0]["HostIp"] != "127.0.0.1"
            || c["ports"]["8080/tcp"][0]["HostPort"] != "17670"
            || !mounts.iter().any(|m| {
                m["Source"] == path.to_string_lossy().as_ref()
                    && m["Destination"] == "/etc/openshell/gateway.toml"
                    && m["RW"] == false
            })
            || (observed.enabled
                && !mounts.iter().any(|m| {
                    let driver_data = state.join("driver-data");
                    m["Source"] == driver_data.to_string_lossy().as_ref()
                        && m["Destination"] == driver_data.to_string_lossy().as_ref()
                        && m["RW"] == true
                }))
        {
            return None;
        }
        Some(observed)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use setup::{collect_query, valid_podman_version};
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        use std::time::Instant;

        fn script(path: &Path, text: &str) {
            fs::write(path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }

        fn fixture_env(root: &Path) -> HashMap<OsString, OsString> {
            HashMap::from([
                (OsString::from("PATH"), root.as_os_str().to_owned()),
                (OsString::from("HOME"), root.join("home").into_os_string()),
            ])
        }

        #[test]
        fn diagnostic_config_and_policy_use_private_nonblocking_nofollow_descriptors() {
            use crate::config::{Agent, ConfigDoc, write_new};
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("private");
            let project = root.path().join("project");
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            fs::create_dir(&project).unwrap();
            let path = directory.join("dungeon.json");
            let policy = directory.join("policy.json");
            write_new(
                &path,
                &ConfigDoc {
                    schema: 2,
                    name: "fixture".into(),
                    gateway: "dungeon-local".into(),
                    workspace: "default".into(),
                    project,
                    project_transfer: ProjectTransfer::Copy,
                    image: "localhost/fixture:1".into(),
                    agent: Agent::Opencode,
                    providers: vec![],
                    imports: vec![],
                },
            )
            .unwrap();
            write_new(&policy, &crate::policy::starter_policy()).unwrap();
            assert_eq!(
                private_config(&path).unwrap().fingerprint,
                crate::config::load_config(&path).unwrap().fingerprint
            );
            for target in [&path, &policy] {
                let backup = directory.join("backup");
                fs::rename(target, &backup).unwrap();
                symlink(&backup, target).unwrap();
                assert!(private_config(&path).is_err());
                fs::remove_file(target).unwrap();
                let name = std::ffi::CString::new(target.as_os_str().as_encoded_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                let start = Instant::now();
                assert!(private_config(&path).is_err());
                assert!(start.elapsed() < Duration::from_millis(500));
                fs::remove_file(target).unwrap();
                fs::rename(&backup, target).unwrap();
                fs::set_permissions(target, fs::Permissions::from_mode(0o644)).unwrap();
                assert!(private_config(&path).is_err());
                fs::set_permissions(target, fs::Permissions::from_mode(0o600)).unwrap();
            }
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(private_config(&path).is_err());
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(private_config(&path).is_ok());
            // Both file reads remain anchored to the checked directory descriptor.
            let held = fs::File::open(&directory).unwrap();
            fs::rename(&directory, root.path().join("old-private")).unwrap();
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("policy.json"), "malicious replacement").unwrap();
            assert_eq!(
                private_json(&held, std::ffi::OsStr::new("policy.json")).unwrap(),
                crate::policy::starter_policy()
            );
        }

        #[test]
        fn fresh_fixture_queries_under_parallel_fork_activity() {
            use std::os::unix::process::CommandExt;
            let workers = (0..8)
                .map(|_| {
                    std::thread::spawn(|| {
                        for _ in 0..20 {
                            let root = tempfile::tempdir().unwrap();
                            let path = root.path().join("query");
                            script(&path, "#!/bin/sh\nprintf 'fixture\\n'\n");
                            let reply = query(&path, &HashMap::new(), &[], Duration::from_secs(1));
                            assert!(reply.is_some(), "fresh fixture query failed");
                        }
                    })
                })
                .collect::<Vec<_>>();
            for _ in 0..20 {
                let mut command = Command::new("/bin/true");
                unsafe {
                    command.pre_exec(|| {
                        libc::usleep(5000);
                        Ok(())
                    });
                }
                assert!(command.status().unwrap().success());
            }
            for worker in workers {
                worker.join().unwrap();
            }
        }

        #[test]
        fn helper_path_excludes_the_selected_project_and_symlink_directories() {
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            let selected = root.path().join("selected");
            let cwd = root.path().join("cwd");
            let tools_dir = root.path().join("tools");
            for dir in [&selected, &cwd, &tools_dir, &root.path().join("home")] {
                fs::create_dir(dir).unwrap();
            }
            let marker = root.path().join("helper-executed");
            script(
                &selected.join("project-helper"),
                &format!("#!/bin/sh\nprintf invoked > '{}'\n", marker.display()),
            );
            script(
                &tools_dir.join("podman"),
                "#!/bin/sh\nproject-helper\nprintf 'podman version 5.8.4\\n'\n",
            );
            symlink(&selected, root.path().join("project-alias")).unwrap();
            let mut env = fixture_env(root.path());
            env.insert(
                OsString::from("PATH"),
                std::env::join_paths([
                    tools_dir,
                    selected.clone(),
                    root.path().join("project-alias"),
                ])
                .unwrap(),
            );
            let mut report = Report::new();
            inventory(&mut report, None, &env, &[&cwd, &selected]);
            assert!(!marker.exists());
            assert_eq!(
                report
                    .checks
                    .iter()
                    .find(|f| f.check == "podman_version")
                    .unwrap()
                    .state,
                "healthy"
            );
        }

        #[test]
        fn exited_query_leader_retains_identity_until_pipe_descendant_cleanup() {
            let root = tempfile::tempdir().unwrap();
            let binary = root.path().join("query");
            let pid_file = root.path().join("pid");
            script(
                &binary,
                &format!(
                    "#!/bin/sh\n/bin/sleep 30 &\nprintf '%s' $! > '{}'\nexit 0\n",
                    pid_file.display()
                ),
            );
            let mut child = Command::new(&binary)
                .current_dir("/")
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .unwrap();
            // Leader exit is the readiness handshake: the PID write completed,
            // and WNOWAIT keeps its identity reserved for the collector.
            let readiness = Instant::now();
            loop {
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        child.id(),
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result == 0 && unsafe { info.si_pid() } != 0 {
                    break;
                }
                if result != 0 || readiness.elapsed() >= Duration::from_secs(5) {
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.wait();
                    panic!("query fixture failed its leader-exit readiness handshake");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let pid = fs::read_to_string(pid_file).unwrap();
            let start = Instant::now();
            assert!(collect_query(child, Duration::from_millis(100)).is_none());
            assert!(start.elapsed() < Duration::from_secs(2));
            for _ in 0..100 {
                let stat = fs::read_to_string(format!("/proc/{pid}/stat"));
                if stat.as_ref().is_err()
                    || stat
                        .as_ref()
                        .is_ok_and(|s| s.rsplit_once(") ").unwrap().1.starts_with("Z "))
                {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("query descendant remained running after bounded cleanup");
        }

        #[test]
        fn independent_inventory_queries_are_allowlisted_and_create_no_private_state() {
            let root = tempfile::tempdir().unwrap();
            let project = root.path().join("project");
            let home = root.path().join("home");
            fs::create_dir(&project).unwrap();
            fs::create_dir(&home).unwrap();
            fs::create_dir_all(home.join(".local/share/containers/storage")).unwrap();
            fs::write(
                home.join(".local/share/containers/storage/db.sql"),
                "synthetic established marker",
            )
            .unwrap();
            let mock = format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> '{}/calls'
case "$*" in
 '--version') printf 'podman version 5.8.4\n';;
 'info --format json') printf '{{"host":{{"security":{{"rootless":true}}}}}}';;
 'image exists '*|'container exists openshell-gateway-dungeon-local'|'network exists dungeon-local') exit 1;;
 '--url unix://'*' info --format '* ) printf 'true';;
 *) exit 77;;
esac
"#,
                root.path().display()
            );
            script(&root.path().join("podman"), &mock);
            let env = fixture_env(root.path());
            let mut report = Report::new();
            inventory(&mut report, None, &env, &[&project]);
            assert_eq!(
                report
                    .checks
                    .iter()
                    .find(|f| f.check == "openshell")
                    .unwrap()
                    .state,
                "missing"
            );
            for check in [
                "gateway_image",
                "agent_image",
                "gateway_container",
                "gateway_network",
            ] {
                assert_eq!(
                    report
                        .checks
                        .iter()
                        .find(|f| f.check == check)
                        .unwrap()
                        .state,
                    "missing"
                );
            }
            let calls = fs::read_to_string(root.path().join("calls")).unwrap();
            assert!(calls.contains("info --format json\n"));
            assert!(calls.contains("container exists openshell-gateway-dungeon-local\n"));
            assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
            assert!(!home.join(".config").exists());
            assert_eq!(
                fs::read_to_string(home.join(".local/share/containers/storage/db.sql")).unwrap(),
                "synthetic established marker"
            );
            assert_eq!(report.exit_code, 1);
            let mut second = Report::new();
            inventory(&mut second, None, &env, &[&project]);
            assert_eq!(
                serde_json::to_value(&report).unwrap(),
                serde_json::to_value(&second).unwrap()
            );
            // Project-owned tools cannot be used even through an absolute PATH.
            let env = HashMap::from([(OsString::from("PATH"), root.path().into())]);
            assert!(tools("podman", &env, &[root.path()]).is_none());
        }

        #[test]
        fn foreign_registration_is_not_adopted_or_queried_and_output_is_sanitized() {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir_all(root.path().join("home/.config/openshell/gateways")).unwrap();
            script(
                &root.path().join("openshell"),
                &format!(
                    r#"#!/bin/sh
printf '%s\n' "$*" >> '{}/calls'
case "$*" in
 '--version') printf 'openshell 0.0.116\n';;
 'gateway list --output json') printf '[{{"name":"dungeon-local","endpoint":"secret-token","auth":"plaintext"}}]';;
 *) exit 77;;
esac
"#,
                    root.path().display()
                ),
            );
            let mut report = Report::new();
            inventory(&mut report, None, &fixture_env(root.path()), &[]);
            assert_eq!(
                report
                    .checks
                    .iter()
                    .find(|f| f.check == "gateway_registration")
                    .unwrap()
                    .state,
                "incompatible"
            );
            assert!(
                !serde_json::to_string(&report)
                    .unwrap()
                    .contains("secret-token")
            );
            assert_eq!(
                fs::read_to_string(root.path().join("calls")).unwrap(),
                "--version\ngateway list --output json\n"
            );
            assert_eq!(
                fs::read_dir(root.path().join("home/.config/openshell/gateways"))
                    .unwrap()
                    .count(),
                0
            );
            script(
                &root.path().join("openshell"),
                "#!/bin/sh\nif [ \"$*\" = --version ]; then printf 'openshell 0.0.116\\n'; else printf 'malformed secret-token'; fi\n",
            );
            let mut malformed = Report::new();
            inventory(&mut malformed, None, &fixture_env(root.path()), &[]);
            assert_eq!(
                malformed
                    .checks
                    .iter()
                    .find(|f| f.check == "gateway_registration")
                    .unwrap()
                    .state,
                "unknown"
            );
            assert!(
                !serde_json::to_string(&malformed)
                    .unwrap()
                    .contains("secret-token")
            );
        }

        #[test]
        fn bind_and_mapping_are_separate_observations_and_unfamiliar_configs_are_unknown() {
            let root = tempfile::tempdir().unwrap();
            let state = root.path().join("state");
            fs::create_dir(&state).unwrap();
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
            let path = state.join("gateway.toml");
            let container = serde_json::json!({"label":setup::GATEWAY,"image":"image-id","running":true,"started_ns":i64::MAX,"config_path":true,"podman_driver":true,"driver_data":[true],"networks":{"dungeon-local":{}},"ports":{"8080/tcp":[{"HostIp":"127.0.0.1","HostPort":"17670"}]},"mounts":[{"Source":path,"Destination":"/etc/openshell/gateway.toml","RW":false},{"Source":state.join("driver-data"),"Destination":state.join("driver-data"),"RW":true}]});
            for enabled in [false, true] {
                for mapping in [false, true] {
                    let mut text = setup::CONFIG.replace(
                        "enable_bind_mounts = false",
                        if enabled {
                            "enable_bind_mounts = true"
                        } else {
                            "enable_bind_mounts = false"
                        },
                    );
                    if mapping {
                        text.push_str("userns = \"keep-id:uid=998,gid=998\"\n");
                    }
                    fs::write(&path, &text).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                    let observed =
                        local_bind(Some(&state), Some(&container), Some("image-id"), &[]).unwrap();
                    assert_eq!(observed.enabled, enabled);
                    assert_eq!(observed.mapping, mapping);
                    assert_eq!(fs::read_to_string(&path).unwrap(), text);
                }
            }
            for (field, value) in [
                ("image", serde_json::json!("foreign")),
                ("config_path", serde_json::json!(false)),
                ("started_ns", serde_json::json!(0)),
                ("running", serde_json::json!(false)),
                ("driver_data", serde_json::json!([])),
                ("driver_data", serde_json::json!([false])),
                ("driver_data", serde_json::json!([true, false])),
                ("driver_data", serde_json::json!([true, true])),
            ] {
                let mut conflict = container.clone();
                conflict[field] = value;
                assert!(local_bind(Some(&state), Some(&conflict), Some("image-id"), &[]).is_none());
            }
            assert!(
                local_bind(Some(&state), Some(&container), Some("image-id"), &[&state]).is_none()
            );
            for text in [
                "malformed secret-token",
                "[openshell.drivers.podman]\nenable_bind_mounts=true\n",
                "userns = \"keep-id\"\n",
            ] {
                assert!(parse_bind_config(text).is_none());
            }
            assert_eq!(bind_state(false, false, true, true), "disabled-but-healthy");
            assert_eq!(bind_state(false, true, false, true), "enabled-compatible");
            assert_eq!(
                bind_state(false, false, false, true),
                "unknown-incompatible"
            );
            assert_eq!(
                bind_state(true, false, true, true),
                "requested-but-unavailable"
            );
            assert!(safe_root(&state, &[]));
            assert!(!safe_root(Path::new("relative"), &[]));
            assert!(!safe_root(&state, &[root.path()]));
        }

        #[test]
        fn query_is_bounded_and_does_not_echo_untrusted_output() {
            let root = tempfile::tempdir().unwrap();
            let binary = root.path().join("fixture");
            fs::write(&binary, "#!/bin/sh\nprintf 'secret\\033[31m'; sleep 10\n").unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            let env = HashMap::from([(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))]);
            let start = Instant::now();
            assert!(query(&binary, &env, &["--version"], Duration::from_millis(50)).is_none());
            assert!(start.elapsed() < Duration::from_secs(2));
            fs::write(
                &binary,
                "#!/bin/sh\nwhile :; do printf '0123456789012345678901234567890123456789'; done\n",
            )
            .unwrap();
            assert!(query(&binary, &env, &[], Duration::from_secs(1)).is_none());
        }

        #[test]
        fn version_and_json_fail_closed_without_raw_contents() {
            assert!(valid_podman_version("podman version 5.8.4\n"));
            for text in [
                "podman version ",
                "podman version secret\u{1b}[31m",
                "garbage",
            ] {
                assert!(!valid_podman_version(text));
            }
            let mut report = Report::new();
            report.add(
                "test",
                "unknown",
                true,
                "Untrusted output suppressed.",
                "Inspect explicitly.",
            );
            let value = serde_json::to_value(&report).unwrap();
            assert_eq!(value["schema"], 1);
            assert_eq!(value["exit_code"], 1);
            assert_eq!(value["containment_verified"], false);
            assert_eq!(value["checks"][0]["blocking"], true);
            assert!(local_bind(None, None, None, &[]).is_none());
            let mut informational = Report::new();
            informational.add(
                "integration",
                "unsupported",
                false,
                "Unverified integration.",
                "See recorded gates.",
            );
            assert_eq!(informational.exit_code, 0);
        }

        #[test]
        fn absent_runtime_roots_are_not_initialized_and_unsafe_roots_are_not_queried() {
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("home");
            fs::create_dir(&home).unwrap();
            script(
                &root.path().join("podman"),
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}/calls'\n[ \"$*\" = --version ] || exit 77\nprintf 'podman version 5.8.4\\n'\n",
                    root.path().display()
                ),
            );
            script(
                &root.path().join("openshell"),
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}/calls'\n[ \"$*\" = --version ] || exit 77\nprintf 'openshell 0.0.116\\n'\n",
                    root.path().display()
                ),
            );
            let env = fixture_env(root.path());
            let mut report = Report::new();
            inventory(&mut report, None, &env, &[]);
            assert_eq!(
                fs::read_to_string(root.path().join("calls")).unwrap(),
                "--version\n--version\n"
            );
            assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
            fs::create_dir_all(home.join(".local/share/containers/storage")).unwrap();
            let mut empty_store_report = Report::new();
            inventory(&mut empty_store_report, None, &env, &[]);
            assert_eq!(
                empty_store_report
                    .checks
                    .iter()
                    .find(|f| f.check == "podman_storage")
                    .unwrap()
                    .state,
                "unknown"
            );
            assert_eq!(
                fs::read_dir(home.join(".local/share/containers/storage"))
                    .unwrap()
                    .count(),
                0
            );
            let mut unsafe_env = env;
            unsafe_env.insert(
                OsString::from("XDG_CONFIG_HOME"),
                OsString::from("relative-secret"),
            );
            let mut unsafe_report = Report::new();
            inventory(&mut unsafe_report, None, &unsafe_env, &[]);
            assert_eq!(unsafe_report.checks.len(), 1);
            assert_eq!(unsafe_report.checks[0].check, "environment");
            assert_eq!(
                fs::read_to_string(root.path().join("calls")).unwrap(),
                "--version\n--version\n--version\n--version\n"
            );
            fs::create_dir_all(home.join(".config/containers")).unwrap();
            fs::create_dir_all(home.join(".local/share/containers/storage")).unwrap();
            let custom = home.join(".config/containers/storage.conf");
            fs::write(&custom, "untrusted custom storage input").unwrap();
            let mut custom_report = Report::new();
            inventory(&mut custom_report, None, &fixture_env(root.path()), &[]);
            assert_eq!(
                custom_report
                    .checks
                    .iter()
                    .find(|f| f.check == "podman_storage")
                    .unwrap()
                    .state,
                "unknown"
            );
            assert_eq!(
                fs::read_to_string(&custom).unwrap(),
                "untrusted custom storage input"
            );
            assert_eq!(
                fs::read_to_string(root.path().join("calls")).unwrap(),
                "--version\n--version\n--version\n--version\n--version\n--version\n"
            );
        }

        #[test]
        fn remote_capabilities_are_unknown_and_a_live_request_is_blocking() {
            use crate::config::{Agent, ConfigDoc};
            let root = tempfile::tempdir().unwrap();
            let home = root.path().join("home");
            fs::create_dir_all(home.join(".config/openshell/gateways")).unwrap();
            script(
                &root.path().join("openshell"),
                &format!(
                    r#"#!/bin/sh
printf '%s\n' "$*" >> '{}/calls'
case "$*" in
 '--version') printf 'openshell 0.0.116\n';;
 'gateway list --output json') printf '[{{"name":"foreign","endpoint":"https://unknown.example","type":"remote","auth":"mtls","source":"user"}}]';;
 *) exit 77;;
esac
"#,
                    root.path().display()
                ),
            );
            let config = Config {
                path: home.join("private/dungeon.json"),
                values: ConfigDoc {
                    schema: 2,
                    name: "fixture".into(),
                    gateway: "foreign".into(),
                    workspace: "default".into(),
                    project: root.path().join("project"),
                    project_transfer: ProjectTransfer::Live,
                    image: "localhost/fixture:1".into(),
                    agent: Agent::Opencode,
                    providers: vec![],
                    imports: vec![],
                },
                policy: crate::policy::starter_policy(),
                policy_path: home.join("private/policy.json"),
                fingerprint: "fixture".into(),
            };
            let mut report = Report::new();
            inventory(
                &mut report,
                Some(&config),
                &fixture_env(root.path()),
                &[&config.values.project],
            );
            let registration = report
                .checks
                .iter()
                .find(|f| f.check == "gateway_registration")
                .unwrap();
            assert_eq!(registration.state, "unsupported");
            assert!(!registration.blocking);
            let live = report
                .checks
                .iter()
                .find(|f| f.check == "live_bind")
                .unwrap();
            assert_eq!(live.state, "requested-but-unavailable");
            assert!(live.blocking);
            assert_eq!(
                fs::read_to_string(root.path().join("calls")).unwrap(),
                "--version\ngateway list --output json\n"
            );
            assert!(!home.join("private").exists());
        }
    }
}
