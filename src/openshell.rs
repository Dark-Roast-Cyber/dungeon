use crate::config::{Config, ProjectTransfer, RuntimeState, digest, read_json, write_new};
use crate::export;
use crate::policy::validate_policy;
use crate::{Error, OPEN_SHELL_VERSION, Result};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "doctor.rs"]
mod diagnostics;
pub(crate) use diagnostics::run as doctor;

#[cfg(target_os = "linux")]
#[path = "owned.rs"]
mod owned;

#[cfg(target_os = "linux")]
#[path = "setup.rs"]
mod local_setup;

#[cfg(target_os = "linux")]
pub(crate) use local_setup::run as setup;

#[cfg(not(target_os = "linux"))]
pub(crate) fn setup() -> Result<i32> {
    Err(Error::new(
        "dungeon setup supports Linux only (rootless Podman); no MicroVM fallback",
    ))
}

const LABEL: &str = "agentarchaeology.ai/dungeon-config";
pub(crate) const GUEST_PROJECT: &str = "/sandbox/project";

#[derive(Clone, Debug, Serialize)]
struct LiveProjectMount {
    #[serde(rename = "type")]
    kind: &'static str,
    source: String,
    target: &'static str,
    read_only: bool,
    selinux_label: &'static str,
}

fn live_project_mount(config: &Config) -> Option<LiveProjectMount> {
    (config.values.project_transfer == ProjectTransfer::Live).then(|| LiveProjectMount {
        kind: "bind",
        source: config.values.project.to_string_lossy().into_owned(),
        target: GUEST_PROJECT,
        read_only: false,
        selinux_label: "private",
    })
}

fn live_driver_config(config: &Config) -> Option<String> {
    live_project_mount(config).map(|mount| json!({"podman": {"mounts": [mount]}}).to_string())
}

pub(crate) fn project_transfer_plan(config: &Config) -> Value {
    match live_project_mount(config) {
        Some(mount) => json!({
            "mode": "live",
            "host_source": mount.source,
            "guest_target": mount.target,
            "read_only": mount.read_only,
            "selinux_label_candidate": mount.selinux_label,
            "runtime_capability": {
                "driver": "podman",
                "host_bind_mounts": "required",
                "gateway_setting": "enable_bind_mounts=true",
                "user_namespace_candidate": "keep-id:uid=998,gid=998"
            },
            "validation": "unresolved; offline plan does not verify gateway capability or live enforcement"
        }),
        None => json!({
            "mode": "copy",
            "guest_target": GUEST_PROJECT,
            "transfer": "explicit OpenShell upload",
            "export": "reviewed staged export",
            "runtime_capability": null,
            "validation": "offline plan; backend enforcement is not verified by plan"
        }),
    }
}

#[derive(PartialEq, Eq)]
enum Phase {
    Ready,
    Stopped,
}

pub(crate) struct Runtime<'a> {
    config: &'a Config,
    env: HashMap<OsString, OsString>,
    binary: PathBuf,
}

impl<'a> Runtime<'a> {
    pub(crate) fn ssh_owned(&self, arguments: &[String]) -> Result<i32> {
        #[cfg(target_os = "linux")]
        {
            owned::ssh(self, arguments)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = arguments;
            Err(Error::new("owned SSH requires a Linux host and guest"))
        }
    }
    pub(crate) fn execute_owned(&self, arguments: &[String]) -> Result<i32> {
        #[cfg(target_os = "linux")]
        {
            owned::run(self, arguments)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = arguments;
            Err(Error::new(
                "owned execution requires a Linux host and guest",
            ))
        }
    }
    fn phase(remote: &Value) -> Result<Phase> {
        match Self::remote_str(remote, "phase")? {
            "Ready" => Ok(Phase::Ready),
            "Stopped" => Ok(Phase::Stopped),
            phase => Err(Error::new(format!(
                "sandbox is {phase}; inspect status before continuing"
            ))),
        }
    }

