//! Local native container lifecycle; no OpenShell policy is claimed or emulated.
use crate::config::{
    Config, ContainerBinding, ContainerOptions, Engine, RuntimeState, digest, read_json_text,
};
use crate::openshell::{GUEST_PROJECT, find_executable, host_environment, local_setup::query};
use crate::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const LABEL: &str = "agentarchaeology.ai/dungeon-config";
const RUNTIME_LABEL: &str = "agentarchaeology.ai/dungeon-runtime";

struct Native {
    binding: ContainerBinding,
    binary: PathBuf,
    env: HashMap<OsString, OsString>,
}

impl Native {
    fn prefix(&self) -> Vec<String> {
        match self.binding.engine {
            Engine::Podman => vec![
                "--remote".into(),
                "--url".into(),
                self.binding.endpoint.clone(),
            ],
            Engine::Docker => vec!["--host".into(), self.binding.endpoint.clone()],
            Engine::Auto => unreachable!(),
        }
    }
    fn query(&self, args: &[String], seconds: u64) -> Result<String> {
        let mut all = self.prefix();
        all.extend_from_slice(args);
        let args: Vec<_> = all.iter().map(String::as_str).collect();
        let reply = query(&self.binary, &self.env, &args, Duration::from_secs(seconds))
            .ok_or_else(|| Error::new("container service timed out or returned excessive output; inspect the selected service"))?;
        if reply.code != 0 {
            return Err(Error::new(format!(
                "{} {} failed; inspect the selected container service",
                self.binding.engine.name(),
                args.get(self.prefix().len()).unwrap_or(&"command")
            )));
        }
        Ok(reply.text)
    }
    fn command(&self, args: &[String]) -> Command {
        let mut c = Command::new(&self.binary);
        c.args(self.prefix())
            .args(args)
            .env_clear()
            .envs(&self.env)
            .current_dir("/");
        c
    }
    fn inventory(&self, filter: Option<&str>) -> Result<Vec<String>> {
        let mut args = strings(&["ps", "--all", "--no-trunc", "--format", "{{.ID}}"]);
        if let Some(filter) = filter {
            args.extend(["--filter".into(), filter.into()]);
        }
        let text = self.query(&args, 5)?;
        text.lines()
            .map(|s| checked_id(s).map(str::to_owned))
            .collect()
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
fn checked_id(id: &str) -> Result<&str> {
    if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(id)
    } else {
        Err(Error::new("invalid native container identity"))
    }
}

fn engine_environment(project: &Path) -> Result<HashMap<OsString, OsString>> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new("HOME is required for local container services"))?
        .canonicalize()?;
    let excluded: Vec<_> = [project, cwd.as_path()]
        .into_iter()
        .filter(|p| *p != home)
        .collect();
    let mut env = host_environment((project != home).then_some(project))?;
    env.retain(|key, _| {
        matches!(
            key.to_str(),
            Some(
                "PATH"
                    | "HOME"
                    | "USER"
                    | "LOGNAME"
                    | "XDG_CONFIG_HOME"
                    | "XDG_DATA_HOME"
                    | "XDG_STATE_HOME"
                    | "XDG_CACHE_HOME"
                    | "TERM"
                    | "COLORTERM"
                    | "LANG"
                    | "LC_ALL"
                    | "NO_COLOR"
            )
        )
    });
    let paths: Vec<_> = std::env::split_paths(env.get(OsStr::new("PATH")).unwrap())
        .filter(|p| !excluded.iter().any(|root| p.starts_with(root)))
        .collect();
    env.insert(
        "PATH".into(),
        std::env::join_paths(paths).map_err(|_| Error::new("invalid engine PATH"))?,
    );
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ] {
        if let Some(value) = env.get(OsStr::new(key)) {
            let path = Path::new(value);
            let ancestor = path
                .ancestors()
                .find(|p| p.exists())
                .ok_or_else(|| Error::new("invalid engine HOME/XDG path"))?;
            let resolved = ancestor
                .canonicalize()?
                .join(path.strip_prefix(ancestor).unwrap());
            if !path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                || excluded.iter().any(|root| resolved.starts_with(root))
            {
                return Err(Error::new(
                    "engine HOME/XDG roots must stay outside project directories",
                ));
            }
        }
    }
    Ok(env)
}

