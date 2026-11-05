//! The two concrete execution contracts. Existing bindings always retain their runtime.
use crate::config::{Config, ContainerOptions};
use crate::{Error, Result};
use serde_json::Value;
use std::num::NonZeroU16;

pub(crate) const DEFAULT_IMAGE: &str = "ghcr.io/nvidia/openshell-community/sandboxes/base:latest";

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) runtime: Option<RuntimeChoice>,
    pub(crate) container: Option<ContainerOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) image: Option<String>,
}

impl Preferences {
    pub(crate) fn runtime_choice(&self) -> RuntimeChoice {
        self.runtime.unwrap_or(if self.container.is_some() {
            RuntimeChoice::Container
        } else {
            RuntimeChoice::Auto
        })
    }
}

pub(crate) fn preferences(project: Option<&std::path::Path>) -> Result<Preferences> {
    let path = crate::binding::preferences_path()?;
    let cwd = std::env::current_dir()?.canonicalize()?;
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .and_then(|p| p.canonicalize().ok());
    let ancestor = path
        .parent()
        .unwrap()
        .ancestors()
        .find(|p| p.exists())
        .ok_or_else(|| Error::new("setup defaults have no existing parent"))?;
    let resolved = ancestor
        .canonicalize()?
        .join(path.strip_prefix(ancestor).unwrap());
    if [Some(cwd.as_path()), project]
        .into_iter()
        .flatten()
        .any(|p| home.as_deref() != Some(p) && resolved.starts_with(p))
    {
        return Err(Error::new("runtime defaults must be outside the project"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(e) => return Err(e.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > 8192 {
        return Err(Error::new("invalid private setup defaults"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 || meta.nlink() != 1
        {
            return Err(Error::new("setup defaults must be private and user-owned"));
        }
    }
    use std::io::Read;
    let mut text = String::new();
    file.take(8193).read_to_string(&mut text)?;
    if text.len() > 8192 {
        return Err(Error::new("setup defaults exceed limit"));
    }
    let prefs: Preferences = crate::config::read_json_text(&text)
        .and_then(|value| serde_json::from_value(value).map_err(Into::into))
        .map_err(|_| Error::new("invalid private setup defaults"))?;
    if prefs.runtime == Some(RuntimeChoice::Auto)
        || (prefs.runtime == Some(RuntimeChoice::Openshell) && prefs.container.is_some())
        || (prefs.runtime == Some(RuntimeChoice::Container) && prefs.container.is_none())
        || prefs
            .image
            .as_deref()
            .is_some_and(|image| !crate::config::valid_image(image))
    {
        return Err(Error::new("invalid private setup defaults"));
    }
    if let Some(options) = &prefs.container {
        if options.engine == crate::config::Engine::Auto && options.endpoint.is_some() {
            return Err(Error::new("saved endpoint requires an explicit engine"));
        }
        if let Some(endpoint) = &options.endpoint {
            crate::config::validate_endpoint(endpoint)?;
        }
    }
    Ok(prefs)
}

#[cfg(target_os = "linux")]
pub(crate) fn save_preferences(value: &Preferences) -> Result<()> {
    let path = crate::binding::preferences_path()?;
    let previous = preferences(None)?;
    if serde_json::to_value(&previous)? == serde_json::to_value(value)? {
        return Ok(());
    }
    crate::config::create_private_dir(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.as_file().sync_all()?;
    file.persist(&path).map_err(|e| Error::new(e.to_string()))?;
    #[cfg(unix)]
    std::fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}

pub(crate) fn container_plan(config: &Config) -> Value {
    #[cfg(target_os = "linux")]
    {
        crate::container::plan(config)
    }
    #[cfg(not(target_os = "linux"))]
    {
        serde_json::json!({"runtime":"container","supported":false,"image":config.values.image})
    }
}

pub(crate) fn uninstall_binary(
    bin: Option<std::path::PathBuf>,
    dry: bool,
    yes: bool,
) -> Result<i32> {
    #[cfg(target_os = "linux")]
    {
        crate::openshell::local_setup::uninstall_binary(bin, dry, yes)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (bin, dry, yes);
        Err(Error::new("user-local uninstall currently requires Linux"))
    }
}

pub(crate) fn uninstall(bin: Option<std::path::PathBuf>, dry: bool, yes: bool) -> Result<i32> {
    crate::openshell::uninstall(bin, dry, yes)
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuntimeChoice {
    #[default]
    Auto,
    Container,
    Openshell,
}

pub(crate) fn choose(choice: RuntimeChoice, project: &std::path::Path) -> Result<bool> {
    match choice {
        RuntimeChoice::Container => Ok(true),
        RuntimeChoice::Openshell => Ok(false),
        RuntimeChoice::Auto => {
            let env = crate::openshell::host_environment(Some(project))?;
            Ok(crate::openshell::find_executable(
                "openshell",
                env.get(std::ffi::OsStr::new("PATH")),
            )
            .is_none_or(|p| p.starts_with(project)))
        }
    }
}

pub(crate) fn openshell_feature_error() -> Error {
    Error::refusal(
        crate::ErrorCode::Unsupported,
        "this feature requires OpenShell 0.0.116; initialize a separate environment with --runtime openshell",
    )
}

pub(crate) enum Runtime<'a> {
    OpenShell(crate::openshell::Runtime<'a>),
    #[cfg(target_os = "linux")]
    Container(crate::container::Runtime<'a>),
}

macro_rules! shared {
    ($($name:ident ($($arg:ident : $ty:ty),*) -> $ret:ty;)+) => {$ (
        pub(crate) fn $name(&self, $($arg: $ty),*) -> Result<$ret> {
            match self {
                Self::OpenShell(r) => r.$name($($arg),*),
                #[cfg(target_os = "linux")]
                Self::Container(r) => r.$name($($arg),*),
            }
        }
    )+};
}

impl<'a> Runtime<'a> {
    pub(crate) fn new(config: &'a Config) -> Result<Self> {
        if config.values.container.is_some() {
            #[cfg(target_os = "linux")]
            return Ok(Self::Container(crate::container::Runtime::new(config)?));
            #[cfg(not(target_os = "linux"))]
            return Err(Error::new("plain containers currently require Linux"));
        }
        Ok(Self::OpenShell(crate::openshell::Runtime::new(config)?))
    }

    pub(crate) fn machine(self) -> Self {
        match self {
            Self::OpenShell(r) => Self::OpenShell(r.machine()),
            #[cfg(target_os = "linux")]
            Self::Container(r) => Self::Container(r.machine()),
        }
    }

    pub(crate) fn output(&self) -> &crate::output::Output {
        match self {
            Self::OpenShell(r) => &r.output,
            #[cfg(target_os = "linux")]
            Self::Container(r) => &r.output,
        }
    }

    shared! {
        create() -> i32;
        up() -> i32;
        start() -> i32;
        down() -> i32;
        destroy() -> i32;
        inspect_for_destroy() -> Value;
        inspect_bound() -> Value;
        execute(argv: &[String], tty: bool, cwd: &str) -> i32;
        run_agent(argv: &[String], tty: bool) -> i32;
    }

    fn openshell(&self) -> Result<&crate::openshell::Runtime<'a>> {
        match self {
            Self::OpenShell(r) => Ok(r),
            #[cfg(target_os = "linux")]
            Self::Container(_) => Err(openshell_feature_error()),
        }
    }

    pub(crate) fn upload(&self) -> Result<i32> {
        self.openshell()?.upload()
    }
    pub(crate) fn export(&self, yes: bool) -> Result<i32> {
        self.openshell()?.export(yes)
    }
    pub(crate) fn execute_owned(&self, args: &[String]) -> Result<i32> {
        self.openshell()?.execute_owned(args)
    }
    pub(crate) fn ssh_owned(&self, args: &[String]) -> Result<i32> {
        self.openshell()?.ssh_owned(args)
    }
    pub(crate) fn connect_vscode(&self) -> Result<i32> {
        self.openshell()?.connect_vscode()
    }
    pub(crate) fn ssh_config(&self) -> Result<i32> {
        self.openshell()?.ssh_config()
    }
    pub(crate) fn forward_service(
        &self,
        port: NonZeroU16,
        local: Option<NonZeroU16>,
    ) -> Result<i32> {
        self.openshell()?.forward_service(port, local)
    }
    pub(crate) fn machine_execute(
        &self,
        argv: &[String],
        cwd: &str,
        owned: bool,
        stdin: Option<Vec<u8>>,
    ) -> Result<i32> {
        self.openshell()?.machine_execute(argv, cwd, owned, stdin)
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn attach(&self) -> Result<i32> {
        let r = self.openshell()?;
        if r.up()? != 0 {
            return Err(Error::new("sandbox did not become Ready"));
        }
        crate::openshell::attach_opencode(r)
    }
}
