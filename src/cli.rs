use crate::binding;
use crate::config::{
    Agent, Config, ConfigDoc, HarnessMode, ProjectTransfer, canonical_project, create_private_dir,
    load_config, load_config_data, write_new,
};
use crate::openshell::{GUEST_PROJECT, Runtime, plan_create_arguments, project_transfer_plan};
use crate::policy::starter_policy;
use crate::{Error, OPEN_SHELL_VERSION, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_IMAGE: &str = "localhost/dungeon-agent:0.0.3-opencode-2.1.2";
const OPENCODE_TAGS: &str = "https://registry.npmjs.org/-/package/@opencode%2Fcli/dist-tags";

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
    about = "Manage agent environments with OpenShell",
    after_help = "Quick start (after dungeon setup):\n  dungeon init                Review and save private project configuration\n  dungeon                     Attach host OpenCode to guest service (configured OpenCode)\n  dungeon run opencode        Run the guest OpenCode TUI instead\n  dungeon run codex           Run another agent in the same environment\n  dungeon exec -- cargo test  Run an image-provided tool\n  dungeon export              Review guest changes (copy mode)\n  dungeon down                Stop, preserving guest state\n  dungeon list                List bindings in/under this directory (offline)\n  dungeon list -a             List all private bindings (offline)\n  dungeon destroy             Delete a stopped bound sandbox after confirmation\n\nUse dungeon config --path to find your private configuration.\nAutomation: dungeon machine --help",
    subcommand_precedence_over_arg = true
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "explicit private dungeon.json; no project discovery"
    )]
    config: Option<PathBuf>,
    #[arg(
        value_name = "PROJECT",
        help = "project directory for direct launch; defaults to the current directory"
    )]
    project: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Delete local sandboxes, gateway, private Dungeon state and the user-local binary.
    Uninstall {
        /// Inspect the removal scope without changing files or containers.
        #[arg(long, conflicts_with = "yes")]
        dry_run: bool,
        /// Approve permanent loss of guest files, sessions, auth and private configuration.
        #[arg(long)]
        yes: bool,
        /// User-local installation directory (default: ~/.local/bin).
        #[arg(long)]
        bin_dir: Option<PathBuf>,
    },
    /// Set up or verify the supported local OpenShell gateway (Linux/rootless Podman).
    Setup {
        /// Enable project bind capability on a fresh gateway (the default); kept for explicit scripts.
        #[arg(long, conflicts_with = "copy_only")]
        enable_live: bool,
        /// Create a copy-only gateway without bind capability (fresh setup only).
        #[arg(long)]
        copy_only: bool,
        /// Inspect prerequisites and print the capability diff without creating state.
        #[arg(long)]
        dry_run: bool,
    },
    /// Read-only prerequisite and capability diagnostics; never initializes an environment.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// List private project bindings in/under the current directory (not backend inventory).
    List {
        /// Include bindings outside the current directory.
        #[arg(short = 'a', long = "all")]
        all: bool,
    },
    /// Show the reviewed OpenCode image version and host client; optionally check upstream stable.
    Versions {
        /// Query the official npm registry's stable dist-tag (read-only, bounded).
        #[arg(long)]
        check: bool,
    },
    /// Review and save private configuration; never starts a sandbox or uploads files.
    Init {
        #[arg(long, help = "private config directory (default: the project binding)")]
        directory: Option<PathBuf>,
        #[arg(long, default_value = ".", help = "host project directory")]
        project: PathBuf,
        #[arg(long, value_enum, default_value = "live")]
        project_transfer: ProjectTransfer,
        #[arg(long, help = "sandbox name (default: generated from the project path)")]
        name: Option<String>,
        #[arg(long, help = "registered OpenShell gateway (default: dungeon-local)")]
        gateway: Option<String>,
        #[arg(long, default_value = "default")]
        workspace: String,
        #[arg(long, default_value = DEFAULT_IMAGE)]
        image: String,
        #[arg(long, value_enum, default_value = "opencode")]
        agent: Agent,
        #[arg(
            long = "provider",
            help = "existing OpenShell provider name; repeatable, never a secret"
        )]
        providers: Vec<String>,
        /// OpenCode host configuration access; existing bindings are never changed.
        #[arg(long, value_enum, default_value = "read-only")]
        harness_config: HarnessMode,
        /// Omit legacy auth and whole OpenCode JSON configs that can contain credentials.
        #[arg(long)]
        exclude_provider_auth: bool,
        /// Exclude an exact config-relative path/subtree (repeatable, not a glob).
        #[arg(long = "harness-exclude")]
        harness_exclude: Vec<String>,
        #[arg(
            long,
            visible_alias = "non-interactive",
            help = "approve writing configuration without prompting"
        )]
        yes: bool,
    },
    /// Show the resolved private configuration and policy paths; no runtime contact.
    Config {
        #[arg(long, help = "print only the dungeon.json path")]
        path: bool,
    },
    /// Show the policy and creation plan without writing files or contacting OpenShell.
    Plan,
    /// Review/reselect unbound harness files or capture a copy; never restarts active work.
    Reload {
        #[arg(long)]
        yes: bool,
    },
    /// Capture and approve the explicitly declared Neovim snapshot (no runtime changes).
    Imports {
        #[command(subcommand)]
        command: ImportCommands,
    },
    /// Create a sandbox without the initial project upload (advanced).
    Create,
    /// Create or resume the sandbox; live mounts directly, copy uploads once.
    Up,
    /// Stop the sandbox, preserving guest files, sessions and authentication.
    Down,
    /// Delete a stopped bound sandbox and retire its runtime state; keep project configuration.
    Destroy {
        /// Approve deletion without the interactive name confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Inspect the bound sandbox and pending initial-upload state.
    Status,
    /// Start an existing sandbox without uploading (also available during recovery).
    Start,
    /// Stop the sandbox; same as down.
    Stop,
    /// Open Bash inside an already Ready sandbox at /sandbox/project.
    Shell,
    /// Attach host VS Code to an already Ready sandbox.
    Code,
    /// Print native SSH configuration for the bound sandbox.
    SshConfig,
    /// Own one interactive foreground command over native SSH (Linux only).
    Ssh {
        #[arg(long, required = true)]
        owned: bool,
        #[arg(last = true, required = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Forward a guest-loopback service to host loopback (foreground; Ctrl-C to stop).
    Forward {
        #[arg(long, help = "service port inside the guest")]
        port: NonZeroU16,
        #[arg(long, help = "host loopback port (defaults to --port)")]
        local_port: Option<NonZeroU16>,
    },
    /// Start a guest OpenCode service and attach the host OpenCode UI (foreground).
    Attach,
    /// Explicitly refresh the guest project; may overwrite guest edits (copy mode).
    Upload {
        #[arg(long)]
        yes: bool,
    },
    /// Stage and review guest changes before applying them to the host (copy mode).
    Export {
        #[arg(long)]
        yes: bool,
    },
    /// Start vendor-native login inside an already Ready sandbox; no host credential copying.
    Auth {
        #[arg(value_enum)]
        agent: AuthAgent,
    },
    /// Run an agent (default: configured agent); resume a stopped sandbox without uploading.
    Run {
        #[arg(value_enum)]
        agent: Option<Agent>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Run any image-provided command in an already Ready sandbox at /sandbox/project.
    Exec {
        #[arg(long)]
        tty: bool,
        #[arg(
            long,
            conflicts_with = "tty",
            help = "Own a Linux foreground guest command; no stdin; bounded shutdown"
        )]
        owned: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        arguments: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ImportCommands {
    Stage {
        #[arg(long)]
        yes: bool,
    },
}

pub(crate) fn run() -> Result<i32> {
    dispatch(Cli::parse())
}

fn dispatch(cli: Cli) -> Result<i32> {
    let Cli {
        config,
        project,
        command,
    } = cli;

    match command {
        Some(Commands::Uninstall {
            dry_run,
            yes,
            bin_dir,
        }) => {
            if config.is_some() || project.is_some() {
                return Err(Error::new(
                    "uninstall accepts no PROJECT or --config; it removes the local installation",
                ));
            }
            crate::openshell::uninstall(bin_dir, dry_run, yes)
        }
        Some(Commands::List { all }) => {
            if config.is_some() || project.is_some() {
                return Err(Error::new(
                    "list reads private project bindings; it accepts no PROJECT or --config",
                ));
            }
            let entries = if all {
                binding::list()?
            } else {
                binding::list_scoped(&env::current_dir()?.canonicalize()?)?
            };
            println!("{}", serde_json::to_string_pretty(&entries)?);
            Ok(0)
        }
        Some(Commands::Versions { check }) => {
            if project.is_some() {
                return Err(Error::new(
                    "versions accepts --config or the current project binding, not PROJECT",
                ));
            }
            versions(check, config.as_deref())
        }
        Some(Commands::Doctor { json }) => {
            if project.is_some() {
                return Err(Error::new(
                    "doctor accepts --config or the existing current-directory binding, not PROJECT",
                ));
            }
            crate::openshell::doctor(config, json)
        }
        Some(Commands::Setup {
            enable_live,
            copy_only,
            dry_run,
        }) => {
            if project.is_some() {
                return Err(Error::new(
                    "PROJECT is only accepted for direct launch; setup has no project binding",
                ));
            }
            crate::openshell::setup(enable_live || !copy_only, dry_run)
        }
        Some(Commands::Init {
            directory,
            project: init_project,
            project_transfer,
            name,
            gateway,
            workspace,
            image,
            agent,
            providers,
            harness_config,
            exclude_provider_auth,
            harness_exclude,
            yes,
        }) => {
            if config.is_some() {
                return Err(Error::new(
                    "init writes a new configuration; use --directory rather than --config",
                ));
            }
            if project.is_some() {
                return Err(Error::new(
                    "PROJECT is only accepted for direct launch; use init --project PATH",
                ));
            }
            initialize(
                directory,
                init_project,
                project_transfer,
                name,
                gateway,
                workspace,
                image,
                agent,
                providers,
                harness_config,
                exclude_provider_auth,
                harness_exclude,
                yes,
            )
        }
        Some(Commands::Config { path }) => {
            if project.is_some() {
                return Err(Error::new(
                    "run config from the project directory or use --config",
                ));
            }
            let config = resolve_environment(config, None, false, None)?;
            if path {
                println!("{}", config.path.display());
            } else {
                println!(
                    "{}",
                    crate::config::review_json(&json!({
                        "config": config.path,
                        "policy": config.policy_path,
                        "values": config.values,
                        "project_transfer": config.values.project_transfer,
                    }))?
                );
            }
            Ok(0)
        }
        Some(Commands::Plan) => {
            if project.is_some() {
                return Err(Error::new(
                    "PROJECT is only accepted for direct launch; run plan from that directory or use --config",
                ));
            }
            let config = resolve_environment(config, None, false, None)?;
            print_plan(&config)
        }
        None => {
            let config = resolve_environment(config, project.as_deref(), true, None)?;
            launch_default(&config)
        }
        Some(command) => {
            if project.is_some() {
                return Err(Error::new(
                    "PROJECT is only accepted for direct launch; run the subcommand from that directory or use --config",
                ));
            }
            let create_binding = matches!(
                &command,
                Commands::Create | Commands::Up | Commands::Run { .. } | Commands::Attach
            );
            let first_agent = match &command {
                Commands::Run { agent, .. } => *agent,
                _ => None,
            };
            let config = resolve_environment(config, None, create_binding, first_agent)?;
            run_command(&config, command)
        }
    }
}

fn resolve_environment(
    explicit: Option<PathBuf>,
    project: Option<&Path>,
    create_if_missing: bool,
    first_agent: Option<Agent>,
) -> Result<Config> {
    if let Some(path) = explicit {
        if project.is_some() {
            return Err(Error::new(
                "do not combine positional PROJECT with --config; the explicit config is authoritative",
            ));
        }
        return load_config(&path);
    }

    let project = canonical_project(project.unwrap_or_else(|| Path::new(".")))?;
    if let Some(config) = binding::load(&project)? {
        return Ok(config);
    }
    if !create_if_missing {
        return Err(Error::new(format!(
            "no private Dungeon environment is bound to {}; run `dungeon init` from that project first (or `dungeon init --yes` for automation)",
            project.display()
        )));
    }
    create_project_binding(&project, first_agent.unwrap_or(Agent::Opencode))
}

fn create_project_binding(project: &Path, agent: Agent) -> Result<Config> {
    let path = binding::config_path(project)?;
    let directory = path.parent().expect("binding config parent").to_path_buf();
    let values = ConfigDoc {
        schema: 2,
        name: binding::generated_name(project)?,
        gateway: "dungeon-local".into(),
        workspace: "default".into(),
        project: project.to_path_buf(),
        project_transfer: ProjectTransfer::Live,
        image: DEFAULT_IMAGE.into(),
        agent,
        providers: vec![],
        imports: vec![],
        harness: crate::imports::harness::defaults(
            agent,
            HarnessMode::ReadOnly,
            false,
            vec![],
            project,
            &path,
        )?,
    };
    approve_environment(&directory, &values, false)?;
    write_environment(directory, values)
}

fn print_plan(config: &Config) -> Result<i32> {
    let imports = crate::imports::plan(config)?;
    let mut unsatisfied = Vec::new();
    if !config.values.imports.is_empty() {
        unsatisfied.push("imports require approved unchanged snapshot and exact local bind-enabled UID998 gateway; offline plan is not runtime verification");
    }
    if crate::imports::harness::enabled(config) {
        unsatisfied.push("harness imports require Linux and the exact local bind-enabled UID998 gateway; OpenCode database authentication is not shared; copy requires reload approval before up");
    }
    if config.values.project_transfer == ProjectTransfer::Live {
        unsatisfied.push(
            "live project transfer requires trusted Podman bind-mount and keep-id gateway configuration; plan does not verify it",
        );
    }
    let mut command = vec![
        "openshell".into(),
        "--gateway".into(),
        config.values.gateway.clone(),
        "--workspace".into(),
        config.values.workspace.clone(),
    ];
    command.extend(plan_create_arguments(config, &config.policy_path));
    println!(
        "{}",
        crate::config::review_json(&json!({
            "adapter": "openshell",
            "version": OPEN_SHELL_VERSION,
            "environment": {
                "name": config.values.name,
                "config": config.path,
                "project": config.values.project,
            },
            "fingerprint": config.fingerprint,
            "command": command,
            "policy": config.policy,
            "imports": imports,
            "harness": crate::imports::harness::plan(config),
            "tool_requirements": if config.values.imports.is_empty() { vec![] } else { vec!["nvim (reviewed image; not verified by plan)"] },
            "guest_private_state": if config.values.imports.is_empty() { vec![] } else { vec!["/sandbox/.local/share/nvim", "/sandbox/.local/state/nvim", "/sandbox/.cache/nvim"] },
            "provider_requests": config.values.providers,
            "unsatisfied": unsatisfied,
            "project_transfer": project_transfer_plan(config),
            "verification": "plan only; no backend contacted or enforcement verified"
        }))?
    );
    Ok(0)
}

fn launch_default(config: &Config) -> Result<i32> {
    crate::imports::verify(config)?;
    if config.values.agent == Agent::Opencode {
        require_attach_terminal()?;
    }
    let runtime = Runtime::new(config)?;
    if config.values.agent == Agent::Opencode {
        return attach_opencode(&runtime);
    }
    let code = runtime.up()?;
    if code != 0 {
        return Ok(code);
    }
    runtime.run_agent(
        &[config.values.agent.as_str().to_owned()],
        io::stdin().is_terminal() && io::stdout().is_terminal(),
    )
}

fn run_command(config: &Config, command: Commands) -> Result<i32> {
    if let Commands::Reload { yes } = command {
        return crate::imports::harness::reload(config, yes);
    }
    if let Commands::Imports {
        command: ImportCommands::Stage { yes },
    } = command
    {
        return crate::imports::stage(config, yes);
    }
    if !matches!(
        &command,
        Commands::Down | Commands::Stop | Commands::Status | Commands::Destroy { .. }
    ) {
        crate::imports::verify(config)?;
    }
    if config.values.project_transfer == ProjectTransfer::Live {
        match &command {
            Commands::Upload { .. } => {
                return Err(Error::new(
                    "upload is unavailable in live project-transfer mode; the host project is requested directly at /sandbox/project",
                ));
            }
            Commands::Export { .. } => {
                return Err(Error::new(
                    "export is unavailable in live project-transfer mode; live project changes are written directly to the host project",
                ));
            }
            _ => {}
        }
    }
    if matches!(&command, Commands::Upload { yes: false }) {
        return Err(Error::new(
            "review project contents, including secrets and symlinks, then repeat upload --yes",
        ));
    }
    if matches!(&command, Commands::Attach) {
        require_attach_terminal()?;
    }

    let runtime = Runtime::new(config)?;
    match command {
        Commands::Create => runtime.create(),
        Commands::Up => runtime.up(),
        Commands::Down | Commands::Stop => runtime.down(),
        Commands::Destroy { yes } => {
            let remote = runtime.inspect_for_destroy()?;
            if remote["phase"] != "Stopped" {
                return Err(Error::new(
                    "stop the bound sandbox with `dungeon down` before destroy",
                ));
            }
            let name = &config.values.name;
            println!(
                "Delete stopped sandbox {name} ({}) on gateway {} / workspace {}? Guest files, auth and sessions will be lost. Private configuration is retained.",
                remote["id"].as_str().unwrap_or("unknown"),
                config.values.gateway,
                config.values.workspace
            );
            if !yes {
                if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                    return Err(Error::new(
                        "destroy requires a terminal or explicit --yes; nothing deleted",
                    ));
                }
                print!("Type the sandbox name `{name}` to confirm: ");
                io::stdout().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                if answer.trim_end_matches(['\r', '\n']) != name {
                    return Err(Error::new("destroy cancelled; nothing deleted"));
                }
            }
            runtime.destroy()
        }
        Commands::Status => {
            let remote = runtime.inspect_bound()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "id": remote["id"],
                    "name": remote["name"],
                    "workspace": remote["workspace"],
                    "phase": remote["phase"],
                    "initial_upload_pending": remote["initial_upload_pending"],
                }))?
            );
            Ok(0)
        }
        Commands::Upload { yes: true } => runtime.upload(),
        Commands::Export { yes } => runtime.export(yes),
        Commands::Auth {
            agent: AuthAgent::Codex,
        } => {
            eprintln!(
                "Starting Codex device authentication inside the guest. Complete the displayed URL/code in your browser. Dungeon never reads or stores the resulting credential."
            );
            runtime.execute(
                &["codex".into(), "login".into(), "--device-auth".into()],
                true,
                "/sandbox",
            )
        }
        Commands::Auth {
            agent: AuthAgent::Claude,
        } => {
            eprintln!(
                "Starting Claude Code inside the guest. Use /login and complete the vendor browser flow. Dungeon never reads or stores the resulting credential."
            );
            runtime.execute(&["claude".into()], true, "/sandbox")
        }
        Commands::Run { agent, arguments } => {
            let selected = agent.unwrap_or(config.values.agent);
            if !config.state_path().exists() {
                runtime.up()?;
            }
            let mut command = vec![selected.as_str().to_owned()];
            command.extend(arguments);
            runtime.run_agent(
                &command,
                io::stdin().is_terminal() && io::stdout().is_terminal(),
            )
        }
        Commands::Exec {
            tty,
            owned,
            arguments,
        } => {
            if owned {
                runtime.execute_owned(&arguments)
            } else {
                runtime.execute(&arguments, tty, GUEST_PROJECT)
            }
        }
        Commands::Shell => runtime.execute(&["/bin/bash".into(), "-l".into()], true, GUEST_PROJECT),
        Commands::Code => runtime.connect_vscode(),
        Commands::SshConfig => runtime.ssh_config(),
        Commands::Ssh { arguments, .. } => runtime.ssh_owned(&arguments),
        Commands::Forward { port, local_port } => runtime.forward_service(port, local_port),
        Commands::Attach => attach_opencode(&runtime),
        Commands::Start => runtime.start(),
        Commands::Init { .. }
        | Commands::Config { .. }
        | Commands::Setup { .. }
        | Commands::Uninstall { .. }
        | Commands::Doctor { .. }
        | Commands::List { .. }
        | Commands::Versions { .. }
        | Commands::Imports { .. }
        | Commands::Reload { .. }
        | Commands::Plan
        | Commands::Upload { yes: false } => unreachable!(),
    }
}

