use crate::policy::validate_policy;
use crate::{Error, OPEN_SHELL_VERSION, Result};
use data_encoding::BASE32_NOPAD;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_JSON_BYTES: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lower")]
pub(crate) enum Agent {
    Opencode,
    Claude,
    Codex,
}

impl Agent {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, clap::ValueEnum, PartialEq, Eq,
)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lower")]
pub(crate) enum ProjectTransfer {
    #[default]
    Copy,
    Live,
}

impl ProjectTransfer {
    fn is_copy(&self) -> bool {
        *self == Self::Copy
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigDoc {
    pub(crate) schema: u64,
    pub(crate) name: String,
    pub(crate) gateway: String,
    pub(crate) workspace: String,
    pub(crate) project: PathBuf,
    #[serde(default, skip_serializing_if = "ProjectTransfer::is_copy")]
    pub(crate) project_transfer: ProjectTransfer,
    pub(crate) image: String,
    pub(crate) agent: Agent,
    pub(crate) providers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) imports: Vec<Import>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Import {
    pub(crate) source: PathBuf,
    pub(crate) destination: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) path: PathBuf,
    pub(crate) values: ConfigDoc,
    pub(crate) policy: Value,
    pub(crate) policy_path: PathBuf,
    pub(crate) fingerprint: String,
}

impl Config {
    pub(crate) fn baseline_path(&self) -> PathBuf {
        self.path
            .parent()
            .expect("config parent")
            .join("baseline.json")
    }
    pub(crate) fn state_path(&self) -> PathBuf {
        self.path
            .parent()
            .expect("config parent")
            .join("runtime.json")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeState {
    pub(crate) schema: u64,
    pub(crate) id: String,
    pub(crate) config: String,
    pub(crate) observed_policy: String,
}

pub(crate) fn load_config(path: &Path) -> Result<Config> {
    let path = path.canonicalize()?;
    let mut values: ConfigDoc = serde_json::from_value(read_json(&path)?)
        .map_err(|error| Error::new(format!("invalid dungeon.json: {error}")))?;

    if values.schema != 2 {
        return Err(Error::new("expected configuration schema 2"));
    }

    sandbox_name(&values.name)?;
    for (field, value) in [
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

    let mut destinations = HashSet::new();
    for import in &values.imports {
        if import.destination != "/sandbox/.config/nvim" {
            return Err(Error::new(
                "the first import destination must be /sandbox/.config/nvim",
            ));
        }
        if !destinations.insert(&import.destination) {
            return Err(Error::new("duplicate import destination"));
        }
        validate_import(&import.source)?;
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
    for import in &values.imports {
        if import.source.starts_with(&values.project)
            || import
                .source
                .starts_with(path.parent().expect("config parent"))
            || values.project.starts_with(&import.source)
            || path.starts_with(&import.source)
        {
            return Err(Error::new(
                "imports must be outside the project and private configuration directory",
            ));
        }
    }
    if !valid_image(&values.image) {
        return Err(Error::new(
            "use a registry-qualified OCI image, not a local build path or unqualified name",
        ));
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

fn validate_import(source: &Path) -> Result<()> {
    use std::path::Component;
    if !source.is_absolute()
        || source
            .to_string_lossy()
            .split('/')
            .any(|part| part == "." || part == "..")
        || source
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
        || source == Path::new("/")
        || is_home(source)
    {
        return Err(Error::new(
            "import source must be a specific absolute path without traversal",
        ));
    }
    let mut path = PathBuf::new();
    for component in source.components() {
        path.push(component);
        if reserved_import_name(component.as_os_str()) {
            return Err(Error::new(
                "import includes a credential or native state path",
            ));
        }
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_symlink() || !kind.is_dir() && path != source {
            return Err(Error::new(
                "import path contains a symlink or non-directory ancestor",
            ));
        }
    }
    let kind = fs::symlink_metadata(source)?.file_type();
    if !kind.is_dir() {
        return Err(Error::new(
            "Neovim import source must be an ordinary directory",
        ));
    }
    validate_import_tree(source)
}

fn validate_import_tree(directory: &Path) -> Result<()> {
    for item in fs::read_dir(directory)? {
        let item = item?;
        if reserved_import_name(&item.file_name()) {
            return Err(Error::new(
                "import includes a credential or native state path",
            ));
        }
        let kind = item.file_type()?;
        if kind.is_dir() {
            validate_import_tree(&item.path())?;
        } else if !kind.is_file() {
            return Err(Error::new("import contains a symlink or special file"));
        }
    }
    Ok(())
}

fn reserved_import_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".ssh"
            | ".gnupg"
            | ".aws"
            | ".codex"
            | ".claude"
            | ".local"
            | ".cache"
            | ".npmrc"
            | ".netrc"
            | "auth.json"
            | "credentials"
            | "id_rsa"
            | "id_ed25519"
    ) || name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
}

pub(crate) fn create_private_dir(path: &Path) -> Result<()> {
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

pub(crate) fn safe_name(value: &str, field: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let edge = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let allowed = |byte: u8| edge(byte) || byte == b'-';

    if bytes.is_empty()
        || bytes.len() > 63
        || !edge(bytes[0])
        || !edge(bytes[bytes.len() - 1])
        || !bytes.iter().copied().all(allowed)
    {
        return Err(Error::new(format!(
            "{field} must be a lowercase DNS label, 1 to 63 characters"
        )));
    }

    Ok(())
}

pub(crate) fn sandbox_name(value: &str) -> Result<()> {
    safe_name(value, "name")?;
    if value.len() > 19 {
        return Err(Error::new(
            "sandbox name must be at most 19 characters for OpenShell 0.0.116",
        ));
    }
    Ok(())
}

pub(crate) fn write_new<T: Serialize>(path: &Path, value: &T) -> Result<()> {
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

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(Error::new("configuration exceeds 1 MiB"));
    }
    let text =
        String::from_utf8(bytes).map_err(|_| Error::new("configuration must be UTF-8 JSON"))?;
    read_json_text(&text)
}

pub(crate) fn read_json_text(text: &str) -> Result<Value> {
    decode_json(text)
}

pub(crate) fn digest(value: &Value) -> Result<String> {
    Ok(BASE32_NOPAD
        .encode(&Sha256::digest(serde_json::to_vec(value)?))
        .to_ascii_lowercase())
}

fn is_home(path: &Path) -> bool {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.canonicalize().unwrap_or(home) == path)
        .unwrap_or(false)
}

fn valid_image(image: &str) -> bool {
    if image.is_empty()
        || !image
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/@-".contains(&byte))
    {
        return false;
    }
    let Some((registry, rest)) = image.split_once('/') else {
        return false;
    };
    !rest.is_empty()
        && (registry.contains('.') || registry.contains(':') || registry == "localhost")
        && !image.split('/').any(|part| part == "..")
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrictVisitor;

        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("valid JSON without duplicate keys")
            }

            fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(Number::from(value))))
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(Number::from(value))))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                Number::from_f64(value)
                    .map(|number| StrictValue(Value::Number(number)))
                    .ok_or_else(|| E::custom("non-finite number"))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value.to_owned())))
            }

            fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value)))
            }

            fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }

            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> std::result::Result<Self::Value, D::Error> {
                StrictValue::deserialize(deserializer)
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut object: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = object.next_entry::<String, StrictValue>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
                    }
                    values.insert(key, value.0);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }

        deserializer.deserialize_any(StrictVisitor)
    }
}

