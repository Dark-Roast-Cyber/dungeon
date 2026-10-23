use crate::config::{Agent, Config, ConfigDoc, ProjectTransfer, canonical_project, load_config};
use crate::openshell::{GUEST_PROJECT, Runtime};
use crate::{Error, ErrorCode, Result};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const STDIN_LIMIT: usize = 64 * 1024;

#[derive(Parser)]
#[command(name = "dungeon", version)]
struct Request {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    namespace: Namespace,
}
#[derive(Subcommand)]
enum Namespace {
    Machine {
        #[arg(long, required = true)]
        schema: u64,
        #[command(subcommand)]
        operation: Operation,
    },
}
#[derive(Subcommand)]
enum Operation {
    Resolve {
        #[arg(long)]
        project: Option<PathBuf>,
    },
    Init {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        gateway: String,
        #[arg(long)]
        image: String,
        #[arg(long, default_value = "copy", value_enum)]
        project_transfer: ProjectTransfer,
        #[arg(long)]
        yes: bool,
    },
    Create {
        #[arg(long)]
        yes: bool,
    },
    Up {
        #[arg(long)]
        yes: bool,
    },
    Start,
    Status,
    Stop,
    Close {
        #[arg(long)]
        preserve: bool,
    },
    Exec {
        #[arg(long)]
        owned: bool,
        #[arg(long, default_value=GUEST_PROJECT)]
        cwd: String,
        #[arg(long)]
        stdin: bool,
        #[arg(last = true, required = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    Interactive {
        #[arg(long, required = true)]
        owned: bool,
        #[arg(last = true, required = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    Upload {
        #[arg(long)]
        yes: bool,
    },
    Export {
        #[arg(long)]
        yes: bool,
    },
}
impl Operation {
    fn name(&self) -> &'static str {
        match self {
            Self::Resolve { .. } => "resolve",
            Self::Init { .. } => "init",
            Self::Create { .. } => "create",
            Self::Up { .. } => "up",
            Self::Start => "start",
            Self::Status => "status",
            Self::Stop => "stop",
            Self::Close { .. } => "close",
            Self::Exec { .. } => "exec",
            Self::Interactive { .. } => "interactive",
            Self::Upload { .. } => "upload",
            Self::Export { .. } => "export",
        }
    }
}

fn selected(args: &[OsString]) -> bool {
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            let _ = args.next();
            continue;
        }
        if arg.to_string_lossy().starts_with("--config=") {
            continue;
        }
        if arg == "machine" {
            return true;
        }
        if !arg.to_string_lossy().starts_with('-') || arg == "--" {
            return false;
        }
    }
    false
}

pub(crate) fn entry(args: Vec<OsString>) -> Option<i32> {
    if !selected(&args) {
        return None;
    }
    let request = match Request::try_parse_from(args) {
        Ok(request) => request,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return Some(0);
            }
            let _ = print_json(&error_json(
                "request",
                &Error::refusal(ErrorCode::InvalidRequest, "Invalid machine request."),
                Value::Null,
            ));
            return Some(1);
        }
    };
    let Namespace::Machine { schema, operation } = request.namespace;
    let name = operation.name();
    if schema != 1 {
        let _ = print_json(&error_json(
            name,
            &Error::refusal(ErrorCode::InvalidRequest, "Unsupported machine schema."),
            Value::Null,
        ));
        return Some(1);
    }
    match dispatch(request.config, operation) {
        Ok((code, Some(result))) => Some(
            if print_json(&json!({"schema":1,"operation":name,"ok":true,"result":result})).is_ok() {
                code
            } else {
                1
            },
        ),
        Ok((code, None)) => Some(code),
        Err((error, review)) => {
            let _ = print_json(&error_json(name, &error, review));
            Some(1)
        }
    }
}

