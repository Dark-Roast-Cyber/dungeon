use clap::{Parser, Subcommand, ValueEnum};
use data_encoding::BASE32_NOPAD;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const OPEN_SHELL_VERSION: &str = "0.0.116";
const DEFAULT_IMAGE: &str = "ghcr.io/nvidia/openshell-community/sandboxes/base:latest";
const LABEL: &str = "agentarchaeology.ai/dungeon-config";
const GUEST_PROJECT: &str = "/sandbox/project";
const MAX_JSON_BYTES: u64 = 1_048_576;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
struct Error(String);

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lower")]
enum Agent {
    Opencode,
    Claude,
    Codex,
}
impl Agent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lower")]
enum Profile {
    Minimal,
    Development,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lower")]
enum AuthAgent {
    Claude,
    Codex,
}

#[derive(Debug, Parser)]
#[command(
    name = "dungeon",
    version,
    about = "Manage agent environments with OpenShell"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "explicit private dungeon.json; no project discovery"
    )]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long, default_value = "default")]
        workspace: String,
        #[arg(long, default_value = DEFAULT_IMAGE)]
        image: String,
        #[arg(long, value_enum, default_value = "opencode")]
        agent: Agent,
        #[arg(long, value_enum, default_value = "development")]
        profile: Profile,
        #[arg(long = "provider")]
        providers: Vec<String>,
        #[arg(long)]
        non_interactive: bool,
    },
    Plan,
    Create,
    Status,
    Start,
    Stop,
    Shell,
    Code,
    SshConfig,
    Telltale,
    Upload {
        #[arg(long)]
        yes: bool,
    },
    Auth {
        #[arg(value_enum)]
        agent: AuthAgent,
    },
    Run {
        #[arg(value_enum)]
        agent: Option<Agent>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    Exec {
        #[arg(long)]
        tty: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        arguments: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigDoc {
    schema: u64,
    name: String,
    gateway: String,
    workspace: String,
    project: PathBuf,
    image: String,
    agent: Agent,
    providers: Vec<String>,
    policy: String,
}

#[derive(Clone, Debug)]
struct Config {
    path: PathBuf,
    values: ConfigDoc,
    policy: Value,
    policy_path: PathBuf,
    fingerprint: String,
}
impl Config {
    fn state_path(&self) -> PathBuf {
        self.path
            .parent()
            .expect("config parent")
            .join("runtime.json")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeState {
    schema: u64,
    id: String,
    config: String,
    observed_policy: String,
}

fn main() {
    std::process::exit(match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("dungeon: {error}");
            1
        }
    });
}

fn run(cli: Cli) -> Result<i32> {
    let Cli { config, command } = cli;
    match command {
        Commands::Init {
            directory,
            project,
            name,
            gateway,
            workspace,
            image,
            agent,
            profile,
            providers,
            non_interactive,
        } => initialize(
            directory,
            project,
            name,
            gateway,
            workspace,
            image,
            agent,
            profile,
            providers,
            non_interactive,
        ),
        Commands::Plan => {
            let config = load_config(&required_config(config)?)?;
            let mut command = vec![
                "openshell".into(),
                "--gateway".into(),
                config.values.gateway.clone(),
                "--workspace".into(),
                config.values.workspace.clone(),
            ];
            command.extend(create_arguments(&config, &config.policy_path));
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "adapter": "openshell",
                    "version": OPEN_SHELL_VERSION,
                    "fingerprint": config.fingerprint,
                    "command": command,
                    "policy": config.policy,
                    "project_transfer": "separate explicit upload; no host bind mount",
                    "telemetry": "optional in-guest diagnostic only; no durable collection",
                    "verification": "plan only; no backend contacted or enforcement verified"
                }))?
            );
            Ok(0)
        }
        Commands::Upload { yes: false } => Err(Error::new(
            "review project contents, including secrets and symlinks, then repeat upload --yes",
        )),
        command => {
            let config = load_config(&required_config(config)?)?;
            let runtime = Runtime::new(&config)?;
            match command {
                Commands::Create => runtime.create(),
                Commands::Status => {
                    println!("{}", serde_json::to_string_pretty(&runtime.get()?)?);
                    Ok(0)
                }
                Commands::Upload { yes: true } => runtime.upload(),
                Commands::Auth {
                    agent: AuthAgent::Codex,
                } => {
                    eprintln!("Starting Codex device authentication inside the guest. Complete the displayed URL/code in your browser. Dungeon never reads or stores the resulting credential.");
                    runtime.execute(
                        &["codex".into(), "login".into(), "--device-auth".into()],
                        true,
                        "/sandbox",
                    )
                }
                Commands::Auth {
                    agent: AuthAgent::Claude,
                } => {
                    eprintln!("Starting Claude Code inside the guest. Use /login and complete the vendor browser flow. Dungeon never reads or stores the resulting credential.");
                    runtime.execute(&["claude".into()], true, "/sandbox")
                }
                Commands::Run { agent, arguments } => {
                    let selected = agent.unwrap_or(config.values.agent);
                    let mut command = vec![selected.as_str().to_owned()];
                    command.extend(arguments);
                    runtime.execute(
                        &command,
                        io::stdin().is_terminal() && io::stdout().is_terminal(),
                        GUEST_PROJECT,
                    )
                }
                Commands::Exec { tty, arguments } => {
                    runtime.execute(&arguments, tty, GUEST_PROJECT)
                }
                Commands::Shell => {
                    runtime.execute(&["/bin/bash".into(), "-l".into()], true, "/sandbox")
                }
                Commands::Code => runtime.connect_vscode(),
                Commands::SshConfig => runtime.ssh_config(),
                Commands::Start => runtime.lifecycle("start"),
                Commands::Stop => runtime.lifecycle("stop"),
                Commands::Telltale => {
                    eprintln!(
                        "In-guest diagnostic, not an independent audit or durable collector."
                    );
                    runtime.execute(
                        &[
                            "telltale".into(),
                            "scan".into(),
                            "--once".into(),
                            "--dry-run".into(),
                            "--no-local-config".into(),
                            "--root".into(),
                            "/sandbox".into(),
                        ],
                        false,
                        "/sandbox",
                    )
                }
                Commands::Init { .. } | Commands::Plan | Commands::Upload { yes: false } => {
                    unreachable!()
                }
            }
        }
    }
}

