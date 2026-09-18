"""The sole subprocess boundary, pinned to OpenShell's 0.0.116 CLI."""

from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile
from typing import Any

from .config import Config, DungeonError, GUEST_PROJECT, LABEL, OPEN_SHELL_VERSION
from .config import decode, digest, read_json, validate_policy, write_new


def host_environment(project: Path) -> dict[str, str]:
    allowed = {
        "HOME", "USER", "LOGNAME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA",
        "SystemRoot", "SYSTEMROOT", "WINDIR", "PATHEXT", "TERM", "COLORTERM", "LANG", "LC_ALL",
        "LC_CTYPE", "TMPDIR", "TMP", "TEMP", "XDG_CONFIG_HOME", "XDG_DATA_HOME",
        "XDG_STATE_HOME", "XDG_CACHE_HOME", "SSH_AUTH_SOCK",
        "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
    }
    env = {key: value for key, value in os.environ.items() if key in allowed}
    paths = []
    for entry in os.environ.get("PATH", os.defpath).split(os.pathsep):
        path = Path(entry)
        if entry and path.is_absolute() and not path.resolve().is_relative_to(project):
            paths.append(str(path.resolve()))
    env.update(PATH=os.pathsep.join(paths), NO_COLOR="1")
    return env


def create_arguments(config: Config, policy_path: Path) -> list[str]:
    value = config.values
    args = [
        "sandbox", "create", "--name", value["name"], "--from", value["image"],
        "--policy", str(policy_path), "--no-auto-providers", "--approval-mode", "manual",
        "--label", LABEL + "=" + config.fingerprint, "--detach", "--output", "json",
    ]
    for provider in value["providers"]:
        args.extend(["--provider", provider])
    return args


class Runtime:
    def __init__(self, config: Config):
        self.config = config
        self.env = host_environment(config.project)
        binary = shutil.which("openshell", path=self.env["PATH"])
        if not binary:
            raise DungeonError("Install the OpenShell 0.0.116 CLI outside the project and add it to PATH")
        self.binary = str(Path(binary).resolve())
        if Path(self.binary).is_relative_to(config.project):
            raise DungeonError("Refusing a project-owned OpenShell executable")
        version = self.call(["--version"], capture=True, scoped=False).strip()
        if not re.fullmatch(r"openshell\s+v?0\.0\.116", version):
            raise DungeonError("This adapter requires OpenShell " + OPEN_SHELL_VERSION)

    def call(self, args: list[str], *, capture: bool = False, scoped: bool = True, timeout: int | None = 30) -> Any:
        prefix = [self.binary]
        if scoped:
            prefix += ["--gateway", self.config.values["gateway"], "--workspace", self.config.values["workspace"]]
        result = subprocess.run(
            prefix + args, cwd=self.config.path.parent, env=self.env, shell=False,
            capture_output=capture, text=capture, timeout=timeout,
        )
        if capture:
            if result.returncode:
                # Keep arbitrary backend stderr out of structured errors and logs.
                raise DungeonError("OpenShell failed (exit " + str(result.returncode) + "); inspect the gateway directly")
            return result.stdout
        return result.returncode if result.returncode >= 0 else 128 - result.returncode

    def get(self) -> dict[str, Any]:
        value = decode(self.call(["sandbox", "get", self.config.values["name"], "--output", "json"], capture=True))
        if not isinstance(value, dict):
            raise DungeonError("Unexpected OpenShell sandbox response")
        return value

    def check_identity(self, remote: dict[str, Any]) -> None:
        value = self.config.values
        if (remote.get("name") != value["name"] or remote.get("workspace") != value["workspace"]
                or not isinstance(remote.get("id"), str) or not remote["id"]
                or not isinstance(remote.get("labels"), dict)
                or remote["labels"].get(LABEL) != self.config.fingerprint):
            raise DungeonError("Sandbox identity or configuration label does not match; refusing reuse")

    def checked(self, *, ready: bool = True, policy: bool = True) -> dict[str, Any]:
        state = read_json(self.config.state_path)
        if type(state.get("schema")) is not int or state["schema"] != 1 or state.get("config") != self.config.fingerprint:
            raise DungeonError("Launch configuration changed; restore it or initialize a new environment")
        remote = self.get()
        self.check_identity(remote)
        if remote["id"] != state.get("id"):
            raise DungeonError("Sandbox was replaced; refusing to attach to a different instance")
        if policy and digest(remote.get("policy")) != state.get("observed_policy"):
            raise DungeonError("Effective backend policy changed; inspect it before further execution")
        if ready and remote.get("phase") != "Ready":
            raise DungeonError("Sandbox is not Ready; inspect status or start it explicitly")
        return remote

    def create(self) -> int:
        if self.config.state_path.exists():
            self.checked(ready=False)
            print("Environment already exists; no changes made.")
            return 0
        # Freeze exactly the reviewed policy bytes for the create operation.
        with tempfile.TemporaryDirectory(prefix="create-", dir=self.config.path.parent) as directory:
            policy_path = Path(directory) / "policy.json"
            write_new(policy_path, self.config.policy)
            self.call(create_arguments(self.config, policy_path), capture=True, timeout=600)
        remote = self.get()
        self.check_identity(remote)
        if remote.get("phase") != "Ready" or remote.get("policy_source") != "sandbox":
            raise DungeonError("Created sandbox is not Ready with a sandbox policy; inspect it before use")
        policy = remote.get("policy")
        if not isinstance(policy, dict):
            raise DungeonError("Created sandbox did not return an effective policy")
        validate_policy(policy)
        write_new(self.config.state_path, {
            "schema": 1, "id": remote["id"], "config": self.config.fingerprint,
            "observed_policy": digest(policy),
        })
        print("Created " + self.config.values["name"] + ". Project upload is a separate action.")
        return 0

    def execute(self, command: list[str], *, tty: bool = False, workdir: str = GUEST_PROJECT) -> int:
        if not command or not command[0] or any("\x00" in arg for arg in command):
            raise DungeonError("An executable and valid arguments are required")
        self.checked()
        args = ["sandbox", "exec", "--name", self.config.values["name"], "--workdir", workdir]
        if tty:
            args.append("--tty")
        return self.call(args + ["--"] + command, timeout=None)

    def upload(self) -> int:
        if not self.config.project.is_dir():
            raise DungeonError("The host project must exist for upload")
        self.checked()
        # Upstream transfer remains mutable; this is a preflight, not a snapshot.
        for parent, dirs, files in os.walk(self.config.project, followlinks=False):
            for name in dirs + files:
                path = Path(parent) / name
                mode = path.lstat().st_mode
                if stat.S_ISLNK(mode):
                    try:
                        target = path.resolve(strict=True)
                    except RuntimeError as exc:
                        raise DungeonError("Upload contains a symlink loop") from exc
                    if not target.is_relative_to(self.config.project):
                        raise DungeonError("Upload contains a symlink outside the selected project")
                elif not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                    raise DungeonError("Upload contains a socket, device, or other special file")
        return self.call([
            "sandbox", "upload", self.config.values["name"], str(self.config.project), GUEST_PROJECT,
        ], timeout=None)