pub(crate) fn print_json(value: &Value) -> Result<()> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
pub(crate) fn error_json(operation: &str, error: &Error, review: Value) -> Value {
    let message = match error.code {
        ErrorCode::InvalidRequest => "Invalid machine request.",
        ErrorCode::Unbound => "No private environment is bound.",
        ErrorCode::ApprovalRequired => "Explicit approval is required.",
        ErrorCode::Unsupported => "This operation is unsupported.",
        ErrorCode::Busy => "Copy transfer is busy.",
        ErrorCode::Conflict => "Export conflicts require review.",
        ErrorCode::IdentityMismatch => "Environment identity does not match.",
        ErrorCode::PolicyDrift => "Environment policy changed.",
        ErrorCode::CleanupUnconfirmed => "Owned cleanup is unconfirmed.",
        ErrorCode::OperationFailed => "Environment operation failed.",
    };
    json!({"schema":1,"operation":operation,"ok":false,"error":{"code":error.code,"message":message,"effects":error.effects},"review":review})
}

fn environment(config: &Config, remote: Option<&Value>) -> Value {
    json!({"config":config.path,"project":config.values.project,"fingerprint":config.fingerprint,
        "sandbox_id":remote.and_then(|v|v["id"].as_str()),"guest_project":GUEST_PROJECT,
        "execution_location":"local","project_transfer":config.values.project_transfer})
}
fn capability(state: &str, reason: Option<&str>) -> Value {
    json!({"state":state,"reason":reason})
}
fn capabilities(config: &Config) -> Value {
    capabilities_on(config.values.project_transfer, std::env::consts::OS)
}
fn capabilities_on(transfer: ProjectTransfer, platform: &str) -> Value {
    let copy = transfer == ProjectTransfer::Copy;
    let linux = platform == "linux";
    let unix = matches!(platform, "linux" | "macos");
    let executable = if linux { "unknown" } else { "unsupported" };
    let transfer_state = if copy && unix {
        "supported"
    } else {
        "unsupported"
    };
    json!({
        "streamed_exec":capability(executable,Some("linux_fifo_guest_node_required")),
        "owned_interrupt":capability(executable,Some("guest_node_proc_required")),
        "unowned_interrupt":capability("degraded",Some("guest_cleanup_not_guaranteed")),
        "interactive":capability(executable,Some("linux_foreground_terminal_ssh_guest_node_required")),
        "finite_stdin":capability(executable,Some("linux_buffered_before_launch_unowned_only")),
        "live_stdin":capability("unsupported",Some("buffered_native_stdin")),
        "upload":capability(transfer_state,Some(if !copy{"live_transfer"}else if !unix{"unix_private_transfer_locks_required"}else{"whole_project_explicit_approval"})),
        "export":capability(transfer_state,Some(if !copy{"live_transfer"}else if !unix{"unix_private_transfer_locks_required"}else{"whole_project_reviewed_ready_only"})),
        "selective_transfer":capability("unsupported",Some("whole_project_only")),
        "preserve":capability("supported",None),"delete":capability("unsupported",Some("preserve_only")),
        "remote_execution":capability("unsupported",Some("local_only")),
        "cwd":capability("supported",Some("lexical_guest_project_scope_not_symlink_confinement"))
    })
}
fn result(config: &Config, remote: Option<&Value>, review: Value) -> Value {
    let phase = remote
        .map(|v| match v["phase"].as_str() {
            Some("Ready") => "ready",
            Some("Stopped") => "stopped",
            Some("Error") => "error",
            Some("Provisioning" | "Starting" | "Stopping") => "transitioning",
            _ => "unknown",
        })
        .unwrap_or("uncreated");
    json!({"bound":true,"environment":environment(config,remote),"phase":phase,
        "initial_upload_pending":remote.map(|v| v["initial_upload_pending"].clone()).unwrap_or(Value::Null),
        "identity":if remote.is_some(){"verified"}else{"configured"},
        "policy":if remote.is_some(){"verified"}else{"unknown"},"capabilities":capabilities(config),"review":review})
}
fn approve(yes: bool) -> Result<()> {
    if yes {
        Ok(())
    } else {
        Err(Error::refusal(
            ErrorCode::ApprovalRequired,
            "Explicit approval required.",
        ))
    }
}