fn required_config(path: Option<PathBuf>) -> Result<PathBuf> {
    path.ok_or_else(|| {
        Error::new("--config is required; project-local configuration is never auto-loaded")
    })
}

#[allow(clippy::too_many_arguments)]
fn initialize(
    directory: PathBuf,
    project: PathBuf,
    mut name: Option<String>,
    mut gateway: Option<String>,
    workspace: String,
    mut image: String,
    mut agent: Agent,
    mut profile: Profile,
    providers: Vec<String>,
    non_interactive: bool,
) -> Result<i32> {
    let project = project.canonicalize()?;
    if !project.is_dir() || project.parent().is_none() || is_home(&project) {
        return Err(Error::new(
            "select a project directory, not a filesystem root or your home",
        ));
    }
    let directory = if directory.is_absolute() {
        directory
    } else {
        env::current_dir()?.join(directory)
    };
    if directory.starts_with(&project) {
        return Err(Error::new("configuration must be outside the project"));
    }
    if directory.exists() {
        return Err(Error::new(
            "use a new configuration directory; existing directories are never overwritten",
        ));
    }
    if !non_interactive {
        if !io::stdin().is_terminal() {
            return Err(Error::new(
                "use --non-interactive --name NAME --gateway NAME without a terminal",
            ));
        }
        name = Some(ask(
            "Environment name",
            name.as_deref().unwrap_or("development"),
        )?);
        gateway = Some(ask(
            "Registered OpenShell gateway",
            gateway.as_deref().unwrap_or("local"),
        )?);
        image = ask("Reviewed OCI image", &image)?;
        agent = Agent::from_str(
            &ask("Default agent (opencode/claude/codex)", agent.as_str())?,
            true,
        )
        .map_err(|_| Error::new("unknown default agent"))?;
        let default = if profile == Profile::Minimal {
            "minimal"
        } else {
            "development"
        };
        profile = Profile::from_str(&ask("Starter policy (minimal/development)", default)?, true)
            .map_err(|_| Error::new("unknown starter profile"))?;
    }
    let name = name.ok_or_else(|| Error::new("--name is required with --non-interactive"))?;
    let gateway =
        gateway.ok_or_else(|| Error::new("--gateway is required with --non-interactive"))?;
    for (field, value) in [
        ("name", name.as_str()),
        ("gateway", gateway.as_str()),
        ("workspace", workspace.as_str()),
    ] {
        safe_name(value, field)?;
    }
    let mut seen = HashSet::new();
    for provider in &providers {
        safe_name(provider, "provider")?;
        if !seen.insert(provider) {
            return Err(Error::new("provider names must not repeat"));
        }
    }
    create_private_dir(&directory)?;
    write_new(&directory.join("policy.json"), &starter_policy(profile))?;
    write_new(
        &directory.join("dungeon.json"),
        &ConfigDoc {
            schema: 1,
            name,
            gateway,
            workspace,
            project,
            image,
            agent,
            providers,
            policy: "policy.json".into(),
        },
    )?;
    let config = load_config(&directory.join("dungeon.json"))?;
    println!("Configuration: {}", config.path.display());
    println!("Review policy.json and named providers, then run plan. No runtime was started.");
    Ok(0)
}

