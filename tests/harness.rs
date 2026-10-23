#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const SYNTHETIC_SECRET: &str = "synthetic-not-a-real-provider-key";

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    source: PathBuf,
    auth: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        let source = home.join("config/opencode");
        let auth = home.join("data/opencode/auth.json");
        for path in [
            &project,
            &source.join("plugins/local"),
            &source.join("themes"),
            auth.parent().unwrap(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        fs::write(
            source.join("opencode.jsonc"),
            format!("{{\"providers\":{{\"synthetic\":{{\"key\":\"{SYNTHETIC_SECRET}\"}}}}}}"),
        )
        .unwrap();
        fs::write(source.join("cli.json"), "{}").unwrap();
        fs::write(
            source.join("plugins/local/index.js"),
            "export default () => ({})",
        )
        .unwrap();
        fs::write(source.join("themes/synthetic.json"), "{}").unwrap();
        fs::write(
            &auth,
            format!("{{\"synthetic\":{{\"type\":\"api\",\"key\":\"{SYNTHETIC_SECRET}\"}}}}"),
        )
        .unwrap();
        Self {
            _root: root,
            home,
            project,
            source,
            auth,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with_path(args, "")
    }

    fn run_with_path(&self, args: &[&str], path: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dungeon"))
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("XDG_DATA_HOME", self.home.join("data"))
            .env("PATH", path)
            .current_dir(&self.project)
            .stdin(Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) -> Output {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains(SYNTHETIC_SECRET));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(SYNTHETIC_SECRET));
        out
    }

    fn config(&self) -> Value {
        serde_json::from_slice(&self.success(&["config"]).stdout).unwrap()
    }

    fn config_path(&self) -> PathBuf {
        PathBuf::from(self.config()["config"].as_str().unwrap())
    }

    fn plan(&self) -> Value {
        serde_json::from_slice(&self.success(&["plan"]).stdout).unwrap()
    }
}

#[test]
fn new_opencode_binding_selects_only_reviewed_native_leaves_and_no_session_database() {
    let f = Fixture::new();
    // Forbidden fixtures must be skipped before opening; contents are never read.
    fs::write(
        f.source.join("plugins/local/.env"),
        "synthetic forbidden fixture",
    )
    .unwrap();
    fs::write(
        f.source.join("plugins/local/backup.env"),
        "synthetic forbidden fixture",
    )
    .unwrap();
    fs::write(
        f.source.join("plugins/local/session.db"),
        "synthetic native state",
    )
    .unwrap();
    fs::write(f.source.join("unknown.json"), "not an import").unwrap();
    for dir in ["plugins/local/.git", "plugins/local/node_modules"] {
        fs::create_dir_all(f.source.join(dir)).unwrap();
        fs::write(
            f.source.join(dir).join("not-selected.js"),
            "synthetic metadata/dependency",
        )
        .unwrap();
    }
    fs::write(
        f.auth.parent().unwrap().join("opencode.db"),
        "synthetic native state",
    )
    .unwrap();
    let out = f.success(&["init", "--yes"]);
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("database credentials and sessions are NOT shared")
    );
    let config = f.config();
    assert_eq!(config["values"]["harness"]["mode"], "read-only");
    assert_eq!(
        config["values"]["harness"]["files"],
        json!([
            "auth.json",
            "config/cli.json",
            "config/opencode.jsonc",
            "config/plugins/local/index.js",
            "config/themes/synthetic.json"
        ])
    );
    let plan = f.plan();
    for mount in plan["harness"]["mounts"].as_array().unwrap() {
        assert_eq!(mount["read_only"], true);
        assert!(mount.get("selinux_label").is_none());
        assert!(!mount["source"].as_str().unwrap().ends_with("opencode.db"));
    }
    assert!(!f.config_path().with_file_name("runtime.json").exists());
}

#[test]
fn plan_and_config_never_reopen_sources_or_refresh_a_selected_live_inventory() {
    let f = Fixture::new();
    f.success(&["init", "--yes"]);
    let plan = f.plan();
    let path = f.config_path();
    let before = fs::read(&path).unwrap();
    fs::rename(&f.source, f.source.with_file_name("old-source")).unwrap();
    symlink("/unavailable-synthetic-source", &f.source).unwrap();
    fs::remove_file(&f.auth).unwrap();
    assert_eq!(f.plan(), plan);
    assert_eq!(fs::read(&path).unwrap(), before);
    let out = f.run(&["up"]);
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains(SYNTHETIC_SECRET));
    assert!(!path.with_file_name("runtime.json").exists());
}