fn attach_opencode(runtime: &Runtime<'_>) -> Result<i32> {
    require_attach_terminal()?;
    #[cfg(target_os = "linux")]
    {
        if runtime.up()? != 0 {
            return Err(Error::new("sandbox did not become Ready"));
        }
        crate::openshell::attach_opencode(runtime)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = runtime;
        Err(Error::new("OpenCode attachment currently requires Linux"))
    }
}

fn require_attach_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(Error::new(
            "OpenCode attachment requires a terminal; use `dungeon run opencode` for guest CLI execution",
        ));
    }
    Ok(())
}

fn stable_version(text: &str) -> Option<[u64; 3]> {
    let mut parts = text.split('.');
    let parse = |part: Option<&str>| {
        let part = part?;
        (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    };
    let version = [
        parse(parts.next())?,
        parse(parts.next())?,
        parse(parts.next())?,
    ];
    parts.next().is_none().then_some(version)
}

fn versions(check: bool, config: Option<&Path>) -> Result<i32> {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../images/agent/package.json"))?;
    let reviewed = manifest["dependencies"]["@opencode/cli"]
        .as_str()
        .ok_or_else(|| Error::new("reviewed image has no OpenCode version"))?;
    let reviewed_number = stable_version(reviewed)
        .ok_or_else(|| Error::new("reviewed image has an invalid OpenCode version"))?;
    let project = env::current_dir()?.canonicalize()?;
    let environment = crate::openshell::host_environment(Some(&project))?;
    let path = environment.get(std::ffi::OsStr::new("PATH"));

    let host = crate::openshell::find_executable("opencode", path)
        .and_then(|binary| {
            Command::new(binary)
                .arg("--version")
                .env_clear()
                .envs(&environment)
                .output()
                .ok()
        })
        .filter(|output| output.status.success() && output.stdout.len() <= 128)
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().strip_prefix("opencode v").map(str::to_owned))
        .filter(|version| stable_version(version).is_some());
    let latest = if check {
        let binary = crate::openshell::find_executable("curl", path).ok_or_else(|| {
            Error::new("curl is required for the explicit registry version check")
        })?;
        let output = Command::new(binary)
            .args([
                "--disable",
                "--fail",
                "--silent",
                "--show-error",
                "--max-time",
                "5",
                "--max-filesize",
                "65536",
                OPENCODE_TAGS,
            ])
            .env_clear()
            .envs(&environment)
            .output()
            .map_err(|error| {
                Error::new(format!("cannot check stable OpenCode release: {error}"))
            })?;
        if !output.status.success() || output.stdout.len() > 65536 {
            return Err(Error::new(
                "OpenCode release check failed; image and host unchanged",
            ));
        }
        let tags: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let latest = tags["latest"]
            .as_str()
            .filter(|value| stable_version(value).is_some())
            .ok_or_else(|| Error::new("registry stable dist-tag is invalid"))?;
        Some(latest.to_owned())
    } else {
        None
    };
    let selected = if let Some(path) = config {
        Some(load_config(path)?.values.image)
    } else {
        binding::load(&project)?.map(|config| config.values.image)
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "host_opencode": host,
            "reviewed_guest_opencode": reviewed,
            "reviewed_local_image": DEFAULT_IMAGE,
            "selected_project_image": selected,
            "registry_stable": latest,
            "reviewed_update_available": latest.as_deref().and_then(stable_version).is_some_and(|version| version > reviewed_number),
            "selection": "new bindings use the reviewed versioned local image; existing bindings unchanged; no automatic pull, install or migration",
        }))?
    );
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
fn initialize(
    directory: Option<PathBuf>,
    project: PathBuf,
    project_transfer: ProjectTransfer,
    name: Option<String>,
    gateway: Option<String>,
    workspace: String,
    image: String,
    agent: Agent,
    providers: Vec<String>,
    harness_config: HarnessMode,
    exclude_provider_auth: bool,
    harness_exclude: Vec<String>,
    yes: bool,
) -> Result<i32> {
    let project = canonical_project(&project)?;
    let project_binding = directory.is_none();
    let directory = match directory {
        Some(directory) => directory,
        None => binding::config_path(&project)?
            .parent()
            .expect("binding config parent")
            .to_path_buf(),
    };
    let name = name.unwrap_or(binding::generated_name(&project)?);
    let gateway = gateway.unwrap_or_else(|| "dungeon-local".to_owned());
    let harness = crate::imports::harness::defaults(
        agent,
        harness_config,
        exclude_provider_auth,
        harness_exclude,
        &project,
        &directory.join("dungeon.json"),
    )?;
    let values = ConfigDoc {
        schema: 2,
        name,
        gateway,
        workspace,
        project,
        project_transfer,
        image,
        agent,
        providers,
        imports: vec![],
        harness,
    };
    approve_environment(&directory, &values, yes)?;
    let config = write_environment(directory, values)?;

    println!("Configuration: {:?}", config.path);
    println!("No runtime was started. Review policy.json and named providers before first up.");
    if config.values.image == DEFAULT_IMAGE {
        println!(
            "Build the reviewed local image before first up (images/agent/README.md). `dungeon versions --check` shows upstream stable updates; nothing is downloaded automatically."
        );
    }
    if project_binding {
        println!("From the project directory: dungeon plan, then dungeon up.");
    } else {
        println!("Next: run plan, then up, using --config with the path above.");
    }
    Ok(0)
}