fn ask(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();
    Ok(if value.is_empty() {
        default.to_owned()
    } else {
        value.to_owned()
    })
}

fn create_private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn is_home(path: &Path) -> bool {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.canonicalize().unwrap_or(home) == path)
        .unwrap_or(false)
}

fn safe_name(value: &str, field: &str) -> Result<()> {
    let b = value.as_bytes();
    let edge = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let allowed = |c: u8| edge(c) || c == b'-';
    if b.is_empty()
        || b.len() > 63
        || !edge(b[0])
        || !edge(b[b.len() - 1])
        || !b.iter().copied().all(allowed)
    {
        return Err(Error::new(format!(
            "{field} must be a lowercase DNS label, 1 to 63 characters"
        )));
    }
    Ok(())
}

fn valid_image(image: &str) -> bool {
    if image.is_empty()
        || !image
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/@-".contains(&b))
    {
        return false;
    }
    let Some((registry, rest)) = image.split_once('/') else {
        return false;
    };
    !rest.is_empty()
        && (registry.contains('.') || registry.contains(':') || registry == "localhost")
        && !image.split('/').any(|p| p == "..")
}

fn load_config(path: &Path) -> Result<Config> {
    let path = path.canonicalize()?;
    let mut values: ConfigDoc = serde_json::from_value(read_json(&path)?)
        .map_err(|e| Error::new(format!("invalid dungeon.json: {e}")))?;
    if values.schema != 1 {
        return Err(Error::new("expected configuration schema 1"));
    }
    for (field, value) in [
        ("name", values.name.as_str()),
        ("gateway", values.gateway.as_str()),
        ("workspace", values.workspace.as_str()),
    ] {
        safe_name(value, field)?;
    }
    let mut seen = HashSet::new();
    for provider in &values.providers {
        safe_name(provider, "provider")?;
        if !seen.insert(provider) {
            return Err(Error::new("provider names must not repeat"));
        }
    }
    if !values.project.is_absolute() {
        return Err(Error::new("project must be an absolute path"));
    }
    if values.project.exists() {
        if !values.project.is_dir() {
            return Err(Error::new("project must be a directory"));
        }
        values.project = values.project.canonicalize()?;
    }
    if values.project.parent().is_none() || is_home(&values.project) {
        return Err(Error::new(
            "select a project directory, not a filesystem root or your home",
        ));
    }
    if path.starts_with(&values.project) {
        return Err(Error::new(
            "keep Dungeon configuration outside the uploaded project",
        ));
    }
    if !valid_image(&values.image) {
        return Err(Error::new(
            "use a registry-qualified OCI image, not a local build path or unqualified name",
        ));
    }
    if values.policy != "policy.json" {
        return Err(Error::new("policy must name the adjacent policy.json file"));
    }
    let parent = path.parent().expect("config parent");
    let policy_path = parent.join("policy.json").canonicalize()?;
    if policy_path.parent() != Some(parent) || policy_path.starts_with(&values.project) {
        return Err(Error::new(
            "policy must stay in the private configuration directory",
        ));
    }
    let policy = read_json(&policy_path)?;
    validate_policy(&policy)?;
    let fingerprint = digest(&json!({
        "adapter": OPEN_SHELL_VERSION,
        "config": serde_json::to_value(&values)?,
        "policy": policy.clone()
    }))?;
    Ok(Config {
        path,
        values,
        policy,
        policy_path,
        fingerprint,
    })
}

