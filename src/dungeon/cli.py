"""Terminal-first CLI; external application interfaces remain optional."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

from . import __version__
from .config import AGENTS, DEFAULT_IMAGE, OPEN_SHELL_VERSION, PROFILES, DungeonError
from .config import load_config, safe_name, starter_policy, write_new
from .runtime import Runtime, create_arguments


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description="Manage agent environments with OpenShell (engineering preview)")
    root.add_argument("--version", action="version", version=__version__)
    root.add_argument("--config", type=Path, help="explicit private dungeon.json; no project discovery")
    sub = root.add_subparsers(dest="action", required=True)
    init = sub.add_parser("init", help="create configuration without starting a runtime")
    init.add_argument("--directory", type=Path, required=True)
    init.add_argument("--project", type=Path, required=True)
    init.add_argument("--name")
    init.add_argument("--gateway")
    init.add_argument("--workspace", default="default")
    init.add_argument("--image", default=DEFAULT_IMAGE)
    init.add_argument("--agent", choices=AGENTS, default="opencode")
    init.add_argument("--profile", choices=PROFILES, default="development")
    init.add_argument("--provider", action="append", default=[])
    init.add_argument("--non-interactive", action="store_true")
    for name in ("plan", "create", "status", "start", "stop", "shell", "code", "ssh-config", "telltale"):
        sub.add_parser(name)
    upload = sub.add_parser("upload", help="explicitly transfer the selected project into the guest")
    upload.add_argument("--yes", action="store_true", help="confirm transfer and possible guest overwrite")
    auth = sub.add_parser("auth", help="run a vendor-native login flow inside the environment")
    auth.add_argument("agent", choices=("claude", "codex"))
    run = sub.add_parser("run", help="run a known agent directly, without a remote UI attachment")
    run.add_argument("agent", nargs="?", choices=AGENTS)
    run.add_argument("arguments", nargs=argparse.REMAINDER)
    execute = sub.add_parser("exec", help="execute an arbitrary command inside the environment")
    execute.add_argument("--tty", action="store_true")
    execute.add_argument("arguments", nargs=argparse.REMAINDER)
    return root


def initialize(args: argparse.Namespace) -> int:
    project = args.project.expanduser().resolve(strict=True)
    directory = args.directory.expanduser().resolve()
    if not project.is_dir() or project == Path(project.anchor) or project == Path.home().resolve():
        raise DungeonError("Select a project directory, not a filesystem root or your home")
    if directory.is_relative_to(project):
        raise DungeonError("Configuration must be outside the project")
    if directory.exists():
        raise DungeonError("Use a new configuration directory; existing directories are never overwritten")
    if not args.non_interactive:
        if not sys.stdin.isatty():
            raise DungeonError("Use --non-interactive --name NAME --gateway NAME without a terminal")
        def ask(label: str, default: str) -> str:
            return input(label + " [" + default + "]: ").strip() or default
        args.name = ask("Environment name", args.name or "development")
        args.gateway = ask("Registered OpenShell gateway", args.gateway or "local")
        args.image = ask("Reviewed OCI image", args.image)
        args.agent = ask("Default agent (opencode/claude/codex)", args.agent)
        args.profile = ask("Starter policy (minimal/development)", args.profile)
    for field in ("name", "gateway", "workspace"):
        safe_name(getattr(args, field), field)
    for provider in args.provider:
        safe_name(provider, "provider")
    if args.agent not in AGENTS:
        raise DungeonError("Unknown default agent")
    policy = starter_policy(args.profile)
    directory.mkdir(parents=True, mode=0o700)
    write_new(directory / "policy.json", policy)
    write_new(directory / "dungeon.json", {
        "schema": 1, "name": args.name, "gateway": args.gateway, "workspace": args.workspace,
        "project": str(project), "image": args.image, "agent": args.agent,
        "providers": args.provider, "policy": "policy.json",
    })
    config = load_config(directory / "dungeon.json")
    print("Configuration: " + str(config.path))
    print("Review policy.json and named providers, then run plan. No runtime was started.")
    return 0


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.action == "init":
            return initialize(args)
        if args.config is None:
            raise DungeonError("--config is required; project-local configuration is never auto-loaded")
        config = load_config(args.config)
        if args.action == "plan":
            print(json.dumps({
                "adapter": "openshell", "version": OPEN_SHELL_VERSION,
                "fingerprint": config.fingerprint,
                "command": ["openshell", "--gateway", config.values["gateway"], "--workspace", config.values["workspace"]]
                    + create_arguments(config, config.policy_path),
                "policy": config.policy,
                "project_transfer": "separate explicit upload; no host bind mount",
                "telemetry": "optional in-guest diagnostic only; no durable collection",
                "verification": "plan only; no backend contacted or enforcement verified",
            }, indent=2))
            return 0
        if args.action == "upload" and not args.yes:
            raise DungeonError("Review project contents, including secrets and symlinks, then repeat upload --yes")
        runtime = Runtime(config)
        name = config.values["name"]
        if args.action == "create":
            return runtime.create()
        if args.action == "status":
            print(json.dumps(runtime.get(), indent=2))
            return 0
        if args.action == "upload":
            return runtime.upload()
        if args.action == "auth":
            if args.agent == "codex":
                print(
                    "Starting Codex device authentication inside the guest. Complete the displayed "
                    "URL/code in your browser. Dungeon never reads or stores the resulting credential.",
                    file=sys.stderr,
                )
                return runtime.execute(["codex", "login", "--device-auth"], tty=True, workdir="/sandbox")
            print(
                "Starting Claude Code inside the guest. Use /login and complete the vendor browser "
                "flow. Dungeon never reads or stores the resulting credential.",
                file=sys.stderr,
            )
            return runtime.execute(["claude"], tty=True, workdir="/sandbox")
        if args.action in ("run", "exec"):
            command = args.arguments
            if command[:1] == ["--"]:
                command = command[1:]
            if args.action == "run":
                command = [args.agent or config.values["agent"]] + command
            tty = sys.stdin.isatty() and sys.stdout.isatty() if args.action == "run" else args.tty
            return runtime.execute(command, tty=tty)
        if args.action == "shell":
            return runtime.execute(["/bin/bash", "-l"], tty=True, workdir="/sandbox")
        if args.action == "telltale":
            print("In-guest diagnostic, not an independent audit or durable collector.", file=sys.stderr)
            return runtime.execute([
                "telltale", "scan", "--once", "--dry-run", "--no-local-config", "--root", "/sandbox",
            ], workdir="/sandbox")
        runtime.checked(ready=args.action not in ("start", "stop"), policy=args.action != "stop")
        if args.action == "code":
            return runtime.call(["sandbox", "connect", name, "--editor", "vscode"], timeout=None)
        if args.action == "ssh-config":
            return runtime.call(["sandbox", "ssh-config", name])
        return runtime.call(["sandbox", args.action, name], timeout=120)
    except (DungeonError, OSError, UnicodeError, subprocess.TimeoutExpired) as exc:
        print("dungeon: " + str(exc), file=sys.stderr)
        return 1
    except (KeyboardInterrupt, EOFError):
        print("\ndungeon: interrupted; existing sandbox and project are retained", file=sys.stderr)
        return 130