fn approve_environment(directory: &Path, values: &ConfigDoc, yes: bool) -> Result<()> {
    let config_path = if directory.is_absolute() {
        directory.join("dungeon.json")
    } else {
        env::current_dir()?.join(directory).join("dungeon.json")
    };
    load_config_data(config_path, serde_json::to_value(values)?, |path| {
        Ok((path.to_path_buf(), starter_policy()))
    })?;
    println!(
        "{}",
        crate::config::review_json(&json!({
            "config": directory.join("dungeon.json"),
            "environment": values,
            "project_transfer": values.project_transfer,
        }))?
    );
    match values.project_transfer {
        ProjectTransfer::Copy => println!(
            "Copy mode: first up uploads the project; guest changes reach the host only through reviewed export. No writable host mounts."
        ),
        ProjectTransfer::Live => println!(
            "Live mode: the guest can read and WRITE the selected host project immediately at /sandbox/project. Only this project is writable; separately reviewed harness files may be read-only. No upload/export. Requires a bind-enabled gateway. Choose --project-transfer copy for reviewed return of changes."
        ),
    }
    println!(
        "Review project contents before first up; all files within a live project become guest-visible, including secrets. Transfer is not a secret scanner."
    );
    if let Some(h) = &values.harness {
        println!("{}", crate::imports::harness::WARNING);
        if h.mode == HarnessMode::Copy {
            println!(
                "Copy mode: run dungeon reload to capture and approve the harness snapshot before first up."
            );
        }
        if h.exclude_provider_auth {
            println!(
                "Auth exclusion omits legacy auth.json and entire opencode.json/jsonc/cli.json files. It is not a secret scanner for plugins or other content."
            );
        }
    }
    if !yes {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(Error::new(
                "initialization requires a terminal or explicit `dungeon init --yes`; no files were created",
            ));
        }
        println!(
            "To customize, cancel and use dungeon init --agent/--image/--gateway/--project-transfer."
        );
        if !confirm("Save this private configuration and continue?")? {
            return Err(Error::new(
                "project environment initialization cancelled; no files were created",
            ));
        }
    }
    Ok(())
}

