use crate::binding;
use crate::config::{
    Agent, Config, ConfigDoc, HarnessMode, ProjectTransfer, canonical_project, create_private_dir,
    load_config, load_config_data, write_new,
};
use crate::config::{ContainerOptions, Engine};
use crate::openshell::{GUEST_PROJECT, plan_create_arguments, project_transfer_plan};
use crate::policy::starter_policy;
use crate::runtime::{Runtime, RuntimeChoice};
use crate::{Error, OPEN_SHELL_VERSION, Result};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::Command;

const REVIEWED_IMAGE: &str = "localhost/dungeon-agent:0.0.3-opencode-2.1.2";
const OPENCODE_TAGS: &str = "https://registry.npmjs.org/-/package/@opencode%2Fcli/dist-tags";

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lower")]
enum AuthAgent {
    Claude,
    Codex,
}

#[derive(Debug, Default, clap::Args)]
struct EditOptions {
    /// Saved agent choice; invocation-only overrides remain available with run.
    #[arg(long, value_enum, requires = "edit")]
    agent: Option<Agent>,
    /// OCI image tag or digest for the first guest; never upgrades an existing guest.
    #[arg(long, requires = "edit")]
    image: Option<String>,
    /// OpenShell project access; plain containers require live.
    #[arg(long, value_enum, requires = "edit")]
    project_transfer: Option<ProjectTransfer>,
    /// OpenCode imports: reviewed copy or disabled; original-source read-only is advanced.
    #[arg(long, value_enum, requires = "edit")]
    harness_config: Option<HarnessMode>,
    /// Omit legacy auth and whole OpenCode JSON configs that may contain credentials.
    #[arg(long, requires = "edit")]
    exclude_provider_auth: bool,
    /// Replace exclusions with these exact config-relative paths/subtrees (repeatable).
    #[arg(long = "harness-exclude", requires = "edit")]
    harness_exclude: Vec<String>,
    /// Approve the displayed changes without prompting; otherwise --edit is guided.
    #[arg(long, requires = "edit")]
    yes: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "dungeon",
    version,
    about = "Manage project containers and optional OpenShell environments",
    after_help = "Quick start:\n  dungeon setup               Choose a runtime/image once per machine\n  dungeon                     First run: review, approve and launch; later runs resume\n  dungeon exec -- node --version  Run an image-provided tool\n  dungeon down                Stop, preserving guest state\n\nConfiguration:\n  dungeon init                Review/save only, without starting a guest\n  dungeon config --edit        Review changes before first guest creation\n  dungeon config --path        Find the private configuration\n  dungeon run -- --help        Pass native flags to the selected harness\n\nHarnesses: OpenCode, Claude Code or Codex; no assumed choice.\nUse dungeon COMMAND --help for advanced options.\nAutomation: dungeon setup --yes --runtime container; dungeon init --yes --agent AGENT; dungeon machine --help",
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
        /// Remove only this installed CLI; retain all configuration and runtime resources.
        #[arg(long)]
        binary_only: bool,
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
    /// Choose a runtime and image; prepare the local service and save defaults.
    Setup {
        #[arg(long, value_enum, default_value = "auto")]
        runtime: RuntimeChoice,
        #[arg(long, value_enum, default_value = "auto")]
        engine: Engine,
        #[arg(long)]
        endpoint: Option<String>,
        /// Default image for new projects; accepts a registry tag or immutable digest.
        #[arg(long)]
        image: Option<String>,
        /// Enable project bind capability on a fresh gateway (the default); kept for explicit scripts.
        #[arg(long, conflicts_with = "copy_only")]
        enable_live: bool,
        /// Create a copy-only gateway without bind capability (fresh setup only).
        #[arg(long)]
        copy_only: bool,
        /// Inspect prerequisites and print the capability diff without creating state.
        #[arg(long)]
        dry_run: bool,
        /// Approve socket activation, image pulls, gateway setup and saved defaults.
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
    },
    /// Read-only prerequisite and capability diagnostics; never initializes an environment.
    Doctor {
        #[arg(long, value_enum, default_value = "auto")]
        runtime: RuntimeChoice,
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
        #[arg(long, value_enum, default_value = "auto")]
        runtime: RuntimeChoice,
        #[arg(long, value_enum, default_value = "auto")]
        engine: Engine,
        #[arg(long, help = "local Unix socket; requires an explicit engine")]
        endpoint: Option<String>,
        #[arg(long, help = "private config directory (default: the project binding)")]
        directory: Option<PathBuf>,
        #[arg(long, default_value = ".", help = "host project directory")]
        project: PathBuf,
        #[arg(long, value_enum, help = "project transfer (default: live)")]
        project_transfer: Option<ProjectTransfer>,
        #[arg(long, help = "sandbox name (default: generated from the project path)")]
        name: Option<String>,
        #[arg(long, help = "registered OpenShell gateway (default: dungeon-local)")]
        gateway: Option<String>,
        #[arg(long, help = "OpenShell workspace (default: default)")]
        workspace: Option<String>,
        #[arg(
            long,
            help = "sandbox image tag or digest (default: setup selection, otherwise upstream base:latest)"
        )]
        image: Option<String>,
        #[arg(
            long,
            value_enum,
            help = "harness (asks once for this project if omitted)"
        )]
        agent: Option<Agent>,
        #[arg(
            long = "provider",
            help = "existing OpenShell provider name; repeatable, never a secret"
        )]
        providers: Vec<String>,
        /// OpenCode host configuration access; existing bindings are never changed.
        #[arg(
            long,
            value_enum,
            help = "OpenCode host configuration access (default: disabled)"
        )]
        harness_config: Option<HarnessMode>,
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
    /// Show private configuration, or explicitly edit an uncreated environment.
    Config {
        #[arg(
            long,
            conflicts_with = "edit",
            help = "print only the dungeon.json path"
        )]
        path: bool,
        /// Review and save changes without starting or replacing a guest.
        #[arg(long)]
        edit: bool,
        #[command(flatten)]
        options: EditOptions,
    },
    /// Show the policy and creation plan without writing files or contacting OpenShell.
    Plan,
    /// Review/reselect unbound harness files or capture a copy; never restarts active work.
    Reload {
        /// Change import mode for this unbound environment in the same approval.
        #[arg(long, value_enum)]
        harness_config: Option<HarnessMode>,
        /// Approve the displayed selection without prompting (also supports headless use).
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
        #[arg(
            value_name = "AGENT_OR_ARG",
            trailing_var_arg = true,
            allow_hyphen_values = true,
            help = "optional opencode|claude|codex override; omit it for leading native flags (run -- --version); name it for native subcommands or positional arguments (run codex exec PROMPT)"
        )]
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
    let matches = human_command().get_matches();
    dispatch(Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit()))
}