fn starter_policy(profile: Profile) -> Value {
    let mut policy = json!({
        "version": 1,
        "filesystem_policy": {
            "include_workdir": true,
            "read_only": ["/usr", "/lib", "/proc", "/dev/urandom", "/app", "/etc", "/var/log"],
            "read_write": ["/sandbox", "/tmp", "/dev/null"]
        },
        "landlock": {"compatibility": "hard_requirement"},
        "process": {"run_as_user": "sandbox", "run_as_group": "sandbox"},
        "network_policies": {
            "models": {
                "name": "models",
                "endpoints": [
                    {"host":"api.anthropic.com","port":443},{"host":"api.openai.com","port":443},
                    {"host":"auth.openai.com","port":443},{"host":"chatgpt.com","port":443},
                    {"host":"claude.ai","port":443},{"host":"opencode.ai","port":443},{"host":"models.dev","port":443}
                ],
                "binaries": [{"path":"/usr/bin/**"},{"path":"/usr/local/bin/**"},{"path":"/usr/lib/node_modules/**"},{"path":"/sandbox/**"}]
            }
        }
    });
    if profile == Profile::Development {
        policy["network_policies"]["development"] = json!({
            "name": "development",
            "endpoints": [
                {"host":"github.com","port":443},{"host":"api.github.com","port":443},
                {"host":"raw.githubusercontent.com","port":443},{"host":"registry.npmjs.org","port":443},
                {"host":"pypi.org","port":443},{"host":"files.pythonhosted.org","port":443},
                {"host":"crates.io","port":443},{"host":"index.crates.io","port":443},{"host":"static.crates.io","port":443},
                {"host":"update.code.visualstudio.com","port":443},{"host":"vscode.download.prss.microsoft.com","port":443},
                {"host":"marketplace.visualstudio.com","port":443}
            ],
            "binaries": [{"path":"/usr/bin/**"},{"path":"/usr/local/bin/**"},{"path":"/usr/lib/node_modules/**"},{"path":"/sandbox/**"}]
        });
    }
    policy
}

fn validate_policy(policy: &Value) -> Result<()> {
    let object = policy
        .as_object()
        .ok_or_else(|| Error::new("policy must be a JSON object"))?;
    if object.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(Error::new("expected OpenShell policy version 1"));
    }
    if object
        .get("landlock")
        .and_then(Value::as_object)
        .and_then(|v| v.get("compatibility"))
        .and_then(Value::as_str)
        != Some("hard_requirement")
    {
        return Err(Error::new(
            "Dungeon requires landlock.compatibility=hard_requirement",
        ));
    }
    let process = object
        .get("process")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::new("policy needs process settings"))?;
    if process.get("run_as_user").and_then(Value::as_str) != Some("sandbox")
        || process.get("run_as_group").and_then(Value::as_str) != Some("sandbox")
    {
        return Err(Error::new(
            "the preview image contract requires the sandbox user and group",
        ));
    }
    let network = object
        .get("network_policies")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::new("network_policies must be an object"))?;
    for block in network.values() {
        let endpoints = block
            .as_object()
            .and_then(|v| v.get("endpoints"))
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new("each network policy needs an endpoints array"))?;
        for endpoint in endpoints {
            let endpoint = endpoint
                .as_object()
                .ok_or_else(|| Error::new("network endpoints must be objects"))?;
            let inspected = endpoint.get("protocol").and_then(Value::as_str) == Some("rest")
                || endpoint.get("tls").and_then(Value::as_str) == Some("terminate");
            if inspected && endpoint.get("enforcement").and_then(Value::as_str) != Some("enforce") {
                return Err(Error::new(
                    "inspected endpoints must use enforcement=enforce",
                ));
            }
        }
    }
    Ok(())
}

struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("valid JSON without duplicate keys")
            }
            fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(Number::from(v))))
            }
            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(Number::from(v))))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.to_owned())))
            }
            fn visit_string<E>(self, v: String) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_some<D: Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<Self::Value, D::Error> {
                StrictValue::deserialize(d)
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = seq.next_element::<StrictValue>()? {
                    out.push(v.0);
                }
                Ok(StrictValue(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = Map::new();
                while let Some((k, v)) = map.next_entry::<String, StrictValue>()? {
                    if out.contains_key(&k) {
                        return Err(de::Error::custom(format!("duplicate JSON key: {k}")));
                    }
                    out.insert(k, v.0);
                }
                Ok(StrictValue(Value::Object(out)))
            }
        }
        deserializer.deserialize_any(V)
    }
}
fn decode_json(text: &str) -> Result<Value> {
    let mut d = serde_json::Deserializer::from_str(text);
    let v =
        StrictValue::deserialize(&mut d).map_err(|e| Error::new(format!("invalid JSON: {e}")))?;
    d.end()
        .map_err(|e| Error::new(format!("invalid JSON: {e}")))?;
    Ok(v.0)
}
fn read_json(path: &Path) -> Result<Value> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(Error::new("configuration exceeds 1 MiB"));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| Error::new("configuration must be UTF-8 JSON"))?;
    decode_json(&text)
}
fn write_new<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
fn digest(value: &Value) -> Result<String> {
    Ok(BASE32_NOPAD
        .encode(&Sha256::digest(serde_json::to_vec(value)?))
        .to_ascii_lowercase())
}

fn create_arguments(config: &Config, policy_path: &Path) -> Vec<String> {
    let mut args = vec![
        "sandbox".into(),
        "create".into(),
        "--name".into(),
        config.values.name.clone(),
        "--from".into(),
        config.values.image.clone(),
        "--policy".into(),
        policy_path.to_string_lossy().into_owned(),
        "--no-auto-providers".into(),
        "--approval-mode".into(),
        "manual".into(),
        "--label".into(),
        format!("{LABEL}={}", config.fingerprint),
        "--detach".into(),
        "--output".into(),
        "json".into(),
    ];
    for provider in &config.values.providers {
        args.extend(["--provider".into(), provider.clone()]);
    }
    args
}