    pub(crate) fn up(&self) -> Result<i32> {
        if !self.config.state_path().exists() {
            self.create()?;
            match self.config.values.project_transfer {
                ProjectTransfer::Copy => {
                    let code = self.upload().map_err(|error| Error::new(format!(
                        "environment created, but initial project upload failed: {error}; sandbox preserved; retry with upload --yes"
                    )))?;
                    if code != 0 {
                        return Err(Error::new(format!(
                            "environment created, but initial project upload failed (exit {code}); sandbox preserved; retry with upload --yes"
                        )));
                    }
                    println!("Environment Ready; initial project uploaded.");
                }
                ProjectTransfer::Live => {
                    println!(
                        "Environment Ready; live project mount requested at {GUEST_PROJECT}; no upload performed."
                    );
                }
            }
        } else {
            match Self::phase(&self.checked(false, true)?)? {
                Phase::Ready => match self.config.values.project_transfer {
                    ProjectTransfer::Copy => {
                        println!("Environment already up; guest project unchanged.")
                    }
                    ProjectTransfer::Live => {
                        println!("Environment already up; live project mount unchanged.")
                    }
                },
                Phase::Stopped => {
                    self.start()?;
                    println!("Environment Ready; existing environment resumed (no upload).");
                }
            }
        }
        Ok(0)
    }

    pub(crate) fn run_agent(&self, command: &[String], tty: bool) -> Result<i32> {
        if Self::phase(&self.checked(false, true)?)? == Phase::Stopped {
            self.start()?;
        }
        self.execute(command, tty, GUEST_PROJECT)
    }

    pub(crate) fn start(&self) -> Result<i32> {
        match Self::phase(&self.checked(false, true)?)? {
            Phase::Ready => Ok(0),
            Phase::Stopped => {
                let code = self.lifecycle_command("start")?;
                if code != 0 {
                    return Err(Error::new(format!(
                        "sandbox start failed (exit {code}); no replacement created"
                    )));
                }
                self.checked(true, true).map_err(|error| Error::new(format!(
                    "sandbox started but Ready/policy verification failed: {error}; no agent launched"
                )))?;
                Ok(0)
            }
        }
    }