fn human_command() -> clap::Command {
    let mut command = Cli::command();
    command.build();
    let mut groups = [String::new(), String::new(), String::new()];
    for subcommand in command.get_subcommands() {
        let name = subcommand.get_name();
        let group = match name {
            "init" | "up" | "down" | "status" | "list" | "run" | "exec" | "shell" | "auth"
            | "export" => 0,
            "setup" | "doctor" | "config" | "plan" => 1,
            _ => 2,
        };
        groups[group].push_str(&format!(
            "  {name:<12}{}\n",
            subcommand
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default()
        ));
    }
    command.help_template(format!(
        "{{about-with-newline}}\n{{usage-heading}} {{usage}}\n\nEveryday commands:\n{}\nSetup and configuration:\n{}\nAdvanced and recovery:\n{}\nArguments:\n{{positionals}}\nOptions:\n{{options}}\n{{after-help}}\n",
        groups[0], groups[1], groups[2]
    ))
}

fn dispatch(cli: Cli) -> Result<i32> {
    let Cli {
        config,
        project,
        command,
    } = cli;

    match command {
        Some(Commands::Uninstall {
            binary_only,
            dry_run,
            yes,
            bin_dir,
        }) => {
            if config.is_some() || project.is_some() {
                return Err(Error::new(
                    "uninstall accepts no PROJECT or --config; it removes the local installation",
                ));
            }
            if binary_only {
                crate::runtime::uninstall_binary(bin_dir, dry_run, yes)
            } else {
                crate::runtime::uninstall(bin_dir, dry_run, yes)
            }
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
            writeln!(io::stdout(), "{}", serde_json::to_string_pretty(&entries)?)?;
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
        Some(Commands::Doctor { json, runtime }) => {
            if project.is_some() {
                return Err(Error::new(
                    "doctor accepts --config or the existing current-directory binding, not PROJECT",
                ));
            }
            crate::openshell::doctor(config, json, runtime)
        }
        Some(Commands::Setup {
            runtime,
            engine,
            endpoint,
            image,
            enable_live,
            copy_only,
            dry_run,
            yes,
        }) => {
            if project.is_some() || config.is_some() {
                return Err(Error::new(
                    "setup accepts no PROJECT or --config; it saves defaults for new projects",
                ));
            }
            setup(
                runtime,
                engine,
                endpoint,
                image,
                enable_live,
                copy_only,
                dry_run,
                yes,
            )
        }
        Some(Commands::Init {
            runtime,
            engine,
            endpoint,
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
                runtime,
                engine,
                endpoint,
            )
        }
        Some(Commands::Config {
            path,
            edit,
            options,
        }) => {
            if project.is_some() {
                return Err(Error::new(
                    "run config from the project directory or use --config",
                ));
            }
            let config = resolve_environment(config, None, false, None)?;
            if edit {
                return edit_environment(&config, options);
            }
            if path {
                writeln!(io::stdout(), "{}", config.path.display())?;
            } else {
                writeln!(
                    io::stdout(),
                    "{}",
                    crate::config::review_json(&json!({
                        "config": config.path,
                        "policy": config.policy_path,
                        "values": config.values,
                        "project_transfer": config.values.project_transfer,
                    }))?
                )?;
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
            require_launch_terminal()?;
            let config = resolve_environment(config, project.as_deref(), true, None)?;
            run_command(&config, Commands::Run { arguments: vec![] })
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
                Commands::Run { arguments } => run_selection(arguments)?.0,
                Commands::Attach => Some(Agent::Opencode),
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
            "no private Dungeon environment is bound to {}; run `dungeon init` from that project first (or `dungeon init --yes --agent opencode|claude|codex` for automation)",
            project.display()
        )));
    }
    create_project_binding(&project, select_agent(first_agent)?)
}

