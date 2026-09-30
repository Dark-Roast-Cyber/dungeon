use crate::config::{
    Agent, ConfigDoc, ProjectTransfer, create_private_dir, load_config, safe_name, sandbox_name,
    write_new,
};
use crate::openshell::{GUEST_PROJECT, Runtime, plan_create_arguments, project_transfer_plan};
use crate::policy::starter_policy;
use crate::{Error, OPEN_SHELL_VERSION, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

const DEFAULT_IMAGE: &str = "ghcr.io/nvidia/openshell-community/sandboxes/base:latest";

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
    Setup,
    Init {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        project: PathBuf,
        #[arg(long, value_enum, default_value = "copy")]
        project_transfer: ProjectTransfer,
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
        #[arg(long = "provider")]
        providers: Vec<String>,
        #[arg(long)]
        non_interactive: bool,
    },
    Plan,
    Create,
    Up,
    Down,
    Status,
    Start,
    Stop,
    Shell,
    Code,
    SshConfig,
    Upload {
        #[arg(long)]
        yes: bool,
    },
    Export {
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

pub(crate) fn run() -> Result<i32> {
    dispatch(Cli::parse())
}

fn dispatch(cli: Cli) -> Result<i32> {
    let Cli { config, command } = cli;
    match command {
        Commands::Setup => crate::openshell::setup(),
        Commands::Init {
            directory,
            project,
            project_transfer,
            name,
            gateway,
            workspace,
            image,
            agent,
            providers,
            non_interactive,
        } => initialize(
            directory,
            project,
            project_transfer,
            name,
            gateway,
            workspace,
            image,
            agent,
            providers,
            non_interactive,
        ),
        Commands::Plan => {
            let config = load_config(&required_config(config)?)?;
            let imports = config
                .values
                .imports
                .iter()
                .map(|import| {
                    Ok(json!({
                        "source": import.source,
                        "destination": import.destination,
                        "mode": "requested read-only snapshot; not applied",
                        "contents": crate::export::snapshot(&import.source)?,
                    }))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut unsatisfied = Vec::new();
            if !config.values.imports.is_empty() {
                unsatisfied.push(
                    "read-only guest import application is not yet supported; create/up refused",
                );
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
            command.extend(plan_create_arguments(&config, &config.policy_path));
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "adapter": "openshell",
                    "version": OPEN_SHELL_VERSION,
                    "fingerprint": config.fingerprint,
                    "command": command,
                    "policy": config.policy,
                    "imports": imports,
                    "tool_requirements": if config.values.imports.is_empty() { vec![] } else { vec!["nvim (reviewed image; not verified by plan)"] },
                    "guest_private_state": if config.values.imports.is_empty() { vec![] } else { vec!["/sandbox/.local/share/nvim", "/sandbox/.local/state/nvim", "/sandbox/.cache/nvim"] },
                    "provider_requests": config.values.providers,
                    "unsatisfied": unsatisfied,
                    "project_transfer": project_transfer_plan(&config),
                    "verification": "plan only; no backend contacted or enforcement verified"
                }))?
            );
            Ok(0)
        }
        command => {
            let config = load_config(&required_config(config)?)?;
            if !config.values.imports.is_empty() {
                return Err(Error::new(
                    "read-only guest imports are not yet enforceable by the OpenShell upload path; plan is review-only; remove imports to operate this environment",
                ));
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
            let runtime = Runtime::new(&config)?;
            match command {
                Commands::Create => runtime.create(),
                Commands::Up => runtime.up(),
                Commands::Down | Commands::Stop => runtime.down(),
                Commands::Status => {
                    println!("{}", serde_json::to_string_pretty(&runtime.get()?)?);
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
                    let mut command = vec![selected.as_str().to_owned()];
                    command.extend(arguments);
                    runtime.run_agent(
                        &command,
                        io::stdin().is_terminal() && io::stdout().is_terminal(),
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
                Commands::Start => runtime.start(),
                Commands::Init { .. }
                | Commands::Setup
                | Commands::Plan
                | Commands::Upload { yes: false } => {
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
    project_transfer: ProjectTransfer,
    mut name: Option<String>,
    mut gateway: Option<String>,
    workspace: String,
    mut image: String,
    mut agent: Agent,
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
                "use --non-interactive --name NAME without a terminal",
            ));
        }
        name = Some(ask(
            "Environment name",
            name.as_deref().unwrap_or("development"),
        )?);
        gateway = Some(ask(
            "Registered OpenShell gateway",
            gateway.as_deref().unwrap_or("dungeon-local"),
        )?);
        image = ask("Reviewed OCI image", &image)?;
        agent = Agent::from_str(
            &ask("Default agent (opencode/claude/codex)", agent.as_str())?,
            true,
        )
        .map_err(|_| Error::new("unknown default agent"))?;
    }

    let name = name.ok_or_else(|| Error::new("--name is required with --non-interactive"))?;
    let gateway = gateway.unwrap_or_else(|| "dungeon-local".to_owned());

    sandbox_name(&name)?;
    for (field, value) in [
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
    write_new(&directory.join("policy.json"), &starter_policy())?;
    write_new(
        &directory.join("dungeon.json"),
        &ConfigDoc {
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

fn is_home(path: &std::path::Path) -> bool {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.canonicalize().unwrap_or(home) == path)
        .unwrap_or(false)
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
    fn telltale_command_is_not_exposed() {
        assert!(Cli::try_parse_from(["dungeon", "telltale"]).is_err());
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
        })
        .unwrap();
        doc["imports"] = json!([{"source": source, "destination": "/sandbox/.config/nvim"}]);
        let path = private.join("dungeon.json");
        write_new(&path, &doc).unwrap();
        for action in ["create", "up", "run", "upload", "down"] {
            let mut args = vec!["dungeon", "--config", path.to_str().unwrap(), action];
            if action == "upload" {
                args.push("--yes");
            }
            assert!(
                dispatch(Cli::try_parse_from(args).unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains("not yet enforceable"),
                "{action}"
            );
        }
    }
}