pub(crate) fn write_environment(directory: PathBuf, values: ConfigDoc) -> Result<Config> {
    let directory = if directory.is_absolute() {
        directory
    } else {
        env::current_dir()?.join(directory)
    };
    if directory.exists() {
        return Err(Error::new(
            "use a new configuration directory; existing directories are never overwritten",
        ));
    }
    let ancestor = directory
        .ancestors()
        .find(|path| path.exists())
        .ok_or_else(|| Error::new("configuration directory has no existing ancestor"))?;
    let suffix = directory
        .strip_prefix(ancestor)
        .expect("directory ancestor");
    if suffix
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(Error::new(
            "use a configuration directory without traversal through missing directories",
        ));
    }
    let directory = ancestor.canonicalize()?.join(suffix);

    let policy = starter_policy();
    load_config_data(
        directory.join("dungeon.json"),
        serde_json::to_value(&values)?,
        |path| Ok((path.to_path_buf(), policy.clone())),
    )?;
    create_private_dir(&directory)?;
    write_new(&directory.join("policy.json"), &policy)?;
    write_new(&directory.join("dungeon.json"), &values)?;
    load_config(&directory.join("dungeon.json"))
}

fn confirm(label: &str) -> Result<bool> {
    print!("{label} [y/N]: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_launch_accepts_current_or_explicit_project_and_subcommands_keep_precedence() {
        let current = Cli::try_parse_from(["dungeon"]).unwrap();
        assert!(current.project.is_none());
        assert!(current.command.is_none());

        let explicit = Cli::try_parse_from(["dungeon", "../project"]).unwrap();
        assert_eq!(explicit.project, Some(PathBuf::from("../project")));
        assert!(explicit.command.is_none());

        let up = Cli::try_parse_from(["dungeon", "up"]).unwrap();
        assert!(up.project.is_none());
        assert!(matches!(up.command, Some(Commands::Up)));

        let configured =
            Cli::try_parse_from(["dungeon", "--config", "/private/dungeon.json"]).unwrap();
        assert_eq!(
            configured.config,
            Some(PathBuf::from("/private/dungeon.json"))
        );
        assert!(configured.command.is_none());
    }

    #[test]
    fn first_run_selects_an_agent_without_accepting_unreviewed_options() {
        let parsed = Cli::try_parse_from(["dungeon", "run", "claude"]).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::Run {
                agent: Some(Agent::Claude),
                ..
            })
        ));
        let parsed = Cli::try_parse_from(["dungeon", "run", "codex"]).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::Run {
                agent: Some(Agent::Codex),
                ..
            })
        ));
        assert!(Cli::try_parse_from(["dungeon", "run", "--project-transfer", "live"]).is_err());
    }

    #[test]
    fn explicit_config_resolution_is_authoritative() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let private = root.path().join("private");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&private).unwrap();
        let project = project.canonicalize().unwrap();
        write_new(&private.join("policy.json"), &starter_policy()).unwrap();
        let path = private.join("dungeon.json");
        write_new(
            &path,
            &ConfigDoc {
                schema: 2,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project: project.clone(),
                project_transfer: ProjectTransfer::Copy,
                image: "localhost/test:1".into(),
                agent: Agent::Opencode,
                providers: vec![],
                imports: vec![],
                harness: None,
            },
        )
        .unwrap();

        let resolved = resolve_environment(Some(path.clone()), None, false, None).unwrap();
        assert_eq!(resolved.path, path);
        assert_eq!(resolved.values.project, project);

        let error =
            resolve_environment(Some(resolved.path.clone()), Some(root.path()), false, None)
                .unwrap_err()
                .to_string();
        assert!(
            error.contains("explicit config is authoritative"),
            "{error}"
        );
    }

    #[test]
    fn auth_is_vendor_scoped() {
        assert!(Cli::try_parse_from(["dungeon", "auth", "claude"]).is_ok());
        assert!(Cli::try_parse_from(["dungeon", "auth", "codex"]).is_ok());
        assert!(Cli::try_parse_from(["dungeon", "auth", "grok"]).is_err());
        assert!(Cli::try_parse_from(["dungeon", "auth", "opencode"]).is_err());
    }

    #[test]
    fn owned_execution_is_explicit_and_non_tty() {
        assert!(Cli::try_parse_from(["dungeon", "ssh", "--owned", "--", "nvim", "a b"]).is_ok());
        assert!(Cli::try_parse_from(["dungeon", "ssh", "--", "nvim"]).is_err());
        assert!(Cli::try_parse_from(["dungeon", "ssh", "--owned"]).is_err());
        assert!(Cli::try_parse_from(["dungeon", "ssh", "--owned", "nvim"]).is_err());
        assert!(Cli::try_parse_from(["dungeon", "exec", "--owned", "--", "node", "-v"]).is_ok());
        assert!(
            Cli::try_parse_from(["dungeon", "exec", "--owned", "--tty", "--", "true"]).is_err()
        );
        let generic = Cli::try_parse_from(["dungeon", "exec", "--", "true"]).unwrap();
        assert!(matches!(
            generic.command,
            Some(Commands::Exec {
                owned: false,
                tty: false,
                ..
            })
        ));
    }

    #[test]
    fn forwarding_accepts_only_nonzero_ports_not_addresses_or_background_mode() {
        let cli = Cli::try_parse_from([
            "dungeon",
            "forward",
            "--port",
            "4096",
            "--local-port",
            "14096",
        ])
        .unwrap();
        assert!(cli.project.is_none());
        assert!(
            matches!(cli.command, Some(Commands::Forward { port, local_port })
            if port.get() == 4096 && local_port.unwrap().get() == 14096)
        );
        let cli = Cli::try_parse_from(["dungeon", "forward", "--port", "65535"]).unwrap();
        assert!(
            matches!(cli.command, Some(Commands::Forward { port, local_port: None })
            if port.get() == 65535)
        );
        for args in [
            vec!["dungeon", "forward"],
            vec!["dungeon", "forward", "--port", "0"],
            vec!["dungeon", "forward", "--port", "65536"],
            vec!["dungeon", "forward", "--port", "-1"],
            vec!["dungeon", "forward", "--port", "127.0.0.1:4096"],
            vec!["dungeon", "forward", "--port", "4096", "--local-port", "0"],
            vec![
                "dungeon",
                "forward",
                "--port",
                "4096",
                "--local-port",
                "65536",
            ],
            vec![
                "dungeon",
                "forward",
                "--port",
                "4096",
                "--local-port",
                "0.0.0.0:4096",
            ],
            vec![
                "dungeon",
                "forward",
                "--port",
                "4096",
                "--target-host",
                "example.com",
            ],
            vec!["dungeon", "forward", "--port", "4096", "--background"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn init_has_no_profile_flag() {
        assert!(
            Cli::try_parse_from([
                "dungeon",
                "init",
                "--directory",
                "/tmp/dungeon",
                "--project",
                "/tmp/project",
                "--profile",
                "development"
            ])
            .is_err()
        );
    }

    #[test]
    fn version_report_accepts_only_stable_triplets_and_pinned_candidate() {
        assert_eq!(stable_version("2.1.2"), Some([2, 1, 2]));
        for invalid in ["2.1", "2.1.2-beta", "2.1.2.3", "2.1.02x", ""] {
            assert_eq!(stable_version(invalid), None);
        }
        assert!(DEFAULT_IMAGE.ends_with("opencode-2.1.2"));
        assert!(Cli::try_parse_from(["dungeon", "versions", "--check"]).is_ok());
        assert!(Cli::try_parse_from(["dungeon", "versions", "--install"]).is_err());
    }

    #[test]
    fn telltale_is_only_a_possible_project_path_not_a_command() {
        let cli = Cli::try_parse_from(["dungeon", "telltale"]).unwrap();
        assert_eq!(cli.project, Some(PathBuf::from("telltale")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn lifecycle_and_recovery_commands_remain_available() {
        for command in ["up", "down", "start", "stop", "create", "upload"] {
            let args = if command == "upload" {
                vec![
                    "dungeon",
                    "--config",
                    "/private/dungeon.json",
                    command,
                    "--yes",
                ]
            } else {
                vec!["dungeon", "--config", "/private/dungeon.json", command]
            };
            assert!(Cli::try_parse_from(args).is_ok(), "{command}");
        }
    }

    #[test]
    fn live_transfer_upload_and_export_refuse_before_runtime_lookup() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let private = root.path().join("private");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&private).unwrap();
        write_new(&private.join("policy.json"), &starter_policy()).unwrap();
        let path = private.join("dungeon.json");
        write_new(
            &path,
            &ConfigDoc {
                schema: 2,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project,
                project_transfer: ProjectTransfer::Live,
                image: "localhost/test:1".into(),
                agent: Agent::Opencode,
                providers: vec![],
                imports: vec![],
                harness: None,
            },
        )
        .unwrap();

        for args in [
            vec![
                "dungeon",
                "--config",
                path.to_str().unwrap(),
                "upload",
                "--yes",
            ],
            vec![
                "dungeon",
                "--config",
                path.to_str().unwrap(),
                "export",
                "--yes",
            ],
        ] {
            let error = dispatch(Cli::try_parse_from(args).unwrap())
                .unwrap_err()
                .to_string();
            assert!(error.contains("live project-transfer mode"), "{error}");
            assert!(!error.contains("OpenShell"), "{error}");
        }
    }

    #[test]
    fn init_exposes_only_the_closed_project_transfer_domain() {
        assert!(
            Cli::try_parse_from([
                "dungeon",
                "init",
                "--directory",
                "/tmp/dungeon",
                "--project",
                "/tmp/project",
                "--project-transfer",
                "live"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "dungeon",
                "init",
                "--directory",
                "/tmp/dungeon",
                "--project",
                "/tmp/project",
                "--project-transfer",
                "bind"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "dungeon",
                "init",
                "--directory",
                "/tmp/dungeon",
                "--project",
                "/tmp/project",
                "--driver-config-json",
                "{}"
            ])
            .is_err()
        );
    }

    #[test]
    fn declared_import_refuses_runtime_before_openshell_is_contacted() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let source = root.path().join("nvim");
        let private = root.path().join("private");
        for dir in [&project, &source, &private] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(source.join("init.lua"), "vim.opt.number = true").unwrap();
        write_new(&private.join("policy.json"), &starter_policy()).unwrap();
        let mut doc = serde_json::to_value(ConfigDoc {
            schema: 2,
            name: "demo".into(),
            gateway: "local".into(),
            workspace: "default".into(),
            project,
            project_transfer: ProjectTransfer::Copy,
            image: "localhost/test:1".into(),
            agent: Agent::Opencode,
            providers: vec![],
            imports: vec![],
            harness: None,
        })
        .unwrap();
        doc["imports"] = json!([{"source": source, "destination": "/sandbox/.config/nvim"}]);
        let path = private.join("dungeon.json");
        write_new(&path, &doc).unwrap();
        let expected = if cfg!(target_os = "linux") {
            "not approved"
        } else {
            "snapshot imports require Linux"
        };
        for action in ["create", "up", "run", "upload", "export", "forward"] {
            let mut args = vec!["dungeon", "--config", path.to_str().unwrap(), action];
            if matches!(action, "upload" | "export") {
                args.push("--yes");
            }
            if action == "forward" {
                args.extend(["--port", "4096"]);
            }
            assert!(
                dispatch(Cli::try_parse_from(args).unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains(expected),
                "{action}"
            );
        }
    }
}