fn select_agent(agent: Option<Agent>) -> Result<Agent> {
    if let Some(agent) = agent {
        return Ok(agent);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::new(
            "choose a harness with `dungeon init --yes --agent opencode|claude|codex`; no files were created",
        ));
    }
    println!(
        "Choose this project's harness (saved for later launches):\n  1. OpenCode\n  2. Claude Code\n  3. Codex"
    );
    loop {
        print!("Harness [1-3 or name; no default]: ");
        io::stdout().flush()?;
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer)? == 0 {
            return Err(Error::new(
                "harness selection cancelled; no files were created",
            ));
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "1" | "opencode" => return Ok(Agent::Opencode),
            "2" | "claude" => return Ok(Agent::Claude),
            "3" | "codex" => return Ok(Agent::Codex),
            _ => println!("Enter 1, 2 or 3 (opencode, claude or codex); Ctrl-C to cancel."),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut runtime: RuntimeChoice,
    engine: Engine,
    endpoint: Option<String>,
    image: Option<String>,
    enable_live: bool,
    copy_only: bool,
    dry_run: bool,
    yes: bool,
) -> Result<i32> {
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive && !yes && !dry_run {
        return Err(Error::new(
            "setup requires a terminal or explicit --yes; use --dry-run for a read-only preview",
        ));
    }
    let preferences = crate::runtime::preferences(None)?;
    if runtime == RuntimeChoice::Auto {
        if enable_live || copy_only {
            runtime = RuntimeChoice::Openshell;
        } else if engine != Engine::Auto || endpoint.is_some() {
            runtime = RuntimeChoice::Container;
        } else if interactive && !yes && !dry_run {
            println!(
                "Choose a runtime:\n  1. Plain containers (Podman or Docker; no OpenShell policy)\n  2. OpenShell (policy, copy/export and remote clients; rootless Podman)"
            );
            let default = preferences.runtime.unwrap_or(RuntimeChoice::Container);
            loop {
                let answer = setup_answer(&format!(
                    "Runtime [container/openshell; default: {}]: ",
                    if default == RuntimeChoice::Openshell {
                        "openshell"
                    } else {
                        "container"
                    }
                ))?;
                runtime = match answer.to_ascii_lowercase().as_str() {
                    "" => default,
                    "1" | "container" => RuntimeChoice::Container,
                    "2" | "openshell" => RuntimeChoice::Openshell,
                    _ => {
                        println!("Enter 1 or 2 (container or openshell).");
                        continue;
                    }
                };
                break;
            }
        }
    }
    let project = env::current_dir()?.canonicalize()?;
    let plain = crate::runtime::choose(
        if runtime == RuntimeChoice::Auto {
            preferences.runtime_choice()
        } else {
            runtime
        },
        &project,
    )?;
    if plain && (enable_live || copy_only) {
        return Err(Error::new(
            "gateway capability flags require --runtime openshell",
        ));
    }
    if !plain && (engine == Engine::Docker || endpoint.is_some()) {
        return Err(Error::new(
            "managed OpenShell gateway setup requires rootless Podman; use --runtime container for Docker CE",
        ));
    }
    if engine == Engine::Auto && endpoint.is_some() {
        return Err(Error::new("--endpoint requires --engine podman or docker"));
    }
    let options = if engine == Engine::Auto && endpoint.is_none() {
        preferences.container.unwrap_or_default()
    } else {
        ContainerOptions { engine, endpoint }
    };
    let default_image = preferences
        .image
        .unwrap_or_else(|| crate::runtime::DEFAULT_IMAGE.into());
    let image = match image {
        Some(image) => image,
        None if interactive && !yes && !dry_run => {
            println!("Use upstream latest, or enter a version tag / digest to pin the image.");
            let answer = setup_answer(&format!("Image [{default_image}]: "))?;
            if answer.is_empty() {
                default_image
            } else {
                answer
            }
        }
        None => default_image,
    };
    if !crate::config::valid_image(&image) {
        return Err(Error::new(
            "use a registry-qualified OCI image tag or digest",
        ));
    }
    println!(
        "{}",
        crate::config::review_json(&json!({
            "runtime": if plain { "container" } else { "openshell" },
            "container": if plain { Some(&options) } else { None },
            "image": image,
            "gateway_image": if plain { None } else { Some(crate::openshell::GATEWAY_IMAGE) },
            "defaults": binding::preferences_path()?,
            "dry_run": dry_run,
        }))?
    );
    println!(
        "Setup may enable/start the user Podman socket, pull the selected image, {}and save defaults for NEW projects. No package installation, sudo, permission changes, project mounts or existing guest replacement.",
        if plain {
            ""
        } else {
            "pull the compatible pinned gateway image, create/verify dungeon-local, "
        }
    );
    if !plain {
        println!(
            "OpenShell {OPEN_SHELL_VERSION} and rootless Podman must already be installed. Existing gateways are never restarted or overwritten."
        );
    } else {
        println!(
            "Auto discovery prefers usable Podman over Docker CE. Docker daemon access is host-root-equivalent; plain containers do not enforce OpenShell policy."
        );
    }
    if !yes && !dry_run && !confirm("Apply this setup?")? {
        return Err(Error::new("setup cancelled; no changes made"));
    }
    #[cfg(target_os = "linux")]
    {
        let container = if plain {
            let options = crate::container::prepare_service(&options, dry_run)?;
            crate::container::prepare_image(&options, &image, dry_run)?;
            Some(options)
        } else {
            if dry_run {
                crate::openshell::setup(enable_live || !copy_only, true)?;
                crate::openshell::local_setup::prepare(&image, true)?;
            } else {
                crate::openshell::local_setup::prepare(&image, false)?;
                crate::openshell::setup(enable_live || !copy_only, false)?;
            }
            None
        };
        if !dry_run {
            crate::runtime::save_preferences(&crate::runtime::Preferences {
                runtime: Some(if plain {
                    RuntimeChoice::Container
                } else {
                    RuntimeChoice::Openshell
                }),
                container,
                image: Some(image),
            })?;
            println!(
                "Setup complete. Existing project bindings are unchanged. Next: cd /path/to/project && dungeon init"
            );
        }
        Ok(0)
    }
    #[cfg(not(target_os = "linux"))]
    Err(Error::new("local setup currently requires Linux"))
}

