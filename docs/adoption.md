# Dungeon adoption cheat sheet (source preview)

Dungeon 0.0.2 is **source-only**. Do not use the published v0.0.1 Python package
or unpublished npm commands for this Rust workflow. Supported local path:
Linux, OpenShell **0.0.116**, rootless Podman, and Rust **1.98.1** for building.
This is not a general containment or platform certification.

## 1. Install the CLI, not a gateway

Install Git, Rust/cargo, OpenShell 0.0.116 and rootless Podman yourself. From a
reviewed Dungeon checkout:

```sh
./scripts/install-local.sh --dry-run
./scripts/install-local.sh
export PATH="$HOME/.local/bin:$PATH"
command -v dungeon
dungeon --version
```

The helper builds with `cargo build --release --locked` and installs only the
binary. It does **not** change PATH permanently, install dependencies, start a
gateway, pull an image, copy auth, or touch containers. If `~/.local/bin/dungeon`
already holds a different binary, it refuses. Compare/review it first, then use
`./scripts/install-local.sh --replace` to approve replacement. `--bin-dir PATH`
selects another absolute user-owned directory. Build/install from the checkout
you intend to run; uncommitted source changes are included. For a one-shell
trial without installation, use `cargo build --release --locked` followed by
`export PATH="$PWD/target/release:$PATH"`.

## 2. Inspect, then set up OpenShell

```sh
dungeon doctor                   # read-only; inspect individual checks
systemctl --user enable --now podman.socket
podman pull ghcr.io/nvidia/openshell/gateway@sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645
dungeon setup --dry-run
dungeon setup                    # fresh bind-capable gateway; never mounts a project itself
dungeon doctor
```

`setup` does not repair or restart an existing gateway. If doctor finds the
`openshell-gateway-dungeon-local` container **stopped**, inspect its label,
mounts, network and port with Podman before explicitly starting it. Do not
delete its database, register a replacement over it, or use `podman prune`.
An existing copy-only gateway cannot be turned into a live-capable gateway by
plain `setup`. Use `setup --copy-only` to verify it and
`init --project-transfer copy` for a new project there; fresh bind capability
needs an explicit disposal/reprovision decision. Doctor health is not proof of isolation.

## 3. Choose a project and agent

```sh
cd /path/to/project
dungeon                         # review project mount, attach host UI to guest OpenCode
# OR, on a new project:
dungeon run claude              # review binding, choose Claude as default
dungeon run codex               # choose Codex for this invocation on an existing binding
```

First use requires a terminal for review. `dungeon init` writes the private
config without starting anything; use it **before** first launch to choose an
image, transfer mode, gateway, or provider:

```sh
dungeon init --agent claude       # other settings use reviewed defaults
dungeon plan                     # inspect policy and requested mounts
dungeon                           # first up, then the selected agent
```

The default reviewed candidate must be [built locally](../images/agent/README.md)
before `up`; it is not published or pulled by Dungeon. Use `dungeon versions`
to compare the host CLI and reviewed guest pin, or `versions --check` to check
the registry stable tag. Do not rely on host tools or host auth in the guest.
Claude/Codex login is guest-owned (`dungeon auth claude|codex` after `up`);
OpenCode uses its own guest `/connect` flow. Changing image, policy, or scope
after creation fails identity checks rather than rebuilding in place.

## 4. Daily use and deliberate disposal

For a configured OpenCode project, interactive `dungeon` (or explicit
`dungeon attach`) starts an authenticated guest service, opens a temporary
host-loopback forward and attaches your installed host OpenCode UI. It checks
that host and guest versions match; it does not install or replace either CLI.
Dungeon pauses at `Press Enter to open OpenCode` so the startup and connection
details stay visible before the host UI takes over the terminal. Ctrl-C there
cancels attachment and cleans up the service and forward.
Exit the host UI or press Ctrl-C to stop the service and forward, keeping the
sandbox. If cleanup cannot be confirmed, choose to stop the sandbox (preserves
data; name confirmation required) or leave it running, then inspect it. A
noninteractive invocation refuses attachment. `dungeon run opencode` remains
the direct guest TUI; the [manual service/forward flow](configuration.md#remote-first-attachment)
remains available. Codex and Claude's vendor-specific remote modes need their
own validation and still run as guest CLIs.

| Need | Command |
| --- | --- |
| See bindings in/under this directory | `dungeon list` (offline JSON) |
| See all private bindings | `dungeon list -a` (not all OpenShell sandboxes) |
| Inspect this project's actual sandbox | `dungeon status` |
| Run another guest tool | `dungeon exec -- cargo test` |
| Enter the guest shell | `dungeon shell` |
| Return copy-mode guest edits | `dungeon export` (review/approve) |
| Send host edits to the guest | `dungeon upload --yes` (may overwrite guest edits) |
| Stop, keep guest files/auth/sessions | `dungeon down` |
| Delete a finished bound sandbox | `dungeon down`, then `dungeon destroy` |

**Live is the new CLI default:** after binding review, `up` requests one writable
mount of the selected project on a compatible bind-enabled gateway. It does not
mount your home or engine socket. Guest changes reach the host immediately;
upload/export do not apply. For a sandbox copy instead, explicitly choose
`dungeon init --project-transfer copy`: first `up` uploads once, resumed launches
do not sync, and `export` is reviewed. Choose before creation. Old configurations
that omit the transfer field remain copy-mode.

`destroy` loses guest files, sessions and auth, but retains private configuration
for a fresh `up`. Export first if using copy mode. It is not a gateway or orphan
cleanup command. Installing a new CLI does not migrate an existing binding or
gateway. See [configuration](configuration.md) for detailed limits and recovery.