fn platform_check(operation: &Operation, platform: &str) -> Result<()> {
    if (platform != "linux"
        && matches!(
            operation,
            Operation::Exec { .. } | Operation::Interactive { .. }
        ))
        || (!matches!(platform, "linux" | "macos")
            && matches!(
                operation,
                Operation::Upload { .. } | Operation::Export { .. }
            ))
    {
        return Err(Error::refusal(
            ErrorCode::Unsupported,
            "Operation unsupported on this host platform.",
        ));
    }
    Ok(())
}
pub(crate) fn validate_exec(argv: &[String], cwd: &str) -> Result<()> {
    if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|v| v.contains('\0')) {
        return Err(Error::refusal(
            ErrorCode::InvalidRequest,
            "Invalid command arguments.",
        ));
    }
    if cwd.contains('\0')
        || !(cwd == GUEST_PROJECT || cwd.starts_with(&format!("{GUEST_PROJECT}/")))
        || cwd
            .split('/')
            .skip(1)
            .any(|v| v.is_empty() || v == "." || v == "..")
    {
        return Err(Error::refusal(
            ErrorCode::InvalidRequest,
            "Invalid guest working directory.",
        ));
    }
    Ok(())
}

type Response = std::result::Result<(i32, Option<Value>), (Error, Value)>;
fn dispatch(explicit: Option<PathBuf>, operation: Operation) -> Response {
    let mut review = Value::Null;
    let run = (|| -> Result<(i32, Option<Value>)> {
        if let Operation::Init {
            directory,
            project,
            name,
            gateway,
            image,
            project_transfer,
            yes,
        } = operation
        {
            if explicit.is_some() {
                return Err(Error::refusal(
                    ErrorCode::InvalidRequest,
                    "Init has its own selection.",
                ));
            }
            approve(yes)?;
            let config = crate::cli::write_environment(
                directory,
                ConfigDoc {
                    schema: 2,
                    name,
                    gateway,
                    workspace: "default".into(),
                    image,
                    project: canonical_project(&project)?,
                    project_transfer,
                    agent: Agent::Opencode,
                    providers: vec![],
                    imports: vec![],
                    harness: None,
                },
            )?;
            return Ok((0, Some(result(&config, None, Value::Null))));
        }
        let project = match &operation {
            Operation::Resolve { project } => project.as_deref(),
            _ => None,
        };
        if explicit.is_some() && project.is_some() {
            return Err(Error::refusal(
                ErrorCode::InvalidRequest,
                "Selectors conflict.",
            ));
        }
        let config = if let Some(path) = explicit {
            Some(load_config(&path)?)
        } else {
            crate::binding::load(&canonical_project(project.unwrap_or(Path::new(".")))?)?
        };
        let Some(config) = config else {
            if matches!(operation, Operation::Resolve { .. }) {
                return Ok((
                    0,
                    Some(
                        json!({"bound":false,"environment":null,"phase":"unbound","initial_upload_pending":null,"identity":"unknown","policy":"unknown","capabilities":null,"review":null}),
                    ),
                ));
            }
            return Err(Error::refusal(ErrorCode::Unbound, "No binding."));
        };
        if matches!(operation, Operation::Resolve { .. }) {
            return Ok((0, Some(result(&config, None, Value::Null))));
        }
        match &operation {
            Operation::Create { yes } | Operation::Up { yes } | Operation::Upload { yes } => {
                approve(*yes)?
            }
            Operation::Close { preserve: false } => {
                return Err(Error::refusal(
                    ErrorCode::Unsupported,
                    "Close requires preserve.",
                ));
            }
            Operation::Exec {
                owned: true,
                stdin: true,
                ..
            } => {
                return Err(Error::refusal(
                    ErrorCode::Unsupported,
                    "Owned stdin unsupported.",
                ));
            }
            _ => {}
        }
        if config.values.project_transfer == ProjectTransfer::Live
            && matches!(
                operation,
                Operation::Upload { .. } | Operation::Export { .. }
            )
        {
            return Err(Error::refusal(
                ErrorCode::Unsupported,
                "Live project transfer has no upload/export.",
            ));
        }
        platform_check(&operation, std::env::consts::OS)?;
        if !matches!(
            operation,
            Operation::Status | Operation::Stop | Operation::Close { .. }
        ) {
            crate::imports::verify(&config)?;
        }
        let runtime = Runtime::new(&config)?.machine();
        (|| {
            let code = match operation {
                Operation::Create { .. } => runtime.create()?,
                Operation::Up { .. } => runtime.up()?,
                Operation::Start => runtime.start()?,
                Operation::Stop | Operation::Close { .. } => runtime.down()?,
                Operation::Status => 0,
                Operation::Upload { .. } => runtime.upload()?,
                Operation::Export { yes } => {
                    let outcome = runtime.export(yes);
                    review = runtime.output.reviewed();
                    outcome?
                }
                Operation::Exec {
                    arguments,
                    cwd,
                    owned,
                    stdin,
                } => {
                    validate_exec(&arguments, &cwd)?;
                    let input = if stdin {
                        let mut bytes = Vec::new();
                        std::io::stdin()
                            .take((STDIN_LIMIT + 1) as u64)
                            .read_to_end(&mut bytes)?;
                        if bytes.len() > STDIN_LIMIT {
                            return Err(Error::refusal(
                                ErrorCode::InvalidRequest,
                                "Stdin too large.",
                            ));
                        }
                        Some(bytes)
                    } else {
                        None
                    };
                    return runtime
                        .machine_execute(&arguments, &cwd, owned, input)
                        .map(|code| (code, None));
                }
                Operation::Interactive { arguments, .. } => {
                    validate_exec(&arguments, GUEST_PROJECT)?;
                    return runtime.ssh_owned(&arguments).map(|code| (code, None));
                }
                Operation::Init { .. } | Operation::Resolve { .. } => unreachable!(),
            };
            if code != 0 {
                return Err(Error::new("Native operation failed."));
            }
            let remote = if config.state_path().exists() {
                Some(runtime.inspect_bound()?)
            } else {
                None
            };
            Ok((0, Some(result(&config, remote.as_ref(), review.clone()))))
        })()
        .map_err(|error| runtime.output.error(error))
    })();
    run.map_err(|error| (error, review))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_capabilities_match_categorical_refusals() {
        for platform in ["linux", "macos", "windows"] {
            let caps = capabilities_on(ProjectTransfer::Copy, platform);
            let exec = Operation::Exec {
                owned: false,
                cwd: GUEST_PROJECT.into(),
                stdin: false,
                arguments: vec!["true".into()],
            };
            assert_eq!(platform_check(&exec, platform).is_ok(), platform == "linux");
            for key in [
                "streamed_exec",
                "owned_interrupt",
                "interactive",
                "finite_stdin",
            ] {
                assert_eq!(
                    caps[key]["state"],
                    if platform == "linux" {
                        "unknown"
                    } else {
                        "unsupported"
                    }
                );
            }
            for key in ["upload", "export"] {
                assert_eq!(
                    caps[key]["state"],
                    if platform == "windows" {
                        "unsupported"
                    } else {
                        "supported"
                    }
                );
            }
            assert_eq!(
                platform_check(&Operation::Upload { yes: true }, platform).is_ok(),
                platform != "windows"
            );
        }
    }
    #[test]
    fn namespace_selection_does_not_treat_config_values_as_commands() {
        for args in [
            vec!["dungeon", "--config", "machine", "status"],
            vec!["dungeon", "--config=machine", "status"],
            vec!["dungeon", "--", "machine"],
        ] {
            assert!(!selected(
                &args.into_iter().map(OsString::from).collect::<Vec<_>>()
            ));
        }
        assert!(selected(
            &[
                "dungeon", "--config", "private", "machine", "--schema", "1", "status"
            ]
            .map(OsString::from)
        ));
    }
    #[test]
    fn cwd_and_argv_are_data_in_narrow_guest_scope() {
        assert!(
            validate_exec(
                &["/bin/sh".into(), "-c".into(), "echo '$HOST'".into()],
                GUEST_PROJECT
            )
            .is_ok()
        );
        for cwd in [
            "/sandbox/project-other",
            "/sandbox/project/../private",
            "/sandbox/project/./sub",
            "/sandbox/project/",
            "relative",
            "/sandbox/project//x",
        ] {
            assert!(validate_exec(&["true".into()], cwd).is_err());
        }
        assert!(validate_exec(&["a\0secret".into()], GUEST_PROJECT).is_err());
    }
    #[test]
    fn diagnostics_never_serialize_error_text() {
        let v = error_json("status", &Error::new("SECRET\u{1b}[31m"), Value::Null).to_string();
        assert!(!v.contains("SECRET"));
        assert!(!v.contains("31m"));
    }
}
