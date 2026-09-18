"""Explicit configuration and native OpenShell policy, with no discovery hooks."""

from __future__ import annotations

import base64
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
from typing import Any

OPEN_SHELL_VERSION = "0.0.116"
DEFAULT_IMAGE = "ghcr.io/nvidia/openshell-community/sandboxes/base:latest"
AGENTS = ("opencode", "claude", "codex")
PROFILES = ("minimal", "development")
LABEL = "agentarchaeology.ai/dungeon-config"
GUEST_PROJECT = "/sandbox/project"


class DungeonError(Exception):
    """An actionable configuration or runtime failure."""


def object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise DungeonError("Duplicate JSON key: " + key)
        result[key] = value
    return result


def decode(text: str) -> Any:
    def bad_constant(value: str) -> None:
        raise DungeonError("Non-finite JSON number is not supported")

    try:
        return json.loads(text, object_pairs_hook=object_pairs, parse_constant=bad_constant)
    except (ValueError, RecursionError) as exc:
        raise DungeonError("Invalid JSON") from exc


def read_json(path: Path) -> dict[str, Any]:
    with path.open("rb") as stream:
        content = stream.read(1_048_577)
    if len(content) > 1_048_576:
        raise DungeonError("Configuration exceeds 1 MiB")
    value = decode(content.decode("utf-8"))
    if not isinstance(value, dict):
        raise DungeonError("Expected a JSON object")
    return value


def digest(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    return base64.b32encode(hashlib.sha256(encoded).digest()).decode().rstrip("=").lower()


def write_new(path: Path, value: dict[str, Any]) -> None:
    """Never replace a configuration or follow an existing symlink."""
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    fd = os.open(path, flags, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def safe_name(value: Any, field: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", value):
        raise DungeonError(field + " must be a lowercase DNS label, 1 to 63 characters")
    return value


def validate_policy(policy: dict[str, Any]) -> None:
    if type(policy.get("version")) is not int or policy["version"] != 1:
        raise DungeonError("Expected OpenShell policy version 1")
    landlock = policy.get("landlock")
    if not isinstance(landlock, dict) or landlock.get("compatibility") != "hard_requirement":
        raise DungeonError("Dungeon requires landlock.compatibility=hard_requirement")
    process = policy.get("process")
    if not isinstance(process, dict) or any(
        process.get(key) != "sandbox" for key in ("run_as_user", "run_as_group")
    ):
        raise DungeonError("The preview image contract requires the sandbox user and group")
    network = policy.get("network_policies")
    if not isinstance(network, dict):
        raise DungeonError("network_policies must be an object")
    for block in network.values():
        if not isinstance(block, dict) or not isinstance(block.get("endpoints"), list):
            raise DungeonError("Each network policy needs an endpoints array")
        for endpoint in block["endpoints"]:
            if not isinstance(endpoint, dict):
                raise DungeonError("Network endpoints must be objects")
            inspected = endpoint.get("protocol") == "rest" or endpoint.get("tls") == "terminate"
            if inspected and endpoint.get("enforcement") != "enforce":
                raise DungeonError("Inspected endpoints must use enforcement=enforce")


@dataclass(frozen=True)
class Config:
    path: Path
    values: dict[str, Any]
    policy: dict[str, Any]
    policy_path: Path

    @property
    def project(self) -> Path:
        return Path(self.values["project"])

    @property
    def fingerprint(self) -> str:
        return digest({"config": self.values, "policy": self.policy, "adapter": OPEN_SHELL_VERSION})

    @property
    def state_path(self) -> Path:
        return self.path.parent / "runtime.json"


def load_config(path: Path) -> Config:
    path = path.expanduser().resolve(strict=True)
    value = read_json(path)
    fields = {"schema", "name", "gateway", "workspace", "project", "image", "agent", "providers", "policy"}
    if set(value) != fields or type(value.get("schema")) is not int or value["schema"] != 1:
        raise DungeonError("Expected configuration schema 1 and exactly: " + ", ".join(sorted(fields)))
    for field in ("name", "gateway", "workspace"):
        safe_name(value[field], field)
    if value["agent"] not in AGENTS:
        raise DungeonError("agent must be opencode, claude, or codex; use exec for other commands")
    if not isinstance(value["providers"], list):
        raise DungeonError("providers must be an array of names, never credentials")
    for provider in value["providers"]:
        safe_name(provider, "provider")
    if len(set(value["providers"])) != len(value["providers"]):
        raise DungeonError("Provider names must not repeat")
    if not isinstance(value["project"], str) or not Path(value["project"]).is_absolute():
        raise DungeonError("project must be an absolute path")
    project = Path(value["project"]).resolve()
    if (project.exists() and not project.is_dir()) or project == Path(project.anchor) or project == Path.home().resolve():
        raise DungeonError("Select a project directory, not a filesystem root or your home")
    if path.is_relative_to(project):
        raise DungeonError("Keep Dungeon configuration outside the uploaded project")
    image = value["image"]
    if not isinstance(image, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/@-]*", image):
        raise DungeonError("image must be a fully qualified OCI image reference")
    registry, separator, _ = image.partition("/")
    if not separator or not ("." in registry or ":" in registry or registry == "localhost"):
        raise DungeonError("Use a registry-qualified image, not a community name or local build path")
    if ".." in image.split("/") or (path.parent / image).exists():
        raise DungeonError("Local image build paths are not accepted")
    if value["policy"] != "policy.json":
        raise DungeonError("policy must name the adjacent policy.json file")
    policy_path = (path.parent / "policy.json").resolve(strict=True)
    if policy_path.parent != path.parent or policy_path.is_relative_to(project):
        raise DungeonError("Policy must stay in the private configuration directory")
    policy = read_json(policy_path)
    validate_policy(policy)
    value["project"] = str(project)
    return Config(path, value, policy, policy_path)


def starter_policy(profile: str) -> dict[str, Any]:
    if profile not in PROFILES:
        raise DungeonError("Unknown starter profile")
    policy: dict[str, Any] = {
        "version": 1,
        "filesystem_policy": {
            "include_workdir": True,
            "read_only": ["/usr", "/lib", "/proc", "/dev/urandom", "/app", "/etc", "/var/log"],
            "read_write": ["/sandbox", "/tmp", "/dev/null"],
        },
        "landlock": {"compatibility": "hard_requirement"},
        "process": {"run_as_user": "sandbox", "run_as_group": "sandbox"},
        "network_policies": {},
    }
    groups = {
        "models": ["api.anthropic.com", "api.openai.com", "auth.openai.com", "chatgpt.com", "claude.ai", "opencode.ai", "models.dev"],
    }
    if profile == "development":
        groups["development"] = [
            "github.com", "api.github.com", "raw.githubusercontent.com", "registry.npmjs.org",
            "pypi.org", "files.pythonhosted.org", "crates.io", "index.crates.io", "static.crates.io",
            "update.code.visualstudio.com", "vscode.download.prss.microsoft.com", "marketplace.visualstudio.com",
        ]
    for name, hosts in groups.items():
        policy["network_policies"][name] = {
            "name": name,
            "endpoints": [{"host": host, "port": 443} for host in hosts],
            "binaries": [{"path": p} for p in ("/usr/bin/**", "/usr/local/bin/**", "/usr/lib/node_modules/**", "/sandbox/**")],
        }
    return policy
