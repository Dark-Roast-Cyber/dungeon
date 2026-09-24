use crate::config::{Config, RuntimeState, digest, read_json, write_new};
use crate::policy::validate_policy;
use crate::{Error, OPEN_SHELL_VERSION, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
            let code = self.upload().map_err(|error| Error::new(format!(
                "environment created, but initial project upload failed: {error}; sandbox preserved; retry with upload --yes"
            )))?;
            if code != 0 {
                return Err(Error::new(format!(
                    "environment created, but initial project upload failed (exit {code}); sandbox preserved; retry with upload --yes"
                )));
            }
            println!("Environment Ready; initial project uploaded.");
        } else {
            match Self::phase(&self.checked(false, true)?)? {
                Phase::Ready => println!("Environment already up; guest project unchanged."),
                Phase::Stopped => {
                    self.start()?;
                    println!("Environment Ready; existing guest work preserved (no upload).");
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

        println!(
            "Created {}. Project upload is a separate action.",
            self.config.values.name
        );
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
        if !self.config.values.project.is_dir() {
            return Err(Error::new("the host project must exist for upload"));
        }
        self.checked(true, true)?;
        preflight_upload(&self.config.values.project, &self.config.values.project)?;
        // OpenShell nests named directories; uploading "." places their contents at the fixed guest path.
        Ok(self.upload_command().status()?.code().unwrap_or(128))
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
        let state: RuntimeState = serde_json::from_value(read_json(&self.config.state_path())?)
            .map_err(|error| Error::new(format!("invalid runtime.json: {error}")))?;

        if state.schema != 1 || state.config != self.config.fingerprint {
            return Err(Error::new(
                "launch configuration changed; restore it or initialize a new environment",
            ));
        }

        let remote = self.get().map_err(|error| {
            Error::new(format!(
                "bound sandbox unavailable; refusing to create or adopt a replacement: {error}"
            ))
        })?;
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
    use crate::config::{Agent, ConfigDoc};
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
                    image: "localhost/test:1".into(),
                    agent: Agent::Opencode,
                    providers: vec![],
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
  *' sandbox create '*) echo Ready > "$root/phase" ;;
  *' sandbox start '*) test ! -e "$root/fail-start" || exit 9; echo Ready > "$root/phase" ;;
  *' sandbox stop '*) echo Stopped > "$root/phase" ;;
  *' sandbox upload '*) test ! -e "$root/fail-upload" || exit 8 ;;
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
        runtime.up().unwrap();
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("upload"), 1);
        runtime.down().unwrap();
        runtime.down().unwrap();
        assert_eq!(fixture.calls("stop"), 1);
        runtime.up().unwrap();
        assert_eq!(fixture.calls("start"), 1);
        assert_eq!(fixture.calls("upload"), 1);
        assert_eq!(fixture.calls("create"), 1);
        assert_eq!(fixture.calls("delete"), 0);
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
        fs::remove_file(fixture.root.path().join("fail-upload")).unwrap();
        runtime.upload().unwrap();
        assert_eq!(fixture.calls("upload"), 2);
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
                image: "ghcr.io/nvidia/openshell-community/sandboxes/base:latest".into(),
                agent: Agent::Opencode,
                providers: vec![],
            },
            policy: starter_policy(),
            policy_path: PathBuf::from("/tmp/policy.json"),
            fingerprint: "fingerprint".into(),
        };
        let args = create_arguments(&config, &config.policy_path);
        assert!(args.iter().any(|arg| arg == "--no-auto-providers"));
        assert!(!args.iter().any(|arg| arg == "--upload"));
        assert!(!args.iter().any(|arg| arg == "--env"));
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
    }
}
