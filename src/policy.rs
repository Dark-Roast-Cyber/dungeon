use crate::{Error, Result};
use serde_json::{Value, json};

pub(crate) fn starter_policy() -> Value {
    json!({
        "version": 1,
        "filesystem_policy": {
            "include_workdir": true,
            "read_only": ["/usr", "/lib", "/proc", "/dev/urandom", "/app", "/etc", "/var/log"],
            "read_write": ["/sandbox", "/tmp", "/dev/null"]
        },
        "landlock": {"compatibility": "hard_requirement"},
        "process": {"run_as_user": "sandbox", "run_as_group": "sandbox"},
        "network_policies": {
            "default": {
                "name": "default",
                "endpoints": [
                    {"host":"api.anthropic.com","port":443},
                    {"host":"api.openai.com","port":443},
                    {"host":"auth.openai.com","port":443},
                    {"host":"chatgpt.com","port":443},
                    {"host":"claude.ai","port":443},
                    {"host":"opencode.ai","port":443},
                    {"host":"models.dev","port":443},
                    {"host":"github.com","port":443},
                    {"host":"api.github.com","port":443},
                    {"host":"raw.githubusercontent.com","port":443},
                    {"host":"registry.npmjs.org","port":443},
                    {"host":"pypi.org","port":443},
                    {"host":"files.pythonhosted.org","port":443},
                    {"host":"crates.io","port":443},
                    {"host":"index.crates.io","port":443},
                    {"host":"static.crates.io","port":443},
                    {"host":"update.code.visualstudio.com","port":443},
                    {"host":"vscode.download.prss.microsoft.com","port":443},
                    {"host":"marketplace.visualstudio.com","port":443}
                ],
                "binaries": [
                    {"path":"/usr/bin/**"},
                    {"path":"/usr/local/bin/**"},
                    {"path":"/usr/lib/node_modules/**"},
                    {"path":"/sandbox/**"}
                ]
            }
        }
    })
}

pub(crate) fn validate_policy(policy: &Value) -> Result<()> {
    let object = policy
        .as_object()
        .ok_or_else(|| Error::new("policy must be a JSON object"))?;

    if object.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(Error::new("expected OpenShell policy version 1"));
    }

    if object
        .get("landlock")
        .and_then(Value::as_object)
        .and_then(|value| value.get("compatibility"))
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
            .and_then(|value| value.get("endpoints"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_policy_requires_hard_landlock() {
        let mut policy = starter_policy();
        policy["landlock"]["compatibility"] = json!("best_effort");
        assert!(validate_policy(&policy).is_err());
    }

    #[test]
    fn starter_policy_has_one_network_policy() {
        let policy = starter_policy();
        let network = policy["network_policies"].as_object().unwrap();
        assert_eq!(network.len(), 1);
        assert!(network.contains_key("default"));
    }
}