fn connect(engine: Engine, endpoint: &str, project: &Path) -> Result<Native> {
    crate::config::validate_endpoint(endpoint)?;
    let socket = Path::new(endpoint.strip_prefix("unix://").unwrap());
    if socket.starts_with(project) {
        return Err(Error::new("container socket must be outside the project"));
    }
    if socket.canonicalize().is_ok_and(|p| p.starts_with(project)) {
        return Err(Error::new("container socket must be outside the project"));
    }
    let meta = fs::metadata(socket).map_err(|_| {
        Error::new(format!(
            "{} service unavailable; start its local socket/service",
            engine.name()
        ))
    })?;
    if !meta.file_type().is_socket()
        || (engine == Engine::Podman && meta.uid() != unsafe { libc::geteuid() })
    {
        return Err(Error::new("unsupported container socket ownership or type"));
    }
    let env = engine_environment(project)?;
    let home = Path::new(env.get(OsStr::new("HOME")).unwrap()).canonicalize()?;
    let cwd = std::env::current_dir()?.canonicalize()?;
    let binary = find_executable(engine.name(), env.get(OsStr::new("PATH")))
        .filter(|p| {
            [project, cwd.as_path()]
                .into_iter()
                .all(|root| root == home || !p.starts_with(root))
        })
        .ok_or_else(|| Error::new(format!("install {} to use this environment", engine.name())))?;
    let mut native = Native {
        binding: ContainerBinding {
            engine,
            endpoint: endpoint.into(),
            daemon: String::new(),
        },
        binary,
        env,
    };
    let info = read_json_text(&native.query(&strings(&["info", "--format", "{{json .}}"]), 5)?)?;
    native.binding.daemon = service_identity(engine, &info)?;
    Ok(native)
}

fn service_identity(engine: Engine, info: &Value) -> Result<String> {
    Ok(match engine {
        Engine::Podman
            if info["host"]["security"]["rootless"] == true && info["host"]["os"] == "linux" =>
        {
            if !info["store"]["graphRoot"].is_string() || !info["host"]["idMappings"].is_object() {
                return Err(Error::new("unrecognized Podman service identity"));
            }
            digest(&json!([
                info["store"]["graphRoot"],
                info["host"]["hostname"],
                info["host"]["idMappings"]
            ]))?
        }
        Engine::Docker
            if info["OSType"] == "linux" && info["ID"].as_str().is_some_and(|s| !s.is_empty()) =>
        {
            let security = info["SecurityOptions"]
                .as_array()
                .ok_or_else(|| Error::new("unrecognized Docker service"))?;
            if info["Rootless"] == true
                || security.iter().any(|s| {
                    s.as_str()
                        .is_some_and(|s| s.contains("rootless") || s.contains("userns"))
                })
            {
                return Err(Error::new(
                    "Docker rootless/userns-remap requires separate UID mapping support; use rootless Podman or standard Docker CE",
                ));
            }
            info["ID"].as_str().unwrap().into()
        }
        _ => {
            return Err(Error::new(
                "use rootless Podman or a standard local Linux Docker CE service",
            ));
        }
    })
}

fn endpoints(engine: Engine, explicit: Option<&str>) -> Vec<String> {
    if let Some(endpoint) = explicit {
        return vec![endpoint.into()];
    }
    match engine {
        Engine::Podman => vec![format!("unix:///run/user/{}/podman/podman.sock", unsafe {
            libc::geteuid()
        })],
        Engine::Docker => vec!["unix:///var/run/docker.sock".into()],
        Engine::Auto => vec![],
    }
}