    pub(crate) fn down(&self) -> Result<i32> {
        match Self::phase(&self.checked(false, false)?)? {
            Phase::Stopped => println!("Environment already stopped; guest state preserved."),
            Phase::Ready => {
                let code = self.lifecycle_command("stop")?;
                if code != 0 {
                    return Err(Error::new(format!("sandbox stop failed (exit {code})")));
                }
                if Self::phase(&self.checked(false, false)?)? != Phase::Stopped {
                    return Err(Error::new(
                        "sandbox stop returned success but sandbox is not Stopped",
                    ));
                }
                println!("Environment stopped; guest project, sessions and auth preserved.");
            }
        }
        Ok(0)
    }
    pub(crate) fn new(config: &'a Config) -> Result<Self> {
        let env = host_environment(Some(&config.values.project))?;
        let binary =
            find_executable("openshell", env.get(&OsString::from("PATH"))).ok_or_else(|| {
                Error::new(format!(
                    "install the OpenShell {OPEN_SHELL_VERSION} CLI outside the project and add it to PATH"
                ))
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

    pub(crate) fn get(&self) -> Result<Value> {
        let value = crate::config::read_json_text(&self.capture(
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

    pub(crate) fn create(&self) -> Result<i32> {
        if self.config.state_path().exists() {
            self.checked(false, true)?;
            println!("Environment already exists; no changes made.");
            return Ok(0);
        }

        let directory = tempfile::Builder::new()
            .prefix("create-")
            .tempdir_in(self.config.path.parent().expect("config parent"))?;
        let policy_path = directory.path().join("policy.json");
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

        match self.config.values.project_transfer {
            ProjectTransfer::Copy => println!(
                "Created {}. Project upload is a separate action.",
                self.config.values.name
            ),
            ProjectTransfer::Live => println!(
                "Created {} with the configured live project mount request. Live transfer has recorded Fedora/OpenShell 0.0.116/rootless-Podman validation only; default-mode promotion remains explicit.",
                self.config.values.name
            ),
        }
        Ok(0)
    }

    pub(crate) fn execute(&self, command: &[String], tty: bool, workdir: &str) -> Result<i32> {
        if command.is_empty()
            || command[0].is_empty()
            || command.iter().any(|arg| arg.contains('\0'))
        {
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

    pub(crate) fn upload(&self) -> Result<i32> {
        if self.config.values.project_transfer == ProjectTransfer::Live {
            return Err(Error::new(format!(
                "upload is unavailable in live project-transfer mode; the host project is requested directly at {GUEST_PROJECT}"
            )));
        }
        if !self.config.values.project.is_dir() {
            return Err(Error::new("the host project must exist for upload"));
        }
        self.checked(true, true)?;
        preflight_upload(&self.config.values.project, &self.config.values.project)?;
        let before = export::snapshot(&self.config.values.project)?;
        // Even a failed transfer may have overwritten part of the guest. The old baseline
        // can no longer justify an export once upload starts.
        let baseline_path = self.config.baseline_path();
        if baseline_path.exists() {
            if !fs::symlink_metadata(&baseline_path)?.file_type().is_file() {
                return Err(Error::new("invalid baseline destination"));
            }
            fs::remove_file(baseline_path)?;
        }
        // OpenShell nests named directories; uploading "." places their contents at the fixed guest path.
        let code = self.upload_command().status()?.code().unwrap_or(128);
        if code == 0 {
            if before != export::snapshot(&self.config.values.project)? {
                return Err(Error::new(
                    "host project changed during upload; baseline not advanced; preserve guest work before retrying upload",
                ));
            }
            // Only exact pre-upload host/post-upload guest matches are proven common state.
            let stage = self.download_project()?;
            let guest = export::snapshot(stage.path())?;
            export::save_baseline(self.config, &export::upload_baseline(&before, &guest))?;
        }
        Ok(code)
    }

    pub(crate) fn export(&self, yes: bool) -> Result<i32> {
        if self.config.values.project_transfer == ProjectTransfer::Live {
            return Err(Error::new(
                "export is unavailable in live project-transfer mode; live project changes are written directly to the host project",
            ));
        }
        self.checked(true, true)?;
        let baseline = export::load_baseline(self.config)?;
        let stage = self.download_project()?;
        export::review_and_apply_checked(self.config, &baseline, stage.path(), yes, || {
            self.checked(true, true).map(|_| ())
        })?;
        Ok(0)
    }

    fn download_project(&self) -> Result<tempfile::TempDir> {
        let stage = tempfile::Builder::new()
            .prefix("dungeon-export-")
            .tempdir_in(self.config.path.parent().expect("config parent"))?;
        let args = vec![
            "sandbox".into(),
            "download".into(),
            self.config.values.name.clone(),
            GUEST_PROJECT.into(),
            stage.path().to_string_lossy().into_owned(),
        ];
        let code = self.status(&args)?;
        if code != 0 {
            return Err(Error::new(format!(
                "OpenShell download failed (exit {code}); host unchanged"
            )));
        }
        self.checked(true, true)?;
        // OpenShell 0.0.116 extracts directory contents directly into DEST.
        Ok(stage)
    }

    pub(crate) fn connect_vscode(&self) -> Result<i32> {
        self.checked(true, true)?;
        self.status(&[
            "sandbox".into(),
            "connect".into(),
            self.config.values.name.clone(),
            "--editor".into(),
            "vscode".into(),
        ])
    }

    pub(crate) fn ssh_config(&self) -> Result<i32> {
        self.checked(true, true)?;
        self.status(&[
            "sandbox".into(),
            "ssh-config".into(),
            self.config.values.name.clone(),
        ])
    }

    pub(crate) fn forward_service(
        &self,
        port: NonZeroU16,
        local_port: Option<NonZeroU16>,
    ) -> Result<i32> {
        self.checked(true, true)?;
        let local_port = local_port.unwrap_or(port);
        self.status(&[
            "forward".into(),
            "service".into(),
            self.config.values.name.clone(),
            "--target-host".into(),
            "127.0.0.1".into(),
            "--target-port".into(),
            port.to_string(),
            "--local".into(),
            format!("127.0.0.1:{local_port}"),
        ])
    }

    fn lifecycle_command(&self, action: &str) -> Result<i32> {
        self.status(&[
            "sandbox".into(),
            action.into(),
            self.config.values.name.clone(),
        ])
    }

    fn command(&self, args: &[String], scoped: bool) -> Command {
        let mut command = Command::new(&self.binary);
        if scoped {
            command.args([
                "--gateway",
                self.config.values.gateway.as_str(),
                "--workspace",
                self.config.values.workspace.as_str(),
            ]);
        }
        command
            .args(args)
            .current_dir(self.config.path.parent().expect("config parent"))
            .env_clear()
            .envs(self.env.iter());
        command
    }

    fn upload_command(&self) -> Command {
        let mut command = self.command(&upload_arguments(self.config), true);
        command.current_dir(&self.config.values.project);
        command
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
            .and_then(|labels| labels.get(LABEL))
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
        self.bound_state()?;
        let remote = self.get().map_err(|error| {
            Error::new(format!(
                "bound sandbox unavailable; refusing to create or adopt a replacement: {error}"
            ))
        })?;
        self.check_remote(remote, ready, policy)
    }

    fn bound_state(&self) -> Result<RuntimeState> {
        let state: RuntimeState = serde_json::from_value(read_json(&self.config.state_path())?)
            .map_err(|error| Error::new(format!("invalid runtime.json: {error}")))?;

        if state.schema != 1 || state.config != self.config.fingerprint {
            return Err(Error::new(
                "launch configuration changed; restore it or initialize a new environment",
            ));
        }
        Ok(state)
    }

    fn check_remote(&self, remote: Value, ready: bool, policy: bool) -> Result<Value> {
        let state = self.bound_state()?;
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
}

pub(crate) fn create_arguments(config: &Config, policy_path: &Path) -> Vec<String> {
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
    if let Some(driver_config) = live_driver_config(config) {
        args.extend(["--driver-config-json".into(), driver_config]);
    }
    args
}

pub(crate) fn plan_create_arguments(config: &Config, policy_path: &Path) -> Vec<String> {
    let mut args = create_arguments(config, policy_path);
    if let Some(index) = args.iter().position(|arg| arg == "--driver-config-json")
        && let Some(value) = args.get_mut(index + 1)
    {
        *value = "<derived from project_transfer>".into();
    }
    args
}

fn upload_arguments(config: &Config) -> Vec<String> {
    vec![
        "sandbox".into(),
        "upload".into(),
        config.values.name.clone(),
        ".".into(),
        GUEST_PROJECT.into(),
    ]
}

pub(crate) fn host_environment(project: Option<&Path>) -> Result<HashMap<OsString, OsString>> {
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

    let mut output = HashMap::new();
    for key in ALLOWED {
        if let Some(value) = env::var_os(key) {
            output.insert(OsString::from(key), value);
        }
    }

    let mut paths = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        for entry in env::split_paths(&path) {
            if entry.is_absolute() {
                let resolved = entry.canonicalize().unwrap_or(entry);
                if project.is_none_or(|project| !resolved.starts_with(project)) {
                    paths.push(resolved);
                }
            }
        }
    }

    output.insert(
        OsString::from("PATH"),
        env::join_paths(paths).map_err(|_| Error::new("PATH contains an unsupported entry"))?,
    );
    output.insert(OsString::from("NO_COLOR"), OsString::from("1"));
    Ok(output)
}

pub(crate) fn find_executable(name: &str, path: Option<&OsString>) -> Option<PathBuf> {
    for directory in env::split_paths(path?) {
        let candidate = directory.join(format!("{name}{}", env::consts::EXE_SUFFIX));
        if candidate.is_file() {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    None
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
    use crate::config::{Agent, ConfigDoc, ProjectTransfer};
    use crate::policy::starter_policy;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    struct Fixture {
        root: tempfile::TempDir,
        config: Config,
        binary: PathBuf,
    }

    #[cfg(unix)]
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let project = root.path().join("project");
            fs::create_dir(&project).unwrap();
            fs::write(project.join("file"), "host").unwrap();
            let config = Config {
                path: root.path().join("dungeon.json"),
                policy_path: root.path().join("policy.json"),
                values: ConfigDoc {
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
                },
                policy: starter_policy(),
                fingerprint: "fingerprint".into(),
            };
            let binary = root.path().join("openshell");
            let policy = serde_json::to_string(&config.policy).unwrap();
            fs::write(&binary, format!(r#"#!/bin/sh
root='{}'
printf '%s\n' "$*" >> "$root/calls"
case " $* " in
  *' forward service '*)
    printf '%s\n' "$PWD" "${{CARGO_MANIFEST_DIR-unset}}" "${{OPENCODE_PASSWORD-unset}}" >> "$root/forward-context"
    test ! -e "$root/fail-forward" || exit 6
    ;;
  *' sandbox create '*) echo Ready > "$root/phase" ;;
  *' sandbox start '*) test ! -e "$root/fail-start" || exit 9; echo Ready > "$root/phase" ;;
  *' sandbox stop '*) echo Stopped > "$root/phase" ;;
  *' sandbox upload '*) test ! -e "$root/fail-upload" || exit 8 ;;
  *' sandbox download '*) for dest do :; done; cp "$root/project/file" "$dest/file" ;;
  *' sandbox exec '*)
    while test "$#" -gt 0 && test "$1" != --; do shift; done
    shift
    if test "$1" = /usr/bin/node; then shift; exec "$root/host-node" "$@"; fi
    ;;
  *' sandbox get '*)
    test -e "$root/phase" || exit 7
    phase=$(cat "$root/phase")
    id=original
    test ! -e "$root/replaced" || id=replacement
    label=fingerprint
    test ! -e "$root/mismatch" || label=other
    policy='{policy}'
    test ! -e "$root/drift" || policy='{{"changed":true}}'
    printf '{{"id":"%s","name":"demo","workspace":"default","labels":{{"{LABEL}":"%s"}},"phase":"%s","policy_source":"sandbox","policy":%s}}\n' "$id" "$label" "$phase" "$policy"
    ;;
esac
"#, root.path().display())).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            Self {
                root,
                config,
                binary,
            }
        }

        fn runtime(&self) -> Runtime<'_> {
            Runtime {
                config: &self.config,
                env: HashMap::new(),
                binary: self.binary.clone(),
            }
        }

        fn flag(&self, name: &str) {
            fs::write(self.root.path().join(name), "").unwrap();
        }
        fn calls(&self, action: &str) -> usize {
            fs::read_to_string(self.root.path().join("calls"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.contains(&format!("sandbox {action} ")))
                .count()
        }
    }

    #[cfg(unix)]
    #[test]
    fn fresh_up_uploads_once_and_resume_preserves_guest() {
        let fixture = Fixture::new();
        let runtime = fixture.runtime();
        runtime.up().unwrap();
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 1);
        assert_eq!(fixture.calls("download"), 1);
        assert!(fixture.config.baseline_path().exists());
        let download_call = fs::read_to_string(fixture.root.path().join("calls")).unwrap();
        let download_call = download_call
            .lines()
            .find(|line| line.contains("sandbox download "))
            .unwrap();
        assert!(download_call.contains(fixture.config.path.parent().unwrap().to_str().unwrap()));
        assert!(!download_call.contains(fixture.config.values.project.to_str().unwrap()));
        runtime.up().unwrap();
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 1);
        assert_eq!(fixture.calls("download"), 1);
        runtime.down().unwrap();
        runtime.down().unwrap();
        assert_eq!(fixture.calls("stop"), 1);
        runtime.up().unwrap();
        assert_eq!(fixture.calls("start"), 1);
        assert_eq!(fixture.calls("upload"), 1);
        assert_eq!(fixture.calls("download"), 1);
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("delete"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn live_up_never_uploads_and_resume_reuses_the_bound_sandbox() {
        let mut fixture = Fixture::new();
        fixture.config.values.project_transfer = ProjectTransfer::Live;
        let runtime = fixture.runtime();

        runtime.up().unwrap();
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fixture.calls("download"), 0);
        assert!(!fixture.config.baseline_path().exists());

        runtime.up().unwrap();
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 0);