fn setup_answer(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0 {
        return Err(Error::new("setup cancelled; no changes made"));
    }
    Ok(answer.trim().to_owned())
}

fn run_selection(arguments: &[String]) -> Result<(Option<Agent>, &[String])> {
    let Some(first) = arguments.first() else {
        return Ok((None, arguments));
    };
    let agent = match first.as_str() {
        "opencode" => Agent::Opencode,
        "claude" => Agent::Claude,
        "codex" => Agent::Codex,
        value if value.starts_with('-') => return Ok((None, arguments)),
        _ => {
            return Err(Error::new(
                "unknown harness; choose opencode, claude or codex, or put native flags after `dungeon run --`",
            ));
        }
    };
    let native = &arguments[1..];
    if native
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--harness-config" || arg.starts_with("--harness-config="))
    {
        return Err(Error::new(
            "--harness-config belongs to init, config --edit or reload, not run; use `dungeon reload --harness-config copy --yes` (or disabled) for an enabled unbound selection; put native agent arguments after --",
        ));
    }
    let native = if native.first().is_some_and(|arg| arg == "--") {
        &native[1..]
    } else {
        native
    };
    Ok((Some(agent), native))
}

fn create_project_binding(project: &Path, agent: Agent) -> Result<Config> {
    let path = binding::config_path(project)?;
    let directory = path.parent().expect("binding config parent").to_path_buf();
    let mut values = environment_defaults(project.to_path_buf(), agent)?;
    require_configuration_terminal(false)?;
    choose_environment(&mut values, true, true)?;
    values.harness = crate::imports::harness::defaults(
        agent,
        choose_copy(agent)?,
        false,
        vec![],
        project,
        &path,
    )?;
    save_environment(directory, values, false)
}

fn require_configuration_terminal(yes: bool) -> Result<()> {
    if !yes && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        return Err(Error::new(
            "configuration requires a terminal or explicit --yes; use `dungeon init --yes --agent AGENT` or `dungeon config --edit --yes` for automation; no configuration saved",
        ));
    }
    Ok(())
}

fn configuration_answer(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0 {
        return Err(Error::new(
            "configuration cancelled; no configuration saved",
        ));
    }
    Ok(answer.trim().to_owned())
}

fn choose_environment(values: &mut ConfigDoc, image: bool, transfer: bool) -> Result<()> {
    if image {
        loop {
            let answer = configuration_answer(&format!("Image [{}]: ", values.image))?;
            if answer.is_empty() {
                break;
            }
            if crate::config::valid_image(&answer) {
                values.image = answer;
                break;
            }
            println!("Enter a registry-qualified image tag or digest.");
        }
    }
    if transfer && values.container.is_none() {
        loop {
            let current = if values.project_transfer == ProjectTransfer::Live {
                "live"
            } else {
                "copy"
            };
            let answer =
                configuration_answer(&format!("Project access [live/copy; default: {current}]: "))?;
            match answer.to_ascii_lowercase().as_str() {
                "" => break,
                "live" => {
                    values.project_transfer = ProjectTransfer::Live;
                    break;
                }
                "copy" => {
                    values.project_transfer = ProjectTransfer::Copy;
                    break;
                }
                _ => println!("Live edits host files immediately; copy requires reviewed export."),
            }
        }
    }
    Ok(())
}

fn choose_copy(agent: Agent) -> Result<HarnessMode> {
    if agent == Agent::Opencode {
        println!(
            "Optional host configuration: a private snapshot can expose credentials and execute plugins in the guest. Native login/session databases are never shared."
        );
        if confirm("Use a reviewed copy of your OpenCode configuration?")? {
            return Ok(HarnessMode::Copy);
        }
    }
    Ok(HarnessMode::Disabled)
}

