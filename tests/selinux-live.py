"""Synthetic native SELinux gate; run with uv and an explicitly disposable HOME.

This verifies the developer default and private-mode behavior, not AVC attribution. Never
changes global enforcement, existing bindings, images, services or host labels.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess


def command(argv, check=True):
    result = subprocess.run(argv, capture_output=True, text=True, timeout=150, cwd=home)
    if check and result.returncode:
        raise RuntimeError(f"{argv[0]} {argv[1]}: {result.returncode}: {result.stderr}")
    return result


assert os.environ.get("DUNGEON_SELINUX_TEST") == "disposable-home"
home = Path(os.environ["HOME"]).resolve()
binary = Path(os.environ["DUNGEON_CONTAINER_BINARY"]).resolve(strict=True)
endpoint = os.environ["DUNGEON_CONTAINER_ENDPOINT"]
image = os.environ["DUNGEON_CONTAINER_IMAGE"]
assert home.is_dir() and home != Path("/") and not (home / ".config").exists()
assert home.stat().st_uid == os.getuid() and home.stat().st_mode & 0o077 == 0
assert {entry.name for entry in home.iterdir()} <= {".cache"}
assert image.startswith("localhost/") or "@sha256:" in image
assert command(["getenforce"]).stdout.strip() == "Enforcing"
native = ["podman", "--remote", "--url", endpoint]
info = json.loads(command([*native, "info", "--format", "{{json .}}"]).stdout)
assert info["host"]["security"]["rootless"] is True
assert info["host"]["security"]["selinuxEnabled"] is True
inventory_args = [
    *native,
    "ps",
    "--all",
    "--no-trunc",
    "--format",
    "{{.ID}} {{.Names}} {{.State}} {{.StartedAt}}",
]
before_inventory = command(inventory_args).stdout
receipt = {
    "podman": command(["podman", "--version"]).stdout.strip(),
    "podman_service": info.get("version", {}).get("Version"),
    "kernel": os.uname().release,
    "selinux": "Enforcing",
    "image": image,
    "avc": "not-collected; diagnostic limitation, not a behavioral acceptance blocker",
}
configs = []


def dungeon(config, *args, check=True):
    return command([str(binary), "--config", str(config), *args], check=check)


def metadata(path):
    s = path.stat()
    return {
        "inode": s.st_ino,
        "device": s.st_dev,
        "uid": s.st_uid,
        "gid": s.st_gid,
        "mode": s.st_mode & 0o7777,
        "label": os.getxattr(path, "security.selinux").decode().rstrip("\0"),
    }


def write(path, text):
    path.write_text(text)
    path.chmod(0o644)


def initialize(mode, imports=False):
    project = home / f"{mode}-project"
    project.mkdir(mode=0o700)
    write(project / "existing.txt", "existing\n")
    config = home / f"{mode}-config" / "dungeon.json"
    args = [
        str(binary),
        "init",
        "--yes",
        "--agent",
        "opencode",
        "--project",
        str(project),
        "--directory",
        str(config.parent),
        "--name",
        f"selinux-{mode}",
        "--runtime",
        "container",
        "--engine",
        "podman",
        "--endpoint",
        endpoint,
        "--image",
        image,
    ]
    if mode == "private":
        args.extend(["--selinux-isolation", "private"])
    if imports:
        args.extend(["--harness-config", "copy"])
    command(args)
    configs.append(config)
    return config, project


try:
    source = home / ".config/opencode/themes/fixture.json"
    source.parent.mkdir(parents=True, mode=0o700)
    write(source, '{"fixture":true}\n')
    original_import = metadata(source)
    config, project = initialize("disabled", imports=True)
    assert json.loads(config.read_text())["selinux_isolation"] == "disabled"
    plan = json.loads(dungeon(config, "plan").stdout)
    snapshot = Path(plan["configuration_mounts"][0]["source"])
    label_before = {
        str(p): metadata(p)["label"]
        for p in [project, project / "existing.txt", snapshot]
    }
    dungeon(config, "up")
    status = json.loads(dungeon(config, "status").stdout)
    container_id = status["id"]
    inspected = json.loads(
        command([*native, "container", "inspect", container_id]).stdout
    )[0]
    assert "label=disable" in inspected["HostConfig"]["SecurityOpt"]
    assert "no-new-privileges" in inspected["HostConfig"]["SecurityOpt"]
    assert inspected["ProcessLabel"] == ""
    assert inspected["Config"]["User"] == "998:998"
    assert isinstance(inspected["BoundingCaps"], list)
    assert "CAP_NET_RAW" not in inspected["BoundingCaps"]
    assert "998:0:1" in inspected["HostConfig"]["IDMappings"]["UidMap"]
    assert "998:0:1" in inspected["HostConfig"]["IDMappings"]["GidMap"]
    assert inspected["HostConfig"]["Privileged"] is False
    assert [m["Destination"] for m in inspected["Mounts"] if m["RW"]] == [
        "/sandbox/project"
    ]
    assert status["selinux_isolation"]["verification"] == "native-configuration"
    for path, label in label_before.items():
        assert metadata(Path(path))["label"] == label
    assert metadata(source) == original_import
    assert (
        dungeon(config, "exec", "--", "cat", "/sandbox/project/existing.txt").stdout
        == "existing\n"
    )

    write(project / "created.txt", "created\n")
    write(home / "copy-source.txt", "copied\n")
    shutil.copyfile(home / "copy-source.txt", project / "copied.txt")
    (project / "copied.txt").chmod(0o644)
    incoming = home / "incoming.txt"
    write(incoming, "moved\n")
    moved_before = metadata(incoming)
    incoming.rename(project / "moved.txt")
    assert metadata(project / "moved.txt") == moved_before
    write(home / "replacement.txt", "replaced\n")
    replaced_before = metadata(home / "replacement.txt")
    os.replace(home / "replacement.txt", project / "existing.txt")
    assert metadata(project / "existing.txt") == replaced_before
    expected = {
        "existing.txt": "replaced\n",
        "created.txt": "created\n",
        "copied.txt": "copied\n",
        "moved.txt": "moved\n",
    }
    for name, text in expected.items():
        assert (
            dungeon(config, "exec", "--", "cat", f"/sandbox/project/{name}").stdout
            == text
        )
        observed = metadata(project / name)
        assert observed["uid"] == os.getuid() and observed["gid"] == os.getgid()
        assert observed["mode"] == 0o644
    dungeon(
        config,
        "exec",
        "--",
        "/bin/sh",
        "-c",
        'set -eu; umask 022; printf "guest\\n" > /sandbox/project/guest.txt; '
        'printf "edited\\n" > /sandbox/project/created.txt',
    )
    assert (project / "created.txt").read_text() == "edited\n"
    guest = metadata(project / "guest.txt")
    assert (
        guest["uid"] == os.getuid()
        and guest["gid"] == os.getgid()
        and guest["mode"] == 0o644
    )
    write(project / "guest.txt", "host-edited\n")
    assert (
        dungeon(config, "exec", "--", "cat", "/sandbox/project/guest.txt").stdout
        == "host-edited\n"
    )
    dungeon(
        config,
        "exec",
        "--",
        "/bin/sh",
        "-c",
        'set -eu; test ! -e "$1"; '
        "test ! -e /run/podman/podman.sock; test ! -e /var/run/docker.sock; "
        'test ! -e /sandbox/.ssh; test "$(id -u)" = 998; '
        'grep -q "^NoNewPrivs:[[:space:]]*1" /proc/self/status; '
        'grep -q "^Seccomp:[[:space:]]*2" /proc/self/status; '
        "if (printf forbidden > /sandbox/.config/opencode/themes/fixture.json) 2>/dev/null; then exit 1; fi",
        "selinux-gate",
        str(home / "copy-source.txt"),
    )
    assert snapshot.read_text() == '{"fixture":true}\n'
    state_before = config.with_name("runtime.json").read_bytes()
    dungeon(config, "down")
    dungeon(config, "up")
    assert config.with_name("runtime.json").read_bytes() == state_before
    assert (
        dungeon(config, "exec", "--", "cat", "/sandbox/project/moved.txt").stdout
        == "moved\n"
    )
    assert (
        dungeon(config, "exec", "--", "cat", "/sandbox/project/guest.txt").stdout
        == "host-edited\n"
    )
    assert metadata(source) == original_import
    assert metadata(snapshot)["label"] == label_before[str(snapshot)]
    assert metadata(project)["label"] == label_before[str(project)]
    assert metadata(project / "existing.txt") == replaced_before
    assert metadata(project / "moved.txt") == moved_before
    receipt["disabled"] = {
        "selection": "new-binding-default",
        "id": container_id,
        "status": status["selinux_isolation"],
        "file_cases": list(expected),
        "host_labels_unchanged": True,
        "read_only_import": True,
        "same_id_resume": True,
    }

    private_config, private_project = initialize("private")
    write(home / "private-incoming.txt", "private-move\n")
    private_before = metadata(home / "private-incoming.txt")
    dungeon(private_config, "up")
    private_status = json.loads(dungeon(private_config, "status").stdout)
    assert ":container_t:s0:c" in private_status["selinux_isolation"]["process_label"]
    assert (
        ":container_file_t:s0:c" in metadata(private_project / "existing.txt")["label"]
    )
    assert (
        dungeon(
            private_config, "exec", "--", "cat", "/sandbox/project/existing.txt"
        ).stdout
        == "existing\n"
    )
    (home / "private-incoming.txt").rename(private_project / "moved.txt")
    assert metadata(private_project / "moved.txt") == private_before
    denial = dungeon(
        private_config, "exec", "--", "cat", "/sandbox/project/moved.txt", check=False
    )
    assert denial.returncode == 1 and "Permission denied" in denial.stderr
    receipt["private"] = {
        "status": private_status["selinux_isolation"],
        "moved_file": private_before,
        "moved_open_denied": True,
    }
finally:
    for config in reversed(configs):
        if config.with_name("runtime.json").exists():
            dungeon(config, "down")
            dungeon(config, "destroy", "--yes")
    receipt["preexisting_inventory_unchanged"] = (
        command(inventory_args).stdout == before_inventory
    )
    receipt["selinux_after"] = command(["getenforce"]).stdout.strip()
    (home / "selinux-result.json").write_text(json.dumps(receipt, indent=2) + "\n")
    assert receipt["preexisting_inventory_unchanged"]
    assert receipt["selinux_after"] == "Enforcing"

print(
    "PASS: Podman developer default and private mode, live files, unchanged host labels/enforcement, read-only imports and resume"
)