#[test]
fn exclusions_omit_whole_mixed_credential_configs_and_do_not_open_excluded_links() {
    let f = Fixture::new();
    fs::remove_file(&f.auth).unwrap();
    symlink("/unavailable-synthetic-auth", &f.auth).unwrap();
    symlink(
        "/unavailable-synthetic-plugin",
        f.source.join("plugins/excluded"),
    )
    .unwrap();
    f.success(&[
        "init",
        "--yes",
        "--exclude-provider-auth",
        "--harness-exclude",
        "plugins/excluded",
        "--harness-exclude",
        "themes",
    ]);
    assert_eq!(
        f.config()["values"]["harness"]["files"],
        json!(["config/plugins/local/index.js"])
    );
    assert_eq!(f.plan()["harness"]["mounts"].as_array().unwrap().len(), 1);
}

#[test]
fn copy_reload_captures_private_bytes_and_source_drift_does_not_refresh_them() {
    let f = Fixture::new();
    f.success(&["init", "--yes", "--harness-config", "copy"]);
    assert!(f.config()["values"]["harness"]["snapshot"].is_null());
    let path = f.config_path();
    let before = fs::read(&path).unwrap();
    let refused = f.run(&["reload"]);
    assert!(!refused.status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
    f.success(&["reload", "--yes"]);
    let selected = f.config();
    let digest = selected["values"]["harness"]["snapshot"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    let plan = f.plan();
    let auth = &plan["harness"]["mounts"][0];
    assert_eq!(auth["selinux_label"], "private");
    let copied = PathBuf::from(auth["source"].as_str().unwrap());
    assert_ne!(copied, f.auth);
    assert_eq!(
        fs::metadata(&copied).unwrap().permissions().mode() & 0o777,
        0o400
    );
    // Same selection can be explicitly reapproved without overwriting an artifact.
    f.success(&["reload", "--yes"]);
    assert_eq!(
        f.config()["values"]["harness"],
        selected["values"]["harness"]
    );
    fs::write(&f.auth, "changed synthetic input").unwrap();
    assert_eq!(f.plan(), plan);
    f.success(&["reload", "--yes"]);
    assert_ne!(f.config()["values"]["harness"]["snapshot"], digest);
    assert!(copied.exists());
}

#[test]
fn reload_repairs_unbound_selection_but_refuses_any_runtime_binding_without_mutation() {
    let f = Fixture::new();
    f.success(&["init", "--yes"]);
    fs::remove_file(f.source.join("plugins/local/index.js")).unwrap();
    fs::write(f.source.join("plugins/new.js"), "export default () => ({})").unwrap();
    f.success(&["reload", "--yes"]);
    let config = f.config();
    let files = config["values"]["harness"]["files"].as_array().unwrap();
    assert!(files.contains(&json!("config/plugins/new.js")));
    assert!(!files.contains(&json!("config/plugins/local/index.js")));
    let path = f.config_path();
    let before = fs::read(&path).unwrap();
    fs::write(
        path.with_file_name("runtime.json"),
        "invalid synthetic binding",
    )
    .unwrap();
    let out = f.run(&["reload", "--yes"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("bound sandbox"));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn disabled_and_legacy_configs_never_gain_harness_mounts() {
    let f = Fixture::new();
    symlink(
        "/unavailable-synthetic-plugin",
        f.source.join("plugins/bad"),
    )
    .unwrap();
    f.success(&["init", "--yes", "--harness-config", "disabled"]);
    assert_eq!(f.plan()["harness"]["mounts"], json!([]));
    let path = f.config_path();
    let mut doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    doc.as_object_mut().unwrap().remove("harness");
    fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    assert!(f.plan()["harness"].is_null());
}

#[test]
fn source_links_hardlinks_special_entries_and_unsafe_permissions_are_refused() {
    for kind in ["symlink", "hardlink", "fifo", "writable"] {
        let f = Fixture::new();
        let path = f.source.join("plugins/bad.js");
        match kind {
            "symlink" => symlink("local/index.js", &path).unwrap(),
            "hardlink" => fs::hard_link(f.source.join("plugins/local/index.js"), &path).unwrap(),
            "fifo" => {
                use std::ffi::CString;
                use std::os::unix::ffi::OsStrExt;
                let name = CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
            "writable" => {
                fs::write(&path, "synthetic").unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
            }
            _ => unreachable!(),
        }
        let out = f.run(&["init", "--yes"]);
        assert!(!out.status.success(), "{kind}: {out:?}");
        assert!(!String::from_utf8_lossy(&out.stderr).contains(SYNTHETIC_SECRET));
        assert!(!f.home.join("config/dungeon").exists());
    }
}

#[test]
fn serialized_mount_budget_refuses_before_scope_approval_or_snapshot_capture() {
    for mode in ["read-only", "copy"] {
        let f = Fixture::new();
        if mode == "copy" {
            f.success(&["init", "--yes", "--harness-config", mode]);
        }
        for i in 0..300 {
            fs::write(
                f.source
                    .join(format!("plugins/{i:04}-{}.js", "x".repeat(120))),
                "synthetic plugin",
            )
            .unwrap();
        }
        let out = if mode == "read-only" {
            f.run(&["init", "--yes", "--harness-config", mode])
        } else {
            f.run(&["reload", "--yes"])
        };
        assert!(!out.status.success(), "{out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("exceeds 48 KiB"));
        assert!(
            out.stdout.is_empty(),
            "oversized scope must refuse before approval"
        );
        if mode == "read-only" {
            assert!(!f.home.join("config/dungeon").exists());
        } else {
            let config = f.config();
            assert!(config["values"]["harness"]["snapshot"].is_null());
            let parent = f.config_path().parent().unwrap().to_path_buf();
            assert!(!parent.join("harness").exists());
            assert!(!fs::read_dir(parent).unwrap().any(|p| {
                p.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("harness-review-")
            }));
        }
    }
}

#[test]
fn snapshot_integrity_is_checked_before_runtime_and_does_not_require_original_sources() {
    let f = Fixture::new();
    f.success(&["init", "--yes", "--harness-config", "copy"]);
    f.success(&["reload", "--yes"]);
    let copied = PathBuf::from(f.plan()["harness"]["mounts"][0]["source"].as_str().unwrap());
    fs::remove_dir_all(&f.source).unwrap();
    fs::remove_file(&f.auth).unwrap();
    let out = f.run(&["up"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("install the OpenShell"),
        "snapshot verification should not need live sources: {out:?}"
    );
    fs::set_permissions(&copied, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&copied, "tampered synthetic fixture").unwrap();
    let out = f.run(&["up"]);
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("install the OpenShell"));
    assert!(!f.config_path().with_file_name("runtime.json").exists());
    // Offline plan reports saved authority, not a successful integrity observation.
    f.success(&["plan"]);
}

#[test]
fn selinux_preflight_precedes_runtime_contact_and_preserves_control_independence() {
    let enforcing = fs::read("/sys/fs/selinux/enforce")
        .map(|bytes| bytes != b"0")
        .unwrap_or_else(|e| e.kind() != std::io::ErrorKind::NotFound);
    for mode in ["read-only", "copy", "disabled", "empty"] {
        let f = Fixture::new();
        if mode == "empty" {
            fs::remove_dir_all(&f.source).unwrap();
            fs::remove_file(&f.auth).unwrap();
        }
        f.success(&[
            "init",
            "--yes",
            "--project-transfer",
            "copy",
            "--harness-config",
            if mode == "empty" { "read-only" } else { mode },
        ]);
        if mode == "copy" {
            f.success(&["reload", "--yes"]);
        }
        let path = f.config_path();
        let before = fs::read(&path).unwrap();
        let bin = f.home.join("bin");
        fs::create_dir(&bin).unwrap();
        let tool = bin.join("openshell");
        fs::write(
            &tool,
            "#!/bin/sh\nprintf contacted > \"$HOME/openshell-contact\"\nexit 97\n",
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
        let marker = f.home.join("openshell-contact");
        for command in [
            vec!["up"],
            vec!["start"],
            vec!["exec", "--", "true"],
            vec!["machine", "--schema", "1", "up", "--yes"],
        ] {
            let out = f.run_with_path(&command, bin.to_str().unwrap());
            assert!(!out.status.success(), "{mode}: {command:?}: {out:?}");
            if mode == "read-only" && enforcing {
                assert!(!marker.exists(), "refusal must precede even version lookup");
                if command[0] != "machine" {
                    assert!(
                        String::from_utf8_lossy(&out.stderr).contains("SELinux"),
                        "{out:?}"
                    );
                }
            } else {
                assert!(marker.exists(), "{mode}: {command:?}: {out:?}");
                fs::remove_file(&marker).unwrap();
            }
            assert_eq!(fs::read(&path).unwrap(), before);
            assert!(!path.with_file_name("runtime.json").exists());
        }
        for command in [
            vec!["status"],
            vec!["down"],
            vec!["destroy", "--yes"],
            vec!["machine", "--schema", "1", "status"],
            vec!["machine", "--schema", "1", "stop"],
        ] {
            let out = f.run_with_path(&command, bin.to_str().unwrap());
            assert!(!out.status.success());
            assert!(
                marker.exists(),
                "control must bypass import checks: {command:?}: {out:?}"
            );
            fs::remove_file(&marker).unwrap();
        }
        f.success(&["plan"]);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