fn edit_environment(config: &Config, mut options: EditOptions) -> Result<i32> {
    require_configuration_terminal(options.yes)?;
    // Refuse before prompts or source discovery, even for stopped/invalid bindings.
    match std::fs::symlink_metadata(config.state_path()) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        _ => {
            return Err(Error::new(
                "configuration editing refused for a bound sandbox; guest login/sessions are preserved; initialize a separate environment",
            ));
        }
    }
    let mut values = config.values.clone();
    let guided = !options.yes
        && options.agent.is_none()
        && options.image.is_none()
        && options.project_transfer.is_none()
        && options.harness_config.is_none()
        && !options.exclude_provider_auth
        && options.harness_exclude.is_empty();
    if guided {
        loop {
            let answer = configuration_answer(&format!(
                "Agent [opencode/claude/codex; default: {}]: ",
                values.agent.as_str()
            ))?;
            values.agent = match answer.to_ascii_lowercase().as_str() {
                "" => values.agent,
                "opencode" => Agent::Opencode,
                "claude" => Agent::Claude,
                "codex" => Agent::Codex,
                _ => {
                    println!("Choose opencode, claude or codex.");
                    continue;
                }
            };
            break;
        }
        choose_environment(&mut values, true, true)?;
        if values.agent == Agent::Opencode {
            loop {
                let answer = configuration_answer(
                    "Host OpenCode configuration [keep/copy/disabled; default: keep]: ",
                )?;
                options.harness_config = match answer.to_ascii_lowercase().as_str() {
                    "" | "keep" => None,
                    "copy" => Some(HarnessMode::Copy),
                    "disabled" => Some(HarnessMode::Disabled),
                    _ => {
                        println!("Choose keep, copy or disabled; copying may expose credentials.");
                        continue;
                    }
                };
                break;
            }
        }
    }
    if let Some(agent) = options.agent {
        values.agent = agent;
    }
    if let Some(image) = options.image {
        values.image = image;
    }
    if let Some(transfer) = options.project_transfer {
        values.project_transfer = transfer;
    }
    let refresh = options.harness_config.is_some()
        || options.exclude_provider_auth
        || !options.harness_exclude.is_empty();
    if refresh {
        if values.harness.is_none() {
            if options.harness_config.is_none() {
                return Err(Error::new(
                    "choose --harness-config copy before setting import exclusions",
                ));
            }
            values.harness = crate::imports::harness::defaults(
                values.agent,
                options.harness_config.unwrap_or(HarnessMode::Disabled),
                options.exclude_provider_auth,
                options.harness_exclude.clone(),
                &values.project,
                &config.path,
            )?;
        } else if let Some(h) = &mut values.harness {
            if let Some(mode) = options.harness_config {
                h.mode = mode;
            }
            h.exclude_provider_auth |= options.exclude_provider_auth;
            if !options.harness_exclude.is_empty() {
                h.exclude = options.harness_exclude;
            }
        }
    }
    crate::imports::harness::update(config, values, refresh, |candidate| {
        approve_environment(candidate, options.yes)
    })?;
    writeln!(
        io::stdout(),
        "Configuration saved. No guest was started or replaced."
    )?;
    Ok(0)
}

fn save_environment(directory: PathBuf, values: ConfigDoc, yes: bool) -> Result<Config> {
    require_configuration_terminal(yes)?;
    let directory = new_environment_directory(directory)?;
    let candidate = load_config_data(
        directory.join("dungeon.json"),
        serde_json::to_value(&values)?,
        |path| Ok((path.to_path_buf(), starter_policy())),
    )?;
    let mut prepared = crate::imports::harness::prepare(candidate, true)?;
    approve_environment(&prepared.config, yes)?;
    let config = write_environment(directory, prepared.config.values.clone())?;
    let _admission = if config
        .values
        .harness
        .as_ref()
        .is_some_and(|h| h.mode == HarnessMode::Copy)
    {
        Some(crate::export::TransferGuard::environment(&config)?)
    } else {
        None
    };
    prepared.publish(&config).map_err(|error| Error::new(format!(
        "configuration saved, but harness snapshot publication failed: {error}; no runtime started; inspect configuration and run reload --yes before launch")))?;
    Ok(config)
}

fn environment_defaults(project: PathBuf, agent: Agent) -> Result<ConfigDoc> {
    let preferences = crate::runtime::preferences(Some(&project))?;
    let plain = preferences.runtime_choice() != RuntimeChoice::Openshell;
    Ok(ConfigDoc {
        openshell_contract: None,
        container: if plain {
            Some(preferences.container.unwrap_or_default())
        } else {
            None
        },
        schema: 2,
        name: binding::generated_name(&project)?,
        gateway: "dungeon-local".into(),
        workspace: "default".into(),
        project,
        project_transfer: ProjectTransfer::Live,
        image: preferences
            .image
            .unwrap_or_else(|| crate::runtime::DEFAULT_IMAGE.into()),
        agent,
        providers: vec![],
        imports: vec![],
        harness: None,
    })
}