fn select(options: &ContainerOptions, project: &Path) -> Result<(Native, Vec<Value>)> {
    if options.engine == Engine::Auto && options.endpoint.is_some() {
        return Err(Error::new("--endpoint requires --engine podman or docker"));
    }
    let engines = if options.engine == Engine::Auto {
        vec![Engine::Podman, Engine::Docker]
    } else {
        vec![options.engine]
    };
    let mut reports = Vec::new();
    let mut selected = None;
    for engine in engines {
        for endpoint in endpoints(engine, options.endpoint.as_deref()) {
            match connect(engine, &endpoint, project) {
                Ok(native) => {
                    reports.push(json!({"engine":engine,"endpoint":endpoint,"state":"usable"}));
                    if selected.is_none() {
                        selected = Some(native);
                    }
                }
                Err(_) => {
                    reports.push(json!({"engine":engine,"endpoint":endpoint,"state":"unavailable"}))
                }
            }
        }
    }
    selected.map(|n| (n, reports)).ok_or_else(|| {
        let env = host_environment(Some(project)).unwrap_or_default();
        let installed = [Engine::Podman, Engine::Docker].iter().any(|e| find_executable(e.name(), env.get(OsStr::new("PATH"))).is_some());
        Error::new(if installed {
            "no usable selected container service; start rootless podman.socket or Docker CE and check socket access (configure with --engine/--endpoint)"
        } else { "install Podman or Docker CE to run containers, then start its service" })
    })
}

pub(crate) fn discover(options: &ContainerOptions, json_output: bool) -> Result<i32> {
    let project = std::env::current_dir()?.canonicalize()?;
    let (native, engines) = select(options, &project)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"runtime":"container","selected":native.binding,"engines":engines,"read_only":true,"policy_enforcement":false})
            )?
        );
    } else {
        println!(
            "Container: {} ({})",
            native.binding.engine.name(),
            native.binding.endpoint
        );
    }
    Ok(0)
}

pub(crate) fn diagnostic(config: Option<&Config>) -> Result<Value> {
    if let Some(config) = config
        && fs::symlink_metadata(config.state_path()).is_ok()
    {
        let runtime = Runtime::new(config)?;
        let status = runtime.inspect_bound()?;
        return Ok(json!({"selected":runtime.native.binding,"status":status}));
    }
    let options = config
        .and_then(|c| c.values.container.clone())
        .or(crate::runtime::preferences()?.container)
        .unwrap_or_default();
    let project = config
        .map(|c| c.values.project.clone())
        .unwrap_or(std::env::current_dir()?.canonicalize()?);
    let (native, engines) = select(&options, &project)?;
    Ok(json!({"selected":native.binding,"engines":engines}))
}

pub(crate) fn resolved_options(options: &ContainerOptions) -> Result<ContainerOptions> {
    let (native, _) = select(options, &std::env::current_dir()?.canonicalize()?)?;
    Ok(ContainerOptions {
        engine: native.binding.engine,
        endpoint: Some(native.binding.endpoint),
    })
}

pub(crate) struct RemovalInventory {
    pub(crate) ids: Vec<String>,
    services: Vec<Native>,
}