fn decode_json(text: &str) -> Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|error| Error::new(format!("invalid JSON: {error}")))?;
    deserializer
        .end()
        .map_err(|error| Error::new(format!("invalid JSON: {error}")))?;
    Ok(value.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::starter_policy;

    #[test]
    fn duplicate_json_keys_are_rejected() {
        assert!(decode_json(r#"{"schema":2,"schema":3}"#).is_err());
    }

    #[test]
    fn names_reject_injection_shapes() {
        assert!(safe_name("demo", "name").is_ok());
        for value in ["--privileged", "../host", "UPPER", "bad_name", ""] {
            assert!(safe_name(value, "name").is_err());
        }
    }

    #[test]
    fn sandbox_name_matches_openshell_limit() {
        assert!(sandbox_name("a123456789012345678").is_ok());
        assert!(sandbox_name("a1234567890123456789").is_err());
        for field in ["gateway", "workspace", "provider"] {
            assert!(safe_name(&"a".repeat(63), field).is_ok());
            assert!(safe_name(&"a".repeat(64), field).is_err());
        }
    }

    #[test]
    fn schema_one_is_rejected_without_compatibility_layer() {
        let value = json!({
            "schema": 1,
            "name": "demo",
            "gateway": "local",
            "workspace": "default",
            "project": "/tmp/demo",
            "image": "ghcr.io/example/demo:latest",
            "agent": "opencode",
            "providers": [],
            "policy": "policy.json"
        });
        assert!(serde_json::from_value::<ConfigDoc>(value).is_err());
    }

    #[test]
    fn project_transfer_is_closed_and_defaults_to_copy() {
        let base = json!({
            "schema": 2,
            "name": "demo",
            "gateway": "local",
            "workspace": "default",
            "project": "/tmp/demo",
            "image": "localhost/test:1",
            "agent": "opencode",
            "providers": []
        });

        let omitted: ConfigDoc = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(omitted.project_transfer, ProjectTransfer::Copy);

        let mut explicit_copy = base.clone();
        explicit_copy["project_transfer"] = json!("copy");
        let explicit_copy: ConfigDoc = serde_json::from_value(explicit_copy).unwrap();
        assert_eq!(explicit_copy.project_transfer, ProjectTransfer::Copy);

        let mut live = base.clone();
        live["project_transfer"] = json!("live");
        let live: ConfigDoc = serde_json::from_value(live).unwrap();
        assert_eq!(live.project_transfer, ProjectTransfer::Live);

        let mut invalid = base;
        invalid["project_transfer"] = json!("bind");
        assert!(serde_json::from_value::<ConfigDoc>(invalid).is_err());
    }

    #[test]
    fn project_transfer_changes_fingerprint_without_raw_driver_authority() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let private = root.path().join("private");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&private).unwrap();
        write_new(&private.join("policy.json"), &starter_policy()).unwrap();
        let config_path = private.join("dungeon.json");
        let mut doc = json!({
            "schema": 2,
            "name": "demo",
            "gateway": "local",
            "workspace": "default",
            "project": project,
            "image": "localhost/test:1",
            "agent": "opencode",
            "providers": []
        });
        let save =
            |value: &Value| fs::write(&config_path, serde_json::to_vec(value).unwrap()).unwrap();

        save(&doc);
        let omitted = load_config(&config_path).unwrap();
        assert_eq!(omitted.values.project_transfer, ProjectTransfer::Copy);
        let copy_fingerprint = omitted.fingerprint.clone();
        let policy = omitted.policy.clone();

        doc["project_transfer"] = json!("copy");
        save(&doc);
        assert_eq!(copy_fingerprint, load_config(&config_path).unwrap().fingerprint);

        doc["project_transfer"] = json!("live");
        save(&doc);
        let live = load_config(&config_path).unwrap();
        assert_ne!(copy_fingerprint, live.fingerprint);
        assert_eq!(policy, live.policy);

        doc["driver_config"] = json!({"podman": {"mounts": []}});
        save(&doc);
        assert!(load_config(&config_path).is_err());
        doc.as_object_mut().unwrap().remove("driver_config");

        doc["mounts"] = json!([]);
        save(&doc);
        assert!(load_config(&config_path).is_err());
    }

    #[test]
    fn import_preflight_and_fingerprint_are_bound_to_the_single_config() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let private = root.path().join("private");
        let source = root.path().join("nvim");
        for dir in [&project, &private, &source] {
            fs::create_dir(dir).unwrap();
        }
        fs::write(source.join("init.lua"), "vim.opt.number = true").unwrap();
        write_new(&private.join("policy.json"), &starter_policy()).unwrap();
        let config_path = private.join("dungeon.json");
        let mut doc = json!({
            "schema": 2, "name": "demo", "gateway": "local", "workspace": "default",
            "project": project, "image": "localhost/test:1", "agent": "opencode",
            "providers": [], "imports": []
        });
        let save =
            |value: &Value| fs::write(&config_path, serde_json::to_vec(value).unwrap()).unwrap();
        save(&doc);
        let original = load_config(&config_path).unwrap().fingerprint;
        let declaration = json!({"source": source, "destination": "/sandbox/.config/nvim"});
        doc["imports"] = json!([declaration]);
        save(&doc);
        assert_ne!(original, load_config(&config_path).unwrap().fingerprint);

        doc["imports"] = json!([declaration, declaration]);
        save(&doc);
        assert!(
            load_config(&config_path)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );

        for bad in [
            source.join("../nvim"),
            project.clone(),
            root.path().to_path_buf(),
            private.clone(),
        ] {
            doc["imports"] = json!([{"source": bad, "destination": "/sandbox/.config/nvim"}]);
            save(&doc);
            assert!(load_config(&config_path).is_err(), "{bad:?}");
        }
        doc["imports"] = json!([declaration]);
        fs::create_dir(source.join(".ssh")).unwrap();
        save(&doc);
        assert!(load_config(&config_path).is_err());
        fs::remove_dir(source.join(".ssh")).unwrap();
        fs::write(source.join(".env"), "FAKE_TOKEN=synthetic").unwrap();
        assert!(load_config(&config_path).is_err());
        fs::remove_file(source.join(".env")).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("init.lua", source.join("alias")).unwrap();
            assert!(load_config(&config_path).is_err());
            fs::remove_file(source.join("alias")).unwrap();
            use std::os::unix::net::UnixListener;
            let _socket = UnixListener::bind(source.join("socket")).unwrap();
            assert!(load_config(&config_path).is_err());
        }
    }
}