fn print_plan(config: &Config) -> Result<i32> {
    if config.values.container.is_some() {
        writeln!(
            io::stdout(),
            "{}",
            crate::config::review_json(&crate::runtime::container_plan(config))?
        )?;
        return Ok(0);
    }
    let imports = crate::imports::plan(config)?;
    let mut unsatisfied = Vec::new();
    if config.values.openshell_contract.is_some() {
        unsatisfied.push("the modern OpenShell contract is not enabled in this build; no runtime operation or migration is authorized by this offline plan");
    }
    if !config.values.imports.is_empty() {
        unsatisfied.push("imports require approved unchanged snapshot and exact local bind-enabled UID998 gateway; offline plan is not runtime verification");
    }
    if crate::imports::harness::enabled(config) {
        unsatisfied.push("harness imports require Linux and the exact local bind-enabled UID998 gateway; OpenCode database authentication is not shared; copy requires an approved unchanged snapshot");
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
    writeln!(
        io::stdout(),
        "{}",
        crate::config::review_json(&json!({
            "adapter": "openshell",
            "version": OPEN_SHELL_VERSION,
            "openshell_contract": config.values.openshell_contract,
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
    )?;
    Ok(0)
}

fn run_command(config: &Config, command: Commands) -> Result<i32> {
    if config.values.container.is_some()
        && matches!(
            &command,
            Commands::Code
                | Commands::SshConfig
                | Commands::Ssh { .. }
                | Commands::Forward { .. }
                | Commands::Attach
                | Commands::Exec { owned: true, .. }
        )
    {
        if matches!(&command, Commands::Attach) {
            require_attach_terminal()?;
        }
        return Err(crate::runtime::openshell_feature_error());
    }
    if let Commands::Reload {
        harness_config,
        yes,
    } = command
    {
        return crate::imports::harness::reload(config, harness_config, yes);
    }
    if let Commands::Imports {
        command: ImportCommands::Stage { yes },
    } = command
    {
        return crate::imports::stage(config, yes);
    }
    let prepared;
    let config = if matches!(
        &command,
        Commands::Run { .. } | Commands::Up | Commands::Create | Commands::Attach
    ) {
        prepared = prepare_launch(config)?;
        &prepared
    } else {
        config
    };
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

    if !matches!(
        &command,
        Commands::Create
            | Commands::Up
            | Commands::Run { .. }
            | Commands::Attach
            | Commands::Upload { .. }
            | Commands::Export { .. }
    ) {
        crate::config::read_runtime_state(&config.state_path())?;
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
            writeln!(
                io::stdout(),
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "id": remote["id"],
                    "name": remote["name"],
                    "workspace": remote["workspace"],
                    "phase": remote["phase"],
                    "initial_upload_pending": remote["initial_upload_pending"],
                    "runtime": if config.values.container.is_some() {"container"} else {"openshell"},
                    "engine": remote["engine"],
                    "policy_enforcement": remote["policy_enforcement"],
                }))?
            )?;
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
        Commands::Run { arguments } => {
            let (agent, arguments) = run_selection(&arguments)?;
            let selected = agent.unwrap_or(config.values.agent);
            if !config.state_path().exists() {
                runtime.up()?;
            }
            let mut command = vec![selected.as_str().to_owned()];
            command.extend_from_slice(arguments);
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

fn prepare_launch(config: &Config) -> Result<Config> {
    let Err(error) = crate::imports::harness::original_sources_compatible(config) else {
        return Ok(config.clone());
    };
    // Any binding marker, including an invalid file or dangling link, preserves the guest.
    match std::fs::symlink_metadata(config.state_path()) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        _ => {
            return Err(Error::new(format!(
                "{error}\nThis environment has runtime state; no configuration changed. Preserve its guest login/sessions and initialize a separate environment with `dungeon init --directory PATH --name NEW_NAME --agent opencode --harness-config copy`, then use --config PATH/dungeon.json for reload and launch."
            )));
        }
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(error);
    }
    eprintln!(
        "Cannot use original-source OpenCode configuration with this host's SELinux state. Original files will not be relabeled.\n\nChoose how to continue for this uncreated environment:\n  1. Review a private copy (recommended; may expose credentials)\n  2. Continue without host configuration\n  3. Cancel (default)"
    );
    let mode = loop {
        eprint!("Harness configuration [1-3; default: cancel]: ");
        io::stderr().flush()?;
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer)? == 0 {
            return Err(Error::new("launch cancelled; configuration unchanged"));
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "1" | "copy" => break HarnessMode::Copy,
            "2" | "disabled" => break HarnessMode::Disabled,
            "" | "3" | "cancel" => {
                return Err(Error::new("launch cancelled; configuration unchanged"));
            }
            _ => eprintln!("Enter 1, 2 or 3 (copy, disabled or cancel)."),
        }
    };
    crate::imports::harness::reload(config, Some(mode), false)?;
    load_config(&config.path)
}

fn attach_opencode(runtime: &Runtime<'_>) -> Result<i32> {
    require_attach_terminal()?;
    #[cfg(target_os = "linux")]
    {
        runtime.attach()
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

fn require_launch_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(Error::new(
            "interactive launch requires a terminal; use `dungeon init --yes --agent opencode|claude|codex` to configure, then `dungeon run -- <native arguments>` for noninteractive guest execution",
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
    writeln!(
        io::stdout(),
        "{}",
        serde_json::to_string_pretty(&json!({
            "host_opencode": host,
            "reviewed_guest_opencode": reviewed,
            "reviewed_local_image": REVIEWED_IMAGE,
            "default_image": crate::runtime::DEFAULT_IMAGE,
            "default_container_image": crate::runtime::DEFAULT_IMAGE,
            "selected_project_image": selected,
            "registry_stable": latest,
            "reviewed_update_available": latest.as_deref().and_then(stable_version).is_some_and(|version| version > reviewed_number),
            "selection": "both runtimes use setup's saved image, otherwise upstream base:latest; reviewed guest versions describe only the optional local recipe, not the upstream image; existing bindings unchanged",
        }))?
    )?;
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
fn initialize(
    directory: Option<PathBuf>,
    project: PathBuf,
    project_transfer: Option<ProjectTransfer>,
    name: Option<String>,
    gateway: Option<String>,
    workspace: Option<String>,
    image: Option<String>,
    agent: Option<Agent>,
    providers: Vec<String>,
    harness_config: Option<HarnessMode>,
    exclude_provider_auth: bool,
    harness_exclude: Vec<String>,
    yes: bool,
    runtime: RuntimeChoice,
    engine: Engine,
    endpoint: Option<String>,
) -> Result<i32> {
    let project = canonical_project(&project)?;
    require_configuration_terminal(yes)?;
    let agent = select_agent(agent)?;
    let project_binding = directory.is_none();
    let directory = match directory {
        Some(directory) => directory,
        None => binding::config_path(&project)?
            .parent()
            .expect("binding config parent")
            .to_path_buf(),
    };
    let mut values = environment_defaults(project, agent)?;
    if matches!(runtime, RuntimeChoice::Openshell) {
        if engine == Engine::Docker || endpoint.is_some() {
            return Err(Error::new(
                "OpenShell engine selection belongs to its gateway; plain Docker uses --runtime container",
            ));
        }
        values.container = None;
    } else if matches!(runtime, RuntimeChoice::Container)
        || engine != Engine::Auto
        || endpoint.is_some()
    {
        values.container = Some(if engine == Engine::Auto && endpoint.is_none() {
            values.container.unwrap_or_default()
        } else {
            ContainerOptions { engine, endpoint }
        });
    }
    // Explicit OpenShell-only features select that contract during initialization.
    if matches!(runtime, RuntimeChoice::Auto)
        && engine == Engine::Auto
        && (project_transfer == Some(ProjectTransfer::Copy)
            || gateway.is_some()
            || workspace.is_some()
            || !providers.is_empty())
    {
        values.container = None;
    }
    if let Some(name) = name {
        values.name = name;
    }
    if let Some(gateway) = gateway {
        values.gateway = gateway;
    }
    if let Some(workspace) = workspace {
        values.workspace = workspace;
    }
    let choose_image = image.is_none();
    if let Some(image) = image {
        values.image = image;
    }
    if let Some(transfer) = project_transfer {
        values.project_transfer = transfer;
    }
    values.providers = providers;
    if !yes {
        choose_environment(&mut values, choose_image, project_transfer.is_none())?;
    }
    let harness_config = match harness_config {
        Some(mode) => mode,
        None if !yes => choose_copy(agent)?,
        None => HarnessMode::Disabled,
    };
    values.harness = crate::imports::harness::defaults(
        agent,
        harness_config,
        exclude_provider_auth,
        harness_exclude,
        &values.project,
        &directory.join("dungeon.json"),
    )?;
    let config = save_environment(directory, values, yes)?;

    writeln!(
        io::stdout(),
        "Configuration saved; runtime readiness not checked. No guest was started."
    )?;
    writeln!(io::stdout(), "Configuration: {:?}", config.path)?;
    if config.values.container.is_none() {
        writeln!(
            io::stdout(),
            "Review policy.json and named providers before launch."
        )?;
    }
    if config.values.image == REVIEWED_IMAGE {
        writeln!(
            io::stdout(),
            "Build the reviewed local image before first up (images/agent/README.md). `dungeon versions --check` shows upstream stable updates; nothing is downloaded automatically."
        )?;
    }
    if project_binding {
        writeln!(
            io::stdout(),
            "Next: dungeon setup if needed, then dungeon to launch {} (dungeon run without a terminal).",
            config.values.agent.as_str()
        )?;
    } else {
        writeln!(
            io::stdout(),
            "Next: dungeon setup if needed, then dungeon --config PATH run using the configuration path above."
        )?;
    }
    Ok(0)
}

fn approve_environment(config: &Config, yes: bool) -> Result<()> {
    let values = &config.values;
    writeln!(
        io::stdout(),
        "{}",
        crate::config::review_json(&json!({
            "config": config.path,
            "policy": config.policy_path,
            "name": values.name,
            "harness": values.agent,
            "project": values.project,
            "project_transfer": values.project_transfer,
            "gateway": values.gateway,
            "workspace": values.workspace,
            "image": values.image,
            "providers": values.providers,
            "host_configuration": values.harness.as_ref().filter(|h| h.mode != HarnessMode::Disabled),
            "runtime": if values.container.is_some() {"container"} else {"openshell"},
            "container": values.container,
        }))?
    )?;
    if values.container.is_some() {
        writeln!(
            io::stdout(),
            "Plain container: project read/write; selected configuration mounts read-only; normal container networking, no OpenShell policy enforcement."
        )?;
    } else {
        match values.project_transfer {
            ProjectTransfer::Copy => writeln!(
                io::stdout(),
                "Copy mode: first up uploads the project; guest changes reach the host only through reviewed export. No writable host mounts."
            )?,
            ProjectTransfer::Live => writeln!(
                io::stdout(),
                "Live mode: the guest can read and WRITE the selected host project immediately at /sandbox/project. Only this project is writable; separately reviewed harness files may be read-only. No upload/export. Requires a bind-enabled gateway. Choose --project-transfer copy for reviewed return of changes."
            )?,
        }
    }
    writeln!(
        io::stdout(),
        "Review project contents before first up; all files within a live project become guest-visible, including secrets. Transfer is not a secret scanner."
    )?;
    if let Some(h) = values
        .harness
        .as_ref()
        .filter(|h| h.mode != HarnessMode::Disabled)
    {
        writeln!(io::stdout(), "{}", crate::imports::harness::WARNING)?;
        if h.mode == HarnessMode::Copy {
            writeln!(
                io::stdout(),
                "The reviewed private snapshot will be saved with this configuration; no separate reload is needed."
            )?;
        }
        if h.exclude_provider_auth {
            writeln!(
                io::stdout(),
                "Auth exclusion omits legacy auth.json and entire opencode.json/jsonc/cli.json files. It is not a secret scanner for plugins or other content."
            )?;
        }
    }
    if !yes {
        require_configuration_terminal(false)?;
        if !confirm("Save this private configuration and continue?")? {
            return Err(Error::new(
                "project environment initialization cancelled; no files were created",
            ));
        }
    }
    Ok(())
}

pub(crate) fn write_environment(directory: PathBuf, values: ConfigDoc) -> Result<Config> {
    let directory = new_environment_directory(directory)?;
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

fn new_environment_directory(directory: PathBuf) -> Result<PathBuf> {
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

    Ok(directory)
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
    fn grouped_help_lists_every_registered_command_once() {
        let mut command = human_command();
        let names = command
            .get_subcommands()
            .map(|sub| sub.get_name().to_owned())
            .collect::<Vec<_>>();
        let text = command.render_long_help().to_string();
        for name in names {
            assert_eq!(
                text.lines()
                    .filter(|line| line.starts_with("  ")
                        && line.split_whitespace().next() == Some(name.as_str()))
                    .count(),
                1,
                "{name}: {text}"
            );
        }
    }

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
    fn run_selects_only_known_harnesses_and_forwards_native_flags() {
        let parsed = Cli::try_parse_from(["dungeon", "run", "claude"]).unwrap();
        let Some(Commands::Run { arguments }) = parsed.command else {
            panic!()
        };
        assert_eq!(run_selection(&arguments).unwrap().0, Some(Agent::Claude));
        let parsed = Cli::try_parse_from(["dungeon", "run", "codex"]).unwrap();
        let Some(Commands::Run { arguments }) = parsed.command else {
            panic!()
        };
        assert_eq!(run_selection(&arguments).unwrap().0, Some(Agent::Codex));
        for args in [vec!["--version"], vec!["--project-transfer", "live"]] {
            let arguments = args.into_iter().map(String::from).collect::<Vec<_>>();
            let (agent, native) = run_selection(&arguments).unwrap();
            assert_eq!(agent, None);
            assert_eq!(native, arguments);
        }
        assert!(run_selection(&["unknown".into()]).is_err());
    }

    #[test]
    fn explicit_native_separator_preserves_harness_config_as_workload_data() {
        let args = ["opencode", "--", "--harness-config", "copy"].map(String::from);
        let (agent, native) = run_selection(&args).unwrap();
        assert_eq!(agent, Some(Agent::Opencode));
        assert_eq!(native, ["--harness-config", "copy"]);
        let args = ["--harness-config", "copy"].map(String::from);
        assert_eq!(run_selection(&args).unwrap().1, args);
    }

    #[test]
    fn run_consumes_only_the_dungeon_separator_after_an_explicit_harness() {
        for agent in ["opencode", "claude", "codex"] {
            for (input, expected) in [
                (vec!["--version"], vec!["--version"]),
                (vec!["--", "--version"], vec!["--version"]),
                (vec!["--", "-p", "two words"], vec!["-p", "two words"]),
                (
                    vec!["exec", "--", "--version"],
                    vec!["exec", "--", "--version"],
                ),
                (
                    vec!["--", "exec", "--", "--version"],
                    vec!["exec", "--", "--version"],
                ),
                (vec!["--", "--", "--version"], vec!["--", "--version"]),
                (vec!["--"], vec![]),
            ] {
                let mut argv = vec!["dungeon", "run", agent];
                argv.extend(input);
                let parsed = Cli::try_parse_from(argv).unwrap();
                let Some(Commands::Run { arguments }) = parsed.command else {
                    panic!()
                };
                let (selected, native) = run_selection(&arguments).unwrap();
                assert_eq!(selected.unwrap().as_str(), agent);
                assert_eq!(native, expected, "{agent}: {arguments:?}");
            }
        }
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
                openshell_contract: None,
                container: None,
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
        assert!(REVIEWED_IMAGE.ends_with("opencode-2.1.2"));
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
                openshell_contract: None,
                container: None,
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
            openshell_contract: None,
            container: None,
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