fn host_environment(project: &Path) -> Result<HashMap<OsString, OsString>> {
    const ALLOWED: &[&str] = &[
        "HOME",
        "USER",
        "LOGNAME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "SystemRoot",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "TERM",
        "COLORTERM",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
        "TMP",
        "TEMP",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "SSH_AUTH_SOCK",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
    ];
    let mut out = HashMap::new();
    for key in ALLOWED {
        if let Some(v) = env::var_os(key) {
            out.insert(OsString::from(key), v);
        }
    }
    let mut paths = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        for entry in env::split_paths(&path) {
            if entry.is_absolute() {
                let resolved = entry.canonicalize().unwrap_or(entry);
                if !resolved.starts_with(project) {
                    paths.push(resolved);
                }
            }
        }
    }
    out.insert(
        OsString::from("PATH"),
        env::join_paths(paths).map_err(|_| Error::new("PATH contains an unsupported entry"))?,
    );
    out.insert(OsString::from("NO_COLOR"), OsString::from("1"));
    Ok(out)
}
fn find_executable(name: &str, path: Option<&OsString>) -> Option<PathBuf> {
    for dir in env::split_paths(path?) {
        let candidate = dir.join(format!("{name}{}", env::consts::EXE_SUFFIX));
        if candidate.is_file() {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    None
}

struct Runtime<'a> {
    config: &'a Config,
    env: HashMap<OsString, OsString>,
    binary: PathBuf,
}
impl<'a> Runtime<'a> {
    fn new(config: &'a Config) -> Result<Self> {
        let env = host_environment(&config.values.project)?;
        let binary =
            find_executable("openshell", env.get(&OsString::from("PATH"))).ok_or_else(|| {
                Error::new(
                    "install the OpenShell 0.0.116 CLI outside the project and add it to PATH",
                )
            })?;
        if binary.starts_with(&config.values.project) {
            return Err(Error::new("refusing a project-owned OpenShell executable"));
        }
        let runtime = Self {
            config,
            env,
            binary,
        };
        let version = runtime.capture(&["--version".into()], false)?;
        let version = version.trim();
        if version != format!("openshell {OPEN_SHELL_VERSION}")
            && version != format!("openshell v{OPEN_SHELL_VERSION}")
        {
            return Err(Error::new(format!(
                "this adapter requires OpenShell {OPEN_SHELL_VERSION}"
            )));
        }
        Ok(runtime)
    }
    fn command(&self, args: &[String], scoped: bool) -> Command {
        let mut c = Command::new(&self.binary);
        if scoped {
            c.args([
                "--gateway",
                self.config.values.gateway.as_str(),
                "--workspace",
                self.config.values.workspace.as_str(),
            ]);
        }
        c.args(args)
            .current_dir(self.config.path.parent().expect("config parent"))
            .env_clear()
            .envs(self.env.iter());
        c
    }
    fn capture(&self, args: &[String], scoped: bool) -> Result<String> {
        let output = self.command(args, scoped).stdin(Stdio::null()).output()?;
        if !output.status.success() {
            return Err(Error::new(format!(
                "OpenShell failed (exit {}); inspect the gateway directly",
                output.status.code().unwrap_or(128)
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| Error::new("OpenShell returned non-UTF-8 structured output"))
    }
    fn status(&self, args: &[String]) -> Result<i32> {
        Ok(self.command(args, true).status()?.code().unwrap_or(128))
    }
    fn get(&self) -> Result<Value> {
        let value = decode_json(&self.capture(
            &[
                "sandbox".into(),
                "get".into(),
                self.config.values.name.clone(),
                "--output".into(),
                "json".into(),
            ],
            true,
        )?)?;
        if !value.is_object() {
            return Err(Error::new("unexpected OpenShell sandbox response"));
        }
        Ok(value)
    }
    fn remote_str<'b>(remote: &'b Value, key: &str) -> Result<&'b str> {
        remote
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(format!("sandbox response is missing {key}")))
    }
    fn check_identity(&self, remote: &Value) -> Result<()> {
        let label = remote
            .get("labels")
            .and_then(Value::as_object)
            .and_then(|m| m.get(LABEL))
            .and_then(Value::as_str);
        if Self::remote_str(remote, "name")? != self.config.values.name
            || Self::remote_str(remote, "workspace")? != self.config.values.workspace
            || Self::remote_str(remote, "id")?.is_empty()
            || label != Some(self.config.fingerprint.as_str())
        {
            return Err(Error::new(
                "sandbox identity or configuration label does not match; refusing reuse",
            ));
        }
        Ok(())
    }
    fn checked(&self, ready: bool, policy: bool) -> Result<Value> {
        let state: RuntimeState = serde_json::from_value(read_json(&self.config.state_path())?)
            .map_err(|e| Error::new(format!("invalid runtime.json: {e}")))?;
        if state.schema != 1 || state.config != self.config.fingerprint {
            return Err(Error::new(
                "launch configuration changed; restore it or initialize a new environment",
            ));
        }
        let remote = self.get()?;
        self.check_identity(&remote)?;
        if Self::remote_str(&remote, "id")? != state.id {
            return Err(Error::new(
                "sandbox was replaced; refusing to attach to a different instance",
            ));
        }
        if policy
            && digest(
                remote
                    .get("policy")
                    .ok_or_else(|| Error::new("sandbox response is missing policy"))?,
            )? != state.observed_policy
        {
            return Err(Error::new(
                "effective backend policy changed; inspect it before further execution",
            ));
        }
        if ready && Self::remote_str(&remote, "phase")? != "Ready" {
            return Err(Error::new(
                "sandbox is not Ready; inspect status or start it explicitly",
            ));
        }
        Ok(remote)
    }
    fn create(&self) -> Result<i32> {
        if self.config.state_path().exists() {
            self.checked(false, true)?;
            println!("Environment already exists; no changes made.");
            return Ok(0);
        }
        let dir = tempfile::Builder::new()
            .prefix("create-")
            .tempdir_in(self.config.path.parent().expect("config parent"))?;
        let policy_path = dir.path().join("policy.json");
        write_new(&policy_path, &self.config.policy)?;
        self.capture(&create_arguments(self.config, &policy_path), true)?;
        let remote = self.get()?;
        self.check_identity(&remote)?;
        if Self::remote_str(&remote, "phase")? != "Ready"
            || Self::remote_str(&remote, "policy_source")? != "sandbox"
        {
            return Err(Error::new(
                "created sandbox is not Ready with a sandbox policy; inspect it before use",
            ));
        }
        let policy = remote
            .get("policy")
            .ok_or_else(|| Error::new("created sandbox did not return an effective policy"))?;
        validate_policy(policy)?;
        write_new(
            &self.config.state_path(),
            &RuntimeState {
                schema: 1,
                id: Self::remote_str(&remote, "id")?.to_owned(),
                config: self.config.fingerprint.clone(),
                observed_policy: digest(policy)?,
            },
        )?;
        println!(
            "Created {}. Project upload is a separate action.",
            self.config.values.name
        );
        Ok(0)
    }
    fn execute(&self, command: &[String], tty: bool, workdir: &str) -> Result<i32> {
        if command.is_empty() || command[0].is_empty() || command.iter().any(|a| a.contains('\0')) {
            return Err(Error::new("an executable and valid arguments are required"));
        }
        self.checked(true, true)?;
        let mut args = vec![
            "sandbox".into(),
            "exec".into(),
            "--name".into(),
            self.config.values.name.clone(),
            "--workdir".into(),
            workdir.into(),
        ];
        if tty {
            args.push("--tty".into());
        }
        args.push("--".into());
        args.extend(command.iter().cloned());
        self.status(&args)
    }
    fn upload(&self) -> Result<i32> {
        if !self.config.values.project.is_dir() {
            return Err(Error::new("the host project must exist for upload"));
        }
        self.checked(true, true)?;
        preflight_upload(&self.config.values.project, &self.config.values.project)?;
        self.status(&[
            "sandbox".into(),
            "upload".into(),
            self.config.values.name.clone(),
            self.config.values.project.to_string_lossy().into_owned(),
            GUEST_PROJECT.into(),
        ])
    }
    fn connect_vscode(&self) -> Result<i32> {
        self.checked(true, true)?;
        self.status(&[
            "sandbox".into(),
            "connect".into(),
            self.config.values.name.clone(),
            "--editor".into(),
            "vscode".into(),
        ])
    }
    fn ssh_config(&self) -> Result<i32> {
        self.checked(true, true)?;
        self.status(&[
            "sandbox".into(),
            "ssh-config".into(),
            self.config.values.name.clone(),
        ])
    }
    fn lifecycle(&self, action: &str) -> Result<i32> {
        match action {
            "start" => {
                self.checked(false, true)?;
            }
            "stop" => {
                self.checked(false, false)?;
            }
            _ => return Err(Error::new("unsupported lifecycle action")),
        }
        self.status(&[
            "sandbox".into(),
            action.into(),
            self.config.values.name.clone(),
        ])
    }
}
fn preflight_upload(path: &Path, root: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_symlink() {
            let target = path
                .canonicalize()
                .map_err(|_| Error::new("upload contains an invalid or looping symlink"))?;
            if !target.starts_with(root) {
                return Err(Error::new(
                    "upload contains a symlink outside the selected project",
                ));
            }
        } else if kind.is_dir() {
            preflight_upload(&path, root)?;
        } else if !kind.is_file() {
            return Err(Error::new(
                "upload contains a socket, device, or other special file",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_is_vendor_scoped() {
        assert!(Cli::try_parse_from(["dungeon", "auth", "claude"]).is_ok());
        assert!(Cli::try_parse_from(["dungeon", "auth", "codex"]).is_ok());
        assert!(Cli::try_parse_from(["dungeon", "auth", "grok"]).is_err());
        assert!(Cli::try_parse_from(["dungeon", "auth", "opencode"]).is_err());
    }
    #[test]
    fn duplicate_json_keys_are_rejected() {
        assert!(decode_json(r#"{"schema":1,"schema":2}"#).is_err());
    }
    #[test]
    fn names_reject_injection_shapes() {
        assert!(safe_name("demo", "name").is_ok());
        for value in ["--privileged", "../host", "UPPER", "bad_name", ""] {
            assert!(safe_name(value, "name").is_err());
        }
    }
    #[test]
    fn policy_requires_hard_landlock() {
        let mut policy = starter_policy(Profile::Minimal);
        policy["landlock"]["compatibility"] = json!("best_effort");
        assert!(validate_policy(&policy).is_err());
    }
    #[test]
    fn create_arguments_do_not_upload_or_inject_env() {
        let config = Config {
            path: PathBuf::from("/tmp/dungeon.json"),
            values: ConfigDoc {
                schema: 1,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project: PathBuf::from("/tmp/project"),
                image: DEFAULT_IMAGE.into(),
                agent: Agent::Opencode,
                providers: vec![],
                policy: "policy.json".into(),
            },
            policy: starter_policy(Profile::Minimal),
            policy_path: PathBuf::from("/tmp/policy.json"),
            fingerprint: "fingerprint".into(),
        };
        let args = create_arguments(&config, &config.policy_path);
        assert!(args.iter().any(|a| a == "--no-auto-providers"));
        assert!(!args.iter().any(|a| a == "--upload"));
        assert!(!args.iter().any(|a| a == "--env"));
    }
}
