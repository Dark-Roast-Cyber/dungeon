//! Local native container lifecycle; no OpenShell policy is claimed or emulated.
use crate::config::{
    Config, ContainerBinding, ContainerOptions, Engine, RuntimeState, SelinuxIsolation, digest,
    read_json_text,
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
// The caller supplies the fixed image-owned parent, never a path from configuration.
const PREFERENCE_SEED_SCRIPT: &str = r#"set -eu
parent=$1
if [ -L "$parent" ] || [ ! -d "$parent" ]; then
  echo refusing >&2
  exit 1
fi
cd -P -- "$parent"
if [ -L cli.json ]; then
  echo refusing >&2
  exit 1
fi
if [ -f cli.json ]; then
  exit 0
fi
if [ -e cli.json ]; then
  echo refusing >&2
  exit 1
fi
umask 077
tmp=$(mktemp .dungeon-cli.XXXXXXXXXX)
trap 'rm -f -- "$tmp"' 0
cat > "$tmp"
ln -T -- "$tmp" cli.json
"#;

struct Native {
    binding: ContainerBinding,
    binary: PathBuf,
    env: HashMap<OsString, OsString>,
    selinux_enabled: Option<bool>,
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
        self.query_stdio(args, None, seconds)
    }
    fn query_exit(&self, args: &[String], seconds: u64) -> Result<i32> {
        let mut all = self.prefix();
        all.extend_from_slice(args);
        let arg_refs: Vec<_> = all.iter().map(String::as_str).collect();
        query(
            &self.binary,
            &self.env,
            &arg_refs,
            Duration::from_secs(seconds),
        )
        .ok_or_else(|| {
            Error::new(
                "container service timed out or returned excessive output; inspect the selected service",
            )
        })
        .map(|reply| reply.code)
    }
    fn query_stdin(&self, args: &[String], stdin: &[u8], seconds: u64) -> Result<()> {
        self.query_stdio(args, Some(stdin), seconds).map(|_| ())
    }
    fn query_stdio(&self, args: &[String], stdin: Option<&[u8]>, seconds: u64) -> Result<String> {
        let mut all = self.prefix();
        all.extend_from_slice(args);
        let arg_refs: Vec<_> = all.iter().map(String::as_str).collect();
        let reply = match stdin {
            None => query(
                &self.binary,
                &self.env,
                &arg_refs,
                Duration::from_secs(seconds),
            ),
            Some(stdin) => crate::openshell::local_setup::query_with_stdin(
                &self.binary,
                &self.env,
                &arg_refs,
                Duration::from_secs(seconds),
                stdin,
            ),
        }
        .ok_or_else(|| Error::new("container service timed out or returned excessive output; inspect the selected service"))?;
        if reply.code != 0 {
            return Err(Error::new(format!(
                "{} {} failed; inspect the selected container service",
                self.binding.engine.name(),
                arg_refs.get(self.prefix().len()).unwrap_or(&"command")
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

const ENGINE_REASON_LIMIT: usize = 200;

fn bounded_engine_reason(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > ENGINE_REASON_LIMIT
        || text.chars().any(char::is_control)
    {
        return None;
    }
    Some(text.to_owned())
}

enum StartOutcome {
    Started,
    Refused(Option<String>),
}

enum StartDisposition {
    Removed,
    Pending,
    Stopped,
}

fn start_refusal(engine: &str, reason: Option<&str>, disposition: StartDisposition) -> String {
    let mut message = match reason {
        Some(reason) => format!("{engine} start failed: {reason}"),
        None => format!("{engine} start failed; inspect the selected container service"),
    };
    match disposition {
        StartDisposition::Removed => message.push_str("; no container left pending"),
        StartDisposition::Pending => message
            .push_str("; container initialization incomplete; stop and destroy it before retrying"),
        StartDisposition::Stopped => {}
    }
    message
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
        selinux_enabled: None,
    };
    let info = read_json_text(&native.query(&strings(&["info", "--format", "{{json .}}"]), 5)?)?;
    native.binding.daemon = service_identity(engine, &info)?;
    native.selinux_enabled = info["host"]["security"]["selinuxEnabled"].as_bool();
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

pub(crate) fn prepare_service(
    options: &ContainerOptions,
    dry_run: bool,
) -> Result<ContainerOptions> {
    match resolved_options(options) {
        Ok(selected) => return Ok(selected),
        Err(error) if options.engine == Engine::Docker || options.endpoint.is_some() => {
            return Err(error);
        }
        Err(_) => {}
    }
    let project = std::env::current_dir()?.canonicalize()?;
    let env = engine_environment(&project)?;
    if find_executable("podman", env.get(OsStr::new("PATH"))).is_none() {
        return Err(Error::new(
            "install rootless Podman or Docker CE, then retry setup",
        ));
    }
    let endpoint = endpoints(Engine::Podman, None).remove(0);
    let socket = Path::new(endpoint.strip_prefix("unix://").unwrap());
    match fs::symlink_metadata(socket) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => {
            return Err(Error::new(
                "existing Podman socket is unusable or unreadable; inspect it explicitly. Setup does not change permissions or replace sockets",
            ));
        }
    }
    activate_podman_socket(env, dry_run)?;
    let selected = ContainerOptions {
        engine: Engine::Podman,
        endpoint: Some(endpoint),
    };
    if dry_run {
        Ok(selected)
    } else {
        resolved_options(&selected).map_err(|_| Error::new("Podman service is still unavailable after socket activation; activation may have changed user service state. Inspect it explicitly"))
    }
}

fn activate_podman_socket(mut env: HashMap<OsString, OsString>, dry_run: bool) -> Result<()> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return Err(Error::new(
            "activate rootless Podman as your normal user, not with sudo",
        ));
    }
    println!(
        "{}systemctl --user enable --now podman.socket",
        if dry_run { "Would run: " } else { "Running: " }
    );
    if dry_run {
        return Ok(());
    }
    let binary = find_executable("systemctl", env.get(OsStr::new("PATH"))).ok_or_else(|| {
        Error::new("systemctl is unavailable; start the rootless Podman API service yourself")
    })?;
    env.insert("XDG_RUNTIME_DIR".into(), format!("/run/user/{uid}").into());
    if !query(
        &binary,
        &env,
        &["--user", "enable", "--now", "podman.socket"],
        Duration::from_secs(30),
    )
    .is_some_and(|reply| reply.code == 0)
    {
        return Err(Error::new(
            "Podman socket activation failed or timed out; user service state may have changed. Inspect systemctl --user status podman.socket; no permission changes attempted",
        ));
    }
    Ok(())
}

pub(crate) fn prepare_image(options: &ContainerOptions, image: &str, dry_run: bool) -> Result<()> {
    if !crate::config::valid_image(image) {
        return Err(Error::new("invalid OCI image reference"));
    }
    if dry_run {
        println!(
            "Would {} image: {image}",
            if image.starts_with("localhost/") {
                "verify local"
            } else {
                "pull"
            }
        );
        return Ok(());
    }
    let (native, _) = select(options, &std::env::current_dir()?.canonicalize()?)?;
    if !image.starts_with("localhost/") {
        println!("Pulling image: {image}");
        native.query(&strings(&["pull", image]), 300)?;
    }
    native
        .query(&strings(&["image", "inspect", image]), 5)
        .map_err(|_| {
            Error::new("selected image unavailable; build local images explicitly before setup")
        })?;
    Ok(())
}

pub(crate) fn diagnostic(config: Option<&Config>) -> Result<Value> {
    if let Some(config) = config
        && config.values.container.is_some()
        && fs::symlink_metadata(config.state_path()).is_ok()
    {
        let runtime = Runtime::new(config)?;
        let status = runtime.inspect_bound()?;
        return Ok(json!({"selected":runtime.native.binding,"status":status}));
    }
    let options = config
        .and_then(|c| c.values.container.clone())
        .or(crate::runtime::preferences(config.map(|c| c.values.project.as_path()))?.container)
        .unwrap_or_default();
    let project = config
        .map(|c| c.values.project.clone())
        .unwrap_or(std::env::current_dir()?.canonicalize()?);
    let (native, engines) = select(&options, &project)?;
    Ok(json!({"selected":native.binding,"engines":engines,
        "selinux_isolation":config.map(|c| json!({"requested":c.values.selinux_isolation,"verification":"not-inspected"}))}))
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
        && let Some(options) = crate::runtime::preferences(None)?.container
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
    let mut plan = json!({"runtime":"container", "container":config.values.container,"image":config.values.image,
        "environment":{"config":config.path,"project":config.values.project,"name":config.values.name},
        "fingerprint":config.fingerprint,
        "selinux_isolation":{"requested":config.values.selinux_isolation,"verification":"not-inspected"},
        "project":{"source":config.values.project,"target":GUEST_PROJECT,"read_only":false},
        "configuration_mounts":crate::imports::harness::mounts(config),
        "guest_preference_seed":crate::imports::harness::preference_seed_plan(config),
        "editor_snapshot":crate::imports::mount_source(config),
        "policy_enforcement":false,"network":"container bridge; outbound unrestricted",
        "verification":"offline plan; no service contacted"});
    if let Some(version) = crate::imports::harness::guest_agent_version(config) {
        plan["guest_agent_version"] = version;
    }
    plan
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
        if config.values.selinux_isolation == SelinuxIsolation::Disabled
            && native.binding.engine != Engine::Podman
        {
            return Err(Error::refusal(
                ErrorCode::Unsupported,
                "disabled SELinux isolation requires rootless Podman; refusing engine fallback",
            ));
        }
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
        let relabel = self.config.values.selinux_isolation.is_private();
        let mut mounts = vec![(source.into(), GUEST_PROJECT.into(), false, relabel)];
        if let Some(source) = crate::imports::mount_source(self.config) {
            mounts.push((
                source.to_string_lossy().into_owned(),
                "/sandbox/.config/nvim".into(),
                true,
                relabel,
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
        let disabled = c["HostConfig"]["SecurityOpt"]
            .as_array()
            .is_some_and(|options| options.iter().any(|o| o == "label=disable"));
        match (
            self.native.binding.engine,
            self.config.values.selinux_isolation,
        ) {
            (Engine::Podman, SelinuxIsolation::Disabled)
                if !disabled || c["ProcessLabel"] != "" =>
            {
                return Err(Error::new(
                    "native SELinux configuration differs from requested disabled isolation",
                ));
            }
            (Engine::Podman, SelinuxIsolation::Private) if disabled => {
                return Err(Error::new(
                    "native SELinux configuration disables requested private isolation",
                ));
            }
            (Engine::Podman, SelinuxIsolation::Private)
                if self.native.selinux_enabled == Some(true) =>
            {
                let process = c["ProcessLabel"].as_str().unwrap_or("");
                let mount = c["MountLabel"].as_str().unwrap_or("");
                let categories = process.splitn(4, ':').nth(3).unwrap_or("");
                if !categories.starts_with("s0:c")
                    || mount.splitn(4, ':').nth(3) != Some(categories)
                {
                    return Err(Error::new(
                        "native SELinux private process/mount MCS labels are missing or mismatched",
                    ));
                }
            }
            _ => {}
        }
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
        let preference_seed = crate::imports::harness::preference_seed_bytes(self.config)?;
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
        if self.config.values.selinux_isolation == SelinuxIsolation::Disabled {
            args.extend(strings(&["--security-opt", "label=disable"]));
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
        if let StartOutcome::Refused(reason) = self.start_container(&state.id)? {
            let disposition = if self.retire_unstarted(&state.id).is_ok() {
                StartDisposition::Removed
            } else {
                StartDisposition::Pending
            };
            return Err(Error::new(start_refusal(
                self.native.binding.engine.name(),
                reason.as_deref(),
                disposition,
            )));
        }
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
        // First create only. start_locked does not seed again; a failure stays pending.
        if let Some(bytes) = preference_seed {
            self.seed_preference(&state.id, &bytes)?;
        }
        state.initial_upload_pending = false;
        self.persist(&state, true)?;
        self.output.message(format_args!(
            "Container ready ({}); project mounted read/write.",
            self.native.binding.engine.name()
        ))?;
        Ok(0)
    }
    fn seed_preference(&self, id: &str, bytes: &[u8]) -> Result<()> {
        let user = self.user();
        let args = vec![
            "exec".into(),
            "--interactive".into(),
            "--user".into(),
            user,
            id.into(),
            "/bin/sh".into(),
            "-c".into(),
            PREFERENCE_SEED_SCRIPT.into(),
            "dungeon-seed".into(),
            "/sandbox/.config/opencode".into(),
        ];
        self.native.query_stdin(&args, bytes, 30).map_err(|error| {
            Error::new(format!(
                "guest preference seed failed; container initialization incomplete: {error}"
            ))
        })
    }
    fn start_container(&self, id: &str) -> Result<StartOutcome> {
        if self.native.query_exit(&strings(&["start", id]), 30)? == 0 {
            return Ok(StartOutcome::Started);
        }
        let reason = self.inspect(id).ok().and_then(|container| {
            container["State"]["Error"]
                .as_str()
                .and_then(bounded_engine_reason)
        });
        Ok(StartOutcome::Refused(reason))
    }
    fn retire_unstarted(&self, id: &str) -> Result<()> {
        let container = self.inspect(id)?;
        if container["State"]["Running"] != false
            || container["State"]["Restarting"] == true
            || container["State"]["Paused"] == true
        {
            return Err(Error::new(
                "container start was not a confirmed stopped refusal",
            ));
        }
        self.output.mutating();
        let args = if container["State"]["Dead"] == true {
            strings(&["rm", "--force", id])
        } else {
            strings(&["rm", id])
        };
        self.native.query(&args, 30)?;
        if self.exists(id)? {
            return Err(Error::new("container remains; runtime binding retained"));
        }
        fs::remove_file(self.config.state_path())?;
        fs::File::open(self.config.path.parent().unwrap())?.sync_all()?;
        Ok(())
    }
    fn start_locked(&self) -> Result<i32> {
        let (s, c) = self.checked(true)?;
        if c["State"]["Running"] == false {
            self.output.mutating();
            if let StartOutcome::Refused(reason) = self.start_container(&s.id)? {
                return Err(Error::new(start_refusal(
                    self.native.binding.engine.name(),
                    reason.as_deref(),
                    StartDisposition::Stopped,
                )));
            }
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
            "runtime":"container","engine":self.native.binding.engine,"policy_enforcement":false,"initial_upload_pending":s.initial_upload_pending,
            "selinux_isolation":{"requested":self.config.values.selinux_isolation,"verification":if self.native.binding.engine == Engine::Podman {"native-configuration"} else {"not-inspected"},"process_label":c["ProcessLabel"],"mount_label":c["MountLabel"],"host_selinux_enabled":self.native.selinux_enabled}}),
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

#[cfg(test)]
mod setup_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn socket_activation_uses_only_the_fixed_user_command_and_dry_run_never_executes() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("activation");
        let binary = root.path().join("systemctl");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" \"$XDG_RUNTIME_DIR\" > '{}'\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let env = HashMap::from([("PATH".into(), root.path().as_os_str().to_owned())]);
        let dry = activate_podman_socket(env.clone(), true);
        assert!(!marker.exists());
        if unsafe { libc::geteuid() } == 0 {
            assert!(dry.is_err());
            assert!(activate_podman_socket(env, false).is_err());
            assert!(!marker.exists());
        } else {
            dry.unwrap();
            activate_podman_socket(env, false).unwrap();
            assert_eq!(
                fs::read_to_string(marker).unwrap(),
                format!(
                    "--user enable --now podman.socket\n/run/user/{}\n",
                    unsafe { libc::geteuid() }
                )
            );
        }
    }
}

#[cfg(test)]
mod preference_seed_tests {
    use super::PREFERENCE_SEED_SCRIPT;
    use std::fs;
    use std::io::{Seek, Write};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    fn run_seed(parent: &Path, input: &[u8]) -> Output {
        let mut stdin = tempfile::tempfile().unwrap();
        stdin.write_all(input).unwrap();
        stdin.rewind().unwrap();
        Command::new("/bin/sh")
            .arg("-c")
            .arg(PREFERENCE_SEED_SCRIPT)
            .arg("dungeon-seed")
            .arg(parent)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::from(stdin))
            .output()
            .unwrap()
    }

    #[test]
    fn preference_seed_publishes_exact_private_bytes_once() {
        let bytes = b"{\"theme\":\"synthetic\"}\n\xff";
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("cli.json");
        let created = run_seed(root.path(), bytes);
        assert!(created.status.success(), "{created:?}");
        assert_eq!(fs::read(&dest).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let unchanged = run_seed(root.path(), b"replacement");
        assert!(unchanged.status.success(), "{unchanged:?}");
        assert_eq!(fs::read(&dest).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn preference_seed_preserves_image_owned_preferences() {
        let prior = tempfile::tempdir().unwrap();
        let prior_file = prior.path().join("cli.json");
        fs::write(&prior_file, b"keep").unwrap();
        fs::set_permissions(&prior_file, fs::Permissions::from_mode(0o644)).unwrap();
        let kept = run_seed(prior.path(), b"overwrite");
        assert!(kept.status.success(), "{kept:?}");
        assert_eq!(fs::read(&prior_file).unwrap(), b"keep");
        assert_eq!(
            fs::metadata(&prior_file).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn preference_seed_refuses_symlink_destinations() {
        let linked = tempfile::tempdir().unwrap();
        let target = linked.path().join("target");
        let link = linked.path().join("cli.json");
        symlink(&target, &link).unwrap();
        let denied = run_seed(linked.path(), b"through-link");
        assert!(!denied.status.success(), "{denied:?}");
        assert!(!target.exists());
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    }

    #[test]
    fn preference_seed_refuses_symlink_parents() {
        let parent = tempfile::tempdir().unwrap();
        let real = parent.path().join("real");
        fs::create_dir(&real).unwrap();
        let via = parent.path().join("via");
        symlink(&real, &via).unwrap();
        let parent_denied = run_seed(&via, b"through-parent");
        assert!(!parent_denied.status.success(), "{parent_denied:?}");
        assert!(!real.join("cli.json").exists());
    }

    #[test]
    fn preference_seed_refuses_non_file_destinations() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("cli.json")).unwrap();
        let rejected = run_seed(root.path(), b"not-seeded");
        assert!(!rejected.status.success(), "{rejected:?}");
        assert_eq!(
            fs::read_dir(root.path().join("cli.json")).unwrap().count(),
            0
        );
    }

    #[test]
    fn preference_seed_never_moves_data_into_a_racing_directory() {
        let root = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let ln = bin.path().join("ln");
        fs::write(&ln, "#!/bin/sh\n/bin/mkdir cli.json\nexec /bin/ln \"$@\"\n").unwrap();
        fs::set_permissions(&ln, fs::Permissions::from_mode(0o700)).unwrap();
        let mut stdin = tempfile::tempfile().unwrap();
        stdin.write_all(b"private-seed").unwrap();
        stdin.rewind().unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", PREFERENCE_SEED_SCRIPT, "dungeon-seed"])
            .arg(root.path())
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin.path().display()))
            .stdin(Stdio::from(stdin))
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(
            fs::read_dir(root.path().join("cli.json")).unwrap().count(),
            0
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