impl RemovalInventory {
    pub(crate) fn verify_empty(&self) -> Result<()> {
        for native in &self.services {
            let info =
                read_json_text(&native.query(&strings(&["info", "--format", "{{json .}}"]), 5)?)?;
            if service_identity(native.binding.engine, &info)? != native.binding.daemon {
                return Err(Error::new("container service changed during removal"));
            }
            if !native
                .inventory(Some(&format!("label={RUNTIME_LABEL}=container")))?
                .is_empty()
            {
                return Err(Error::new(
                    "container cleanup incomplete; installation retained",
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn removal_inventory(configs: &[Config]) -> Result<RemovalInventory> {
    let mut services = std::collections::BTreeMap::<String, (Native, Vec<String>)>::new();
    if configs.is_empty()
        && let Some(options) = crate::runtime::preferences()?.container
    {
        let (native, _) = select(&options, &std::env::current_dir()?.canonicalize()?)?;
        services.insert("defaults".into(), (native, vec![]));
    }
    for config in configs {
        let runtime = Runtime::new(config)?;
        let expected = if config.state_path().try_exists()? {
            let state = runtime.recovery_state()?;
            if runtime.exists(&state.id)? {
                Some(runtime.checked_state(false, true)?.0.id)
            } else {
                None
            }
        } else {
            None
        };
        let key = format!(
            "{}:{}",
            runtime.native.binding.engine.name(),
            runtime.native.binding.endpoint
        );
        let entry = services
            .entry(key)
            .or_insert_with(|| (runtime.native, vec![]));
        if let Some(id) = expected {
            entry.1.push(id);
        }
    }
    let mut ids = vec![];
    let mut retained = vec![];
    for (_, (native, mut expected)) in services {
        let mut observed = native.inventory(Some(&format!("label={RUNTIME_LABEL}=container")))?;
        expected.sort();
        observed.sort();
        if observed != expected {
            return Err(Error::new(
                "unrecorded or foreign Dungeon containers exist; inspect before uninstall",
            ));
        }
        ids.extend(expected);
        retained.push(native);
    }
    ids.sort();
    Ok(RemovalInventory {
        ids,
        services: retained,
    })
}

pub(crate) fn plan(config: &Config) -> Value {
    json!({"runtime":"container", "container":config.values.container,"image":config.values.image,
        "environment":{"config":config.path,"project":config.values.project,"name":config.values.name},
        "fingerprint":config.fingerprint,
        "project":{"source":config.values.project,"target":GUEST_PROJECT,"read_only":false},
        "configuration_mounts":crate::imports::harness::mounts(config),
        "editor_snapshot":crate::imports::mount_source(config),
        "policy_enforcement":false,"network":"container bridge; outbound unrestricted",
        "verification":"offline plan; no service contacted"})
}

pub(crate) struct Runtime<'a> {
    config: &'a Config,
    native: Native,
    pub(crate) output: crate::output::Output,
}

impl<'a> Runtime<'a> {
    pub(crate) fn new(config: &'a Config) -> Result<Self> {
        let options = config.values.container.as_ref().unwrap();
        let native = match fs::symlink_metadata(config.state_path()) {
            Ok(_) => {
                let state = crate::config::read_runtime_state(&config.state_path())?;
                if state.config != config.fingerprint || state.schema != 1 {
                    return Err(Error::new(
                        "container configuration changed; restore it before reuse",
                    ));
                }
                let recorded = state.container.ok_or_else(|| {
                    Error::new("runtime binding is not a plain container; refusing to switch")
                })?;
                if recorded.engine == Engine::Auto {
                    return Err(Error::new("invalid recorded container engine"));
                }
                let native = connect(recorded.engine, &recorded.endpoint, &config.values.project)?;
                if native.binding != recorded {
                    return Err(Error::new(
                        "container service identity changed; refusing reuse",
                    ));
                }
                native
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                select(options, &config.values.project)?.0
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            config,
            native,
            output: crate::output::Output::default(),
        })
    }
    pub(crate) fn machine(mut self) -> Self {
        self.output = crate::output::Output::machine();
        self
    }
    fn user(&self) -> String {
        match self.native.binding.engine {
            Engine::Podman => "998:998".into(),
            _ => format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
                libc::getegid()
            }),
        }
    }
    fn name(&self) -> String {
        format!(
            "dungeon-{}-{}",
            self.config.values.name,
            &self.config.fingerprint[..8]
        )
    }
    fn mounts(&self) -> Result<Vec<(String, String, bool, bool)>> {
        let source = self
            .config
            .values
            .project
            .to_str()
            .ok_or_else(|| Error::new("container project path must be UTF-8"))?;
        let mut mounts = vec![(source.into(), GUEST_PROJECT.into(), false, true)];
        if let Some(source) = crate::imports::mount_source(self.config) {
            mounts.push((
                source.to_string_lossy().into_owned(),
                "/sandbox/.config/nvim".into(),
                true,
                true,
            ));
        }
        for mount in crate::imports::harness::mounts(self.config) {
            mounts.push((
                mount["source"].as_str().unwrap().into(),
                mount["target"].as_str().unwrap().into(),
                true,
                mount["selinux_label"] == "private",
            ));
        }
        for (source, target, _, _) in &mounts {
            if [source, target]
                .iter()
                .any(|s| s.contains([':', ',', '\n', '\r', '\0']))
            {
                return Err(Error::new(
                    "container mount paths cannot contain colons, commas or control characters",
                ));
            }
        }
        Ok(mounts)
    }
    fn inspect(&self, id: &str) -> Result<Value> {
        checked_id(id)?;
        let value = read_json_text(
            &self
                .native
                .query(&strings(&["container", "inspect", id]), 5)?,
        )?;
        let items = value
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| Error::new("invalid container inspection"))?;
        let c = &items[0];
        if c["Id"] != id
            || c["Config"]["Labels"][LABEL] != self.config.fingerprint
            || c["Config"]["Labels"][RUNTIME_LABEL] != "container"
            || c["Name"].as_str().map(|s| s.trim_start_matches('/')) != Some(self.name().as_str())
        {
            return Err(Error::refusal(
                ErrorCode::IdentityMismatch,
                "container identity does not match; refusing reuse",
            ));
        }
        Ok(c.clone())
    }
    fn specification(&self, c: &Value) -> Result<Value> {
        let mut mounts = Vec::new();
        for m in c["Mounts"]
            .as_array()
            .ok_or_else(|| Error::new("container mounts missing"))?
        {
            if m["Type"] != "bind" {
                return Err(Error::new("unexpected container volume"));
            }
            mounts.push((
                m["Source"]
                    .as_str()
                    .ok_or_else(|| Error::new("mount source missing"))?
                    .to_owned(),
                m["Destination"]
                    .as_str()
                    .ok_or_else(|| Error::new("mount target missing"))?
                    .to_owned(),
                m["RW"]
                    .as_bool()
                    .ok_or_else(|| Error::new("mount mode missing"))?,
            ));
        }
        mounts.sort();
        let mut expected: Vec<_> = self
            .mounts()?
            .into_iter()
            .map(|(s, t, ro, _)| (s, t, !ro))
            .collect();
        expected.sort();
        if mounts != expected
            || c["HostConfig"]["Privileged"] != false
            || c["Config"]["User"] != self.user()
        {
            return Err(Error::new(
                "container user, privilege or mounts differ from reviewed scope",
            ));
        }
        for key in ["Dns", "DnsOptions", "DnsSearch", "ExtraHosts", "GroupAdd"] {
            let value = &c["HostConfig"][key];
            if !value.is_null() && !value.as_array().is_some_and(Vec::is_empty) {
                return Err(Error::new(
                    "container network or group settings differ from the default",
                ));
            }
        }
        let sysctls = &c["HostConfig"]["Sysctls"];
        if !sysctls.is_null() && !sysctls.as_object().is_some_and(serde_json::Map::is_empty) {
            return Err(Error::new("container sysctls differ from the default"));
        }
        let restart = &c["HostConfig"]["RestartPolicy"];
        if !restart.is_null()
            && (!matches!(restart["Name"].as_str(), Some("" | "no"))
                || (!restart["MaximumRetryCount"].is_null()
                    && restart["MaximumRetryCount"].as_u64() != Some(0)))
        {
            return Err(Error::new(
                "container restart policy differs from the default",
            ));
        }
        let config: serde_json::Map<String, Value> = [
            "User",
            "Image",
            "Entrypoint",
            "Cmd",
            "WorkingDir",
            "Env",
            "Labels",
        ]
        .into_iter()
        .map(|k| (k.into(), c["Config"][k].clone()))
        .collect();
        let host: serde_json::Map<String, Value> = [
            "Privileged",
            "NetworkMode",
            "SecurityOpt",
            "CapAdd",
            "CapDrop",
            "PidMode",
            "IpcMode",
            "UTSMode",
            "UsernsMode",
            "ReadonlyRootfs",
            "Devices",
            "PortBindings",
        ]
        .into_iter()
        .map(|k| (k.into(), c["HostConfig"][k].clone()))
        .collect();
        Ok(json!({"image":c["Image"],"config":config,"host":host,"mounts":mounts}))
    }
    fn state(&self) -> Result<RuntimeState> {
        let state = crate::config::read_runtime_state(&self.config.state_path())?;
        if state.schema != 1
            || state.config != self.config.fingerprint
            || state.container.as_ref() != Some(&self.native.binding)
        {
            return Err(Error::new("container binding changed; refusing operation"));
        }
        if state.id.is_empty() {
            if !state.initial_upload_pending || !state.observed_policy.is_empty() {
                return Err(Error::new("invalid pending container creation intent"));
            }
        } else {
            checked_id(&state.id)?;
        }
        Ok(state)
    }
    fn recovery_state(&self) -> Result<RuntimeState> {
        let mut state = self.state()?;
        if state.id.is_empty() {
            let ids = self
                .native
                .inventory(Some(&format!("label={LABEL}={}", self.config.fingerprint)))?;
            if ids.len() != 1 {
                return Err(Error::new(
                    "pending container creation has no uniquely identifiable result; the engine may still be processing it, inspect the pinned service before recovery",
                ));
            }
            let c = self.inspect(&ids[0])?;
            self.specification(&c)?;
            state.id = ids[0].clone();
        }
        Ok(state)
    }
    fn exists(&self, id: &str) -> Result<bool> {
        checked_id(id)?;
        let ids = self.native.inventory(Some(&format!("id={id}")))?;
        if ids.iter().any(|found| found != id) {
            return Err(Error::new("ambiguous container inventory"));
        }
        Ok(!ids.is_empty())
    }
    fn checked(&self, work: bool) -> Result<(RuntimeState, Value)> {
        self.checked_state(work, false)
    }
    fn checked_state(&self, work: bool, recovery: bool) -> Result<(RuntimeState, Value)> {
        let state = self.recovery_state()?;
        let c = self.inspect(&state.id)?;
        if !c["State"]["Running"].is_boolean()
            || (!recovery
                && ["Paused", "Restarting", "Dead"]
                    .iter()
                    .any(|key| c["State"][key] == true))
        {
            return Err(Error::new(
                "container is in an unsupported transitional state; inspect the selected engine",
            ));
        }
        let observed = digest(&self.specification(&c)?)?;
        if !(state.initial_upload_pending && state.observed_policy.is_empty())
            && observed != state.observed_policy
        {
            return Err(Error::new(
                "container configuration drift; refusing operation",
            ));
        }
        if work && state.initial_upload_pending {
            return Err(Error::new(
                "container initialization incomplete; stop and destroy it before retrying",
            ));
        }
        Ok((state, c))
    }
    fn persist(&self, state: &RuntimeState, replace: bool) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(self.config.path.parent().unwrap())?;
        serde_json::to_writer_pretty(&mut file, state)?;
        file.as_file().sync_all()?;
        if replace {
            file.persist(self.config.state_path())
                .map_err(|e| Error::new(e.to_string()))?;
        } else {
            file.persist_noclobber(self.config.state_path())
                .map_err(|e| Error::new(e.to_string()))?;
        }
        fs::File::open(self.config.path.parent().unwrap())?.sync_all()?;
        Ok(())
    }
    pub(crate) fn create(&self) -> Result<i32> {
        self.up()
    }
    pub(crate) fn up(&self) -> Result<i32> {
        crate::imports::verify(self.config)?;
        let lock = crate::export::TransferGuard::environment(self.config)?;
        let _imports = crate::imports::creation_lock(self.config)?;
        if fs::symlink_metadata(self.config.state_path()).is_ok() {
            return self.start_locked();
        }
        if !self.config.values.project.is_dir() {
            return Err(Error::new("project directory is unavailable"));
        }
        self.output.message(format_args!(
            "Selected {} at {}; plain container policy is not enforced.{}",
            self.native.binding.engine.name(),
            self.native.binding.endpoint,
            if self.native.binding.engine == Engine::Docker {
                " Docker daemon access is host-root-equivalent."
            } else {
                ""
            }
        ))?;
        if !self
            .native
            .inventory(Some(&format!("label={LABEL}={}", self.config.fingerprint)))?
            .is_empty()
        {
            return Err(Error::new(
                "unbound container already exists; inspect it before creating another",
            ));
        }
        let image = &self.config.values.image;
        if self
            .native
            .query(&strings(&["image", "inspect", image]), 5)
            .is_err()
        {
            if image.starts_with("localhost/") {
                return Err(Error::new(
                    "build the configured local image before up, or select a registry image with init --image",
                ));
            }
            self.output
                .message(format_args!("Pulling configured container image."))?;
            self.output.mutating();
            self.native.query(&strings(&["pull", image]), 300)?;
        }
        let image_info = read_json_text(
            &self
                .native
                .query(&strings(&["image", "inspect", image]), 5)?,
        )?;
        if image_info[0]["Config"]["Volumes"]
            .as_object()
            .is_some_and(|v| !v.is_empty())
        {
            return Err(Error::new("select an image without implicit volumes"));
        }
        let mut args = strings(&[
            "create",
            "--name",
            &self.name(),
            "--label",
            &format!("{LABEL}={}", self.config.fingerprint),
            "--label",
            &format!("{RUNTIME_LABEL}=container"),
            "--network",
            "bridge",
            "--hostname",
            "dungeon",
            "--env",
            "HOSTNAME=dungeon",
            "--user",
            &self.user(),
            "--cap-drop",
            "NET_RAW",
            "--security-opt",
            "no-new-privileges",
            "--workdir",
            GUEST_PROJECT,
            "--env",
            "HOME=/sandbox",
            "--env",
            "XDG_CONFIG_HOME=/sandbox/.config",
            "--env",
            "XDG_DATA_HOME=/sandbox/.local/share",
            "--env",
            "XDG_STATE_HOME=/sandbox/.local/state",
            "--env",
            "XDG_CACHE_HOME=/sandbox/.cache",
            "--entrypoint",
            "/bin/sleep",
        ]);
        if self.native.binding.engine == Engine::Podman {
            args.push("--userns=keep-id:uid=998,gid=998".into());
        }
        for (source, target, ro, relabel) in self.mounts()? {
            args.extend([
                "--volume".into(),
                format!(
                    "{source}:{target}:{}{}",
                    if ro { "ro" } else { "rw" },
                    if relabel { ",Z" } else { "" }
                ),
            ]);
        }
        args.extend([image.clone(), "infinity".into()]);
        lock.verify_environment(self.config)?;
        let mut state = RuntimeState {
            schema: 1,
            container: Some(self.native.binding.clone()),
            id: String::new(),
            config: self.config.fingerprint.clone(),
            observed_policy: String::new(),
            initial_upload_pending: true,
        };
        self.output.mutating();
        self.persist(&state, false)?;
        let id = self.native.query(&args, 120)?.trim().to_owned();
        checked_id(&id)?;
        state.id = id;
        self.persist(&state, true)?;
        let c = self.inspect(&state.id)?;
        state.observed_policy = digest(&self.specification(&c)?)?;
        self.persist(&state, true)?;
        self.native.query(&strings(&["start", &state.id]), 30)?;
        // Only image-owned parent directories; never recurse into project or imported leaves.
        let script = "set -eu; for p in /sandbox /sandbox/.config /sandbox/.config/opencode /sandbox/.local /sandbox/.local/share /sandbox/.local/state /sandbox/.cache /sandbox/.local/share/opencode /sandbox/.local/state/opencode /sandbox/.cache/opencode; do test ! -L \"$p\"; mkdir -p \"$p\"; chown \"$1\" \"$p\"; done";
        self.native.query(
            &strings(&[
                "exec",
                "--user",
                "0:0",
                &state.id,
                "/bin/sh",
                "-c",
                script,
                "dungeon-init",
                &self.user(),
            ]),
            30,
        )?;
        self.checked(false)?;
        lock.verify_environment(self.config)?;
        state.initial_upload_pending = false;
        self.persist(&state, true)?;
        self.output.message(format_args!(
            "Container ready ({}); project mounted read/write.",
            self.native.binding.engine.name()
        ))?;
        Ok(0)
    }
    fn start_locked(&self) -> Result<i32> {
        let (s, c) = self.checked(true)?;
        if c["State"]["Running"] == false {
            self.output.mutating();
            self.native.query(&strings(&["start", &s.id]), 30)?;
        }
        if self.checked(true)?.1["State"]["Running"] != true {
            return Err(Error::new("container did not start"));
        }
        Ok(0)
    }
    pub(crate) fn start(&self) -> Result<i32> {
        crate::imports::verify(self.config)?;
        let _lock = crate::export::TransferGuard::environment(self.config)?;
        self.start_locked()
    }
    pub(crate) fn down(&self) -> Result<i32> {
        let _lock = crate::export::TransferGuard::environment(self.config)?;
        let state = self.recovery_state()?;
        if !self.exists(&state.id)? {
            return Ok(0);
        }
        let (s, mut c) = self.checked_state(false, true)?;
        if c["State"]["Paused"] == true {
            self.output.mutating();
            self.native.query(&strings(&["unpause", &s.id]), 30)?;
            c = self.checked(false)?.1;
        }
        if c["State"]["Running"] == true || c["State"]["Restarting"] == true {
            self.output.mutating();
            self.native.query(&strings(&["stop", &s.id]), 45)?;
        }
        let stopped = self.checked_state(false, true)?.1;
        if stopped["State"]["Running"] != false || stopped["State"]["Restarting"] == true {
            return Err(Error::new("container did not stop"));
        }
        Ok(0)
    }
    pub(crate) fn inspect_bound(&self) -> Result<Value> {
        let (s, c) = self.checked(false)?;
        Ok(
            json!({"id":s.id,"name":self.config.values.name,"workspace":self.config.values.workspace,
            "phase":if c["State"]["Running"] == true {"Ready"} else {"Stopped"},
            "runtime":"container","engine":self.native.binding.engine,"policy_enforcement":false,"initial_upload_pending":s.initial_upload_pending}),
        )
    }
    pub(crate) fn inspect_for_destroy(&self) -> Result<Value> {
        let state = self.recovery_state()?;
        if !self.exists(&state.id)? {
            return Ok(
                json!({"id":state.id,"name":self.config.values.name,"workspace":self.config.values.workspace,"phase":"Stopped","runtime":"container","engine":self.native.binding.engine,"policy_enforcement":false,"already_absent":true}),
            );
        }
        let (s, c) = self.checked_state(false, true)?;
        Ok(
            json!({"id":s.id,"name":self.config.values.name,"workspace":self.config.values.workspace,
            "phase":if c["State"]["Running"] == false && c["State"]["Restarting"] != true {"Stopped"} else {"Ready"},
            "runtime":"container","engine":self.native.binding.engine,"policy_enforcement":false}),
        )
    }
    pub(crate) fn destroy(&self) -> Result<i32> {
        let lock = crate::export::TransferGuard::environment(self.config)?;
        let _imports = crate::imports::cleanup_lock(self.config)?;
        let state = self.recovery_state()?;
        if self.state()?.id.is_empty() {
            self.persist(&state, true)?;
        }
        if self.exists(&state.id)? {
            let (s, c) = self.checked_state(false, true)?;
            if c["State"]["Running"] != false || c["State"]["Restarting"] == true {
                return Err(Error::new("stop the container before destroy"));
            }
            self.output.mutating();
            let args = if c["State"]["Dead"] == true {
                strings(&["rm", "--force", &s.id])
            } else {
                strings(&["rm", &s.id])
            };
            self.native.query(&args, 30)?;
            if self.exists(&s.id)? {
                return Err(Error::new("container remains; runtime binding retained"));
            }
        }
        lock.verify_environment(self.config)?;
        fs::remove_file(self.config.state_path())?;
        fs::File::open(self.config.path.parent().unwrap())?.sync_all()?;
        Ok(0)
    }
    pub(crate) fn execute(&self, argv: &[String], tty: bool, cwd: &str) -> Result<i32> {
        crate::imports::verify(self.config)?;
        let (s, c) = self.checked(true)?;
        if c["State"]["Running"] != true {
            return Err(Error::new("container is stopped; run dungeon up"));
        }
        if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|s| s.contains('\0')) {
            return Err(Error::new("an executable and valid arguments are required"));
        }
        let mut args = strings(&["exec", "--interactive", "--workdir", cwd]);
        if tty {
            args.push("--tty".into());
        }
        args.push(s.id);
        args.extend_from_slice(argv);
        let mut command = self.native.command(&args);
        if self.output.machine {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }
        self.output.mutating();
        Ok(command.status()?.code().unwrap_or(128))
    }
    pub(crate) fn run_agent(&self, argv: &[String], tty: bool) -> Result<i32> {
        self.start()?;
        self.execute(argv, tty, GUEST_PROJECT)
    }
}