        runtime.down().unwrap();
        runtime.up().unwrap();
        assert_eq!(fixture.calls("start"), 1);
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fixture.calls("download"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn live_upload_and_export_refuse_before_openshell_contact() {
        let mut fixture = Fixture::new();
        fixture.config.values.project_transfer = ProjectTransfer::Live;
        let runtime = fixture.runtime();

        assert!(runtime.upload().unwrap_err().to_string().contains("live"));
        assert!(
            runtime
                .export(true)
                .unwrap_err()
                .to_string()
                .contains("live")
        );
        assert_eq!(fixture.calls("get"), 0);
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fixture.calls("download"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn upload_failure_preserves_binding_and_allows_explicit_recovery() {
        let fixture = Fixture::new();
        fixture.flag("fail-upload");
        let runtime = fixture.runtime();
        let error = runtime.up().unwrap_err().to_string();
        assert!(error.contains("environment created, but initial project upload failed"));
        assert!(fixture.config.state_path().exists());
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 1);
        assert!(!fixture.config.baseline_path().exists());
        fs::remove_file(fixture.root.path().join("fail-upload")).unwrap();
        runtime.upload().unwrap();
        assert_eq!(fixture.calls("upload"), 2);
        assert!(fixture.config.baseline_path().exists());
        fs::write(fixture.config.values.project.join("file"), "refreshed").unwrap();
        runtime.upload().unwrap();
        assert_eq!(fixture.calls("upload"), 3);
        assert_eq!(
            crate::export::load_baseline(&fixture.config).unwrap(),
            crate::export::snapshot(&fixture.config.values.project).unwrap()
        );
        fixture.flag("fail-upload");
        assert_eq!(runtime.upload().unwrap(), 8);
        assert!(!fixture.config.baseline_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn binding_mismatch_drift_and_absence_refuse_all_transitions() {
        for flag in ["mismatch", "replaced", "drift", "missing", "fingerprint"] {
            let fixture = Fixture::new();
            let runtime = fixture.runtime();
            runtime.create().unwrap();
            runtime.down().unwrap();
            if flag == "missing" {
                fs::remove_file(fixture.root.path().join("phase")).unwrap();
            } else if flag == "fingerprint" {
                let mut state = read_json(&fixture.config.state_path()).unwrap();
                state["config"] = Value::String("other".into());
                fs::write(
                    fixture.config.state_path(),
                    serde_json::to_vec(&state).unwrap(),
                )
                .unwrap();
            } else {
                fixture.flag(flag);
            }
            assert!(runtime.up().is_err(), "{flag}");
            assert!(
                runtime.run_agent(&["opencode".into()], false).is_err(),
                "{flag}"
            );
            if flag != "drift" {
                assert!(runtime.down().is_err(), "{flag}");
            }
            assert_eq!(fixture.calls("create"), 1);
            assert_eq!(fixture.calls("start"), 0);
            assert_eq!(fixture.calls("upload"), 0);
            assert_eq!(fixture.calls("exec"), 0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_starts_only_when_stopped_and_start_failure_prevents_launch() {
        let fixture = Fixture::new();
        let runtime = fixture.runtime();
        runtime.create().unwrap();
        runtime.run_agent(&["opencode".into()], false).unwrap();
        assert_eq!(fixture.calls("exec"), 1);
        runtime.down().unwrap();
        fixture.flag("fail-start");
        assert!(
            runtime
                .run_agent(&["opencode".into()], false)
                .unwrap_err()
                .to_string()
                .contains("start failed")
        );
        assert_eq!(fixture.calls("exec"), 1);
        assert_eq!(fixture.calls("upload"), 0);
        fs::remove_file(fixture.root.path().join("fail-start")).unwrap();
        runtime.run_agent(&["opencode".into()], false).unwrap();
        assert_eq!(fixture.calls("exec"), 2);
        assert_eq!(fixture.calls("upload"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn forwarding_is_scoped_loopback_only_ready_only_and_preserves_exit_status() {
        let fixture = Fixture::new();
        let runtime = fixture.runtime();
        let port = NonZeroU16::new(4096).unwrap();
        let local_port = NonZeroU16::new(14096).unwrap();
        let forward_calls = || {
            fs::read_to_string(fixture.root.path().join("calls"))
                .unwrap_or_default()
                .lines()
                .filter(|line| line.contains("forward service"))
                .count()
        };
        assert!(runtime.forward_service(port, Some(local_port)).is_err());
        assert_eq!(forward_calls(), 0);
        assert_eq!(fixture.calls("create"), 0);
        runtime.create().unwrap();
        let state = fs::read(fixture.config.state_path()).unwrap();
        assert_eq!(runtime.forward_service(port, Some(local_port)).unwrap(), 0);
        assert_eq!(runtime.forward_service(port, None).unwrap(), 0);
        let calls = fs::read_to_string(fixture.root.path().join("calls")).unwrap();
        assert!(calls.contains("--gateway local --workspace default forward service demo --target-host 127.0.0.1 --target-port 4096 --local 127.0.0.1:14096\n"));
        assert!(calls.contains("--gateway local --workspace default forward service demo --target-host 127.0.0.1 --target-port 4096 --local 127.0.0.1:4096\n"));
        assert_eq!(
            fs::read_to_string(fixture.root.path().join("forward-context")).unwrap(),
            format!(
                "{0}\nunset\nunset\n{0}\nunset\nunset\n",
                fixture.root.path().canonicalize().unwrap().display()
            )
        );
        fixture.flag("fail-forward");
        assert_eq!(runtime.forward_service(port, Some(local_port)).unwrap(), 6);
        runtime.down().unwrap();
        assert!(runtime.forward_service(port, Some(local_port)).is_err());
        assert_eq!(forward_calls(), 3);
        assert_eq!(fixture.calls("start"), 0);
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fixture.calls("create"), 1);
        runtime.start().unwrap();
        for flag in ["mismatch", "replaced", "drift"] {
            fixture.flag(flag);
            assert!(
                runtime.forward_service(port, Some(local_port)).is_err(),
                "{flag}"
            );
            fs::remove_file(fixture.root.path().join(flag)).unwrap();
        }
        for phase in ["Provisioning", "Starting", "Stopping", "Error"] {
            fs::write(fixture.root.path().join("phase"), phase).unwrap();
            assert!(runtime.forward_service(port, None).is_err(), "{phase}");
        }
        fs::remove_file(fixture.root.path().join("phase")).unwrap();
        assert!(runtime.forward_service(port, None).is_err());
        fs::write(fixture.root.path().join("phase"), "Ready").unwrap();
        let mut invalid_state: Value = serde_json::from_slice(&state).unwrap();
        invalid_state["config"] = json!("changed");
        fs::write(fixture.config.state_path(), invalid_state.to_string()).unwrap();
        assert!(runtime.forward_service(port, None).is_err());
        fs::write(fixture.config.state_path(), b"not json").unwrap();
        assert!(runtime.forward_service(port, None).is_err());
        fs::write(fixture.config.state_path(), &state).unwrap();
        assert_eq!(forward_calls(), 3);
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("start"), 1);
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fs::read(fixture.config.state_path()).unwrap(), state);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn owned_command_preserves_result_and_binding_and_refuses_drift() {
        let fixture = Fixture::new();
        let environment = host_environment(Some(&fixture.config.values.project)).unwrap();
        let node = find_executable("node", environment.get(&OsString::from("PATH")))
            .expect("owned execution tests require Node 24 on a trusted host PATH");
        std::os::unix::fs::symlink(&node, fixture.root.path().join("host-node")).unwrap();
        let runtime = fixture.runtime();
        runtime.create().unwrap();
        let state = fs::read(fixture.config.state_path()).unwrap();
        assert_eq!(
            runtime
                .execute_owned(&[
                    node.to_str()
                        .expect("host test Node path must be UTF-8")
                        .into(),
                    "-e".into(),
                    "process.exit(19)".into()
                ])
                .unwrap(),
            19
        );
        assert_eq!(fs::read(fixture.config.state_path()).unwrap(), state);
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 0);
        fixture.flag("drift");
        let before = fixture.calls("exec");
        assert!(runtime.execute_owned(&["/bin/true".into()]).is_err());
        assert_eq!(fixture.calls("exec"), before);
    }

    #[cfg(unix)]
    #[test]
    fn failed_resume_does_not_replace_or_upload() {
        let fixture = Fixture::new();
        let runtime = fixture.runtime();
        runtime.create().unwrap();
        runtime.down().unwrap();
        fixture.flag("fail-start");
        assert!(
            runtime
                .up()
                .unwrap_err()
                .to_string()
                .contains("start failed")
        );
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fixture.calls("exec"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn fresh_up_uses_upload_preflight() {
        let fixture = Fixture::new();
        std::os::unix::fs::symlink("/etc/passwd", fixture.config.values.project.join("unsafe"))
            .unwrap();
        assert!(
            fixture
                .runtime()
                .up()
                .unwrap_err()
                .to_string()
                .contains("initial project upload failed")
        );
        assert_eq!(fixture.calls("upload"), 0);
        assert_eq!(fixture.calls("create"), 1);
    }

    #[cfg(unix)]
    #[test]
    fn export_requires_binding_baseline_and_approval() {
        let fixture = Fixture::new();
        let runtime = fixture.runtime();
        runtime.create().unwrap();
        assert!(
            runtime
                .export(true)
                .unwrap_err()
                .to_string()
                .contains("no trustworthy upload baseline")
        );
        assert_eq!(fixture.calls("download"), 0);
        runtime.upload().unwrap();
        fs::write(fixture.config.values.project.join("file"), "host change").unwrap();
        assert!(runtime.export(false).is_ok()); // host-only work stays untouched
        assert_eq!(
            fs::read_to_string(fixture.config.values.project.join("file")).unwrap(),
            "host change"
        );
        for flag in ["mismatch", "replaced", "drift"] {
            fixture.flag(flag);
            assert!(runtime.export(true).is_err(), "{flag}");
            assert_eq!(fixture.calls("download"), 2);
            fs::remove_file(fixture.root.path().join(flag)).unwrap();
        }
        runtime.down().unwrap();
        assert!(
            runtime
                .export(true)
                .unwrap_err()
                .to_string()
                .contains("not Ready")
        );
    }

    #[test]
    fn create_does_not_upload_and_upload_uses_project_contents() {
        let config = Config {
            path: PathBuf::from("/tmp/dungeon.json"),
            values: ConfigDoc {
                schema: 2,
                name: "demo".into(),
                gateway: "local".into(),
                workspace: "default".into(),
                project: PathBuf::from("/tmp/arbitrary-repo"),
                project_transfer: ProjectTransfer::Copy,
                image: "ghcr.io/nvidia/openshell-community/sandboxes/base:latest".into(),
                agent: Agent::Opencode,
                providers: vec![],
                imports: vec![],
            },
            policy: starter_policy(),
            policy_path: PathBuf::from("/tmp/policy.json"),
            fingerprint: "fingerprint".into(),
        };
        let args = create_arguments(&config, &config.policy_path);
        assert!(args.iter().any(|arg| arg == "--no-auto-providers"));
        assert!(!args.iter().any(|arg| arg == "--upload"));
        assert!(!args.iter().any(|arg| arg == "--env"));
        assert!(!args.iter().any(|arg| arg == "--driver-config-json"));
        assert_eq!(
            upload_arguments(&config),
            ["sandbox", "upload", "demo", ".", "/sandbox/project"]
        );
        let runtime = Runtime {
            config: &config,
            env: HashMap::new(),
            binary: PathBuf::from("/usr/bin/openshell"),
        };
        let upload = runtime.upload_command();
        assert_eq!(
            upload.get_current_dir(),
            Some(config.values.project.as_path())
        );
        assert_eq!(
            upload
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            [
                "--gateway",
                "local",
                "--workspace",
                "default",
                "sandbox",
                "upload",
                "demo",
                ".",
                "/sandbox/project"
            ]
        );
        assert_eq!(
            runtime
                .command(&["sandbox".into(), "get".into()], true)
                .get_current_dir(),
            config.path.parent()
        );

        let mut live = config.clone();
        live.values.project_transfer = ProjectTransfer::Live;
        let live_args = create_arguments(&live, &live.policy_path);
        let index = live_args
            .iter()
            .position(|arg| arg == "--driver-config-json")
            .unwrap();
        let driver_config: Value = serde_json::from_str(&live_args[index + 1]).unwrap();
        assert_eq!(
            driver_config,
            json!({
                "podman": {
                    "mounts": [{
                        "type": "bind",
                        "source": "/tmp/arbitrary-repo",
                        "target": GUEST_PROJECT,
                        "read_only": false,
                        "selinux_label": "private"
                    }]
                }
            })
        );
        assert_eq!(
            driver_config["podman"]["mounts"].as_array().unwrap().len(),
            1
        );

        let plan_args = plan_create_arguments(&live, &live.policy_path);
        let index = plan_args
            .iter()
            .position(|arg| arg == "--driver-config-json")
            .unwrap();
        assert_eq!(plan_args[index + 1], "<derived from project_transfer>");

        let plan = project_transfer_plan(&live);
        assert_eq!(plan["mode"], "live");
        assert_eq!(plan["host_source"], "/tmp/arbitrary-repo");
        assert_eq!(plan["guest_target"], GUEST_PROJECT);
        assert_eq!(plan["read_only"], false);
        assert_eq!(
            plan["runtime_capability"]["gateway_setting"],
            "enable_bind_mounts=true"
        );
        assert_eq!(
            plan["runtime_capability"]["user_namespace_candidate"],
            "keep-id:uid=998,gid=998"
        );
        assert!(
            plan["validation"]
                .as_str()
                .unwrap()
                .contains("does not verify")
        );
    }
}
