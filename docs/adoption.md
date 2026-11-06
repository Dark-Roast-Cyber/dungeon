# Dungeon adoption cheat sheet (source preview)

Dungeon 0.0.2 is **source-only**. Do not use the published v0.0.1 Python package
or unpublished npm commands for this Rust workflow. The supported local path is
Linux with rootless Podman or standard Docker CE, and Rust **1.98.1** for building.
OpenShell **0.0.116** is optional for policy enforcement, copy/export, SSH and
forwarding. This is an engineering preview, not containment certification.

## 1. Install the CLI, not a gateway

Install Git, Rust/cargo and your container engine yourself. From a reviewed
Dungeon checkout:

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

## 2. Choose the runtime

Run the setup wizard:

```sh
dungeon setup                     # runtime, image, review, approval
dungeon doctor                    # read-only; follows the saved runtime
```

Setup remembers the runtime, engine/socket and image for new projects. After
confirmation it can run `systemctl --user enable --now podman.socket`, pull images
and create the owned gateway. It installs nothing and never changes permissions.
For Docker CE, start its daemon and check access yourself; daemon access is
host-root-equivalent. Select it with `setup --runtime container --engine docker`.
Noninteractive setup requires `--yes`; `--dry-run` never activates services, pulls
images or writes defaults. Containers mount
only the reviewed project/imports, but **do not enforce OpenShell policy or restrict
outbound networking**. Existing bindings never switch runtime automatically.

### Optional OpenShell

Install the pinned OpenShell CLI and rootless Podman before using this path:

```sh
dungeon doctor --runtime openshell  # read-only; inspect individual checks
dungeon setup --runtime openshell --dry-run
dungeon setup --runtime openshell   # approve socket/images and fresh gateway
dungeon doctor --runtime openshell
```

`setup` does not repair or restart an existing gateway. If doctor finds the
`openshell-gateway-dungeon-local` container **stopped**, inspect its label,
mounts, network and port with Podman before explicitly starting it. Do not
delete its database, register a replacement over it, or use `podman prune`.
An existing copy-only gateway cannot be turned into a live-capable gateway by
plain `setup`. Use `setup --runtime openshell --copy-only` to verify it and
`init --project-transfer copy` for a new project there; fresh bind capability
needs an explicit disposal/reprovision decision. Doctor health is not proof of isolation.

## 3. Choose a project and agent

Both runtimes use setup's saved image, otherwise
`ghcr.io/nvidia/openshell-community/sandboxes/base:latest`. Setup pulls the current
image; first use pulls a missing image, but a cached tag is not refreshed on each
launch. Use `setup --image REGISTRY/IMAGE:VERSION` to pin future projects, or
`init --image REGISTRY/IMAGE@sha256:DIGEST` for one new project. Only a digest pins
immutable content. Setup never replaces existing guests or rewrites their bindings.

For the optional reviewed tool versions, build the
[local Linux amd64 image](../images/agent/README.md) and select it with
`dungeon init --image localhost/dungeon-agent:0.0.3-opencode-2.1.2`. The tag names
the OpenCode pin, not a required harness choice. Upstream image contents and
versions differ from this recipe; its native results do not validate `latest`.
To use the optional host OpenCode UI, separately install a host client
matching the guest pin (currently 2.1.2); use `dungeon versions` to compare them.
Ordinary `dungeon` / `dungeon run` uses the guest CLI and needs no host harness client.

```sh
cd /path/to/project
dungeon                         # choose a harness, approve project scope, launch guest CLI
# OR, on a new project:
dungeon run claude              # review binding, choose Claude as default
dungeon run codex               # choose Codex for this invocation on an existing binding
```

First use asks for OpenCode, Claude Code or Codex (no assumed choice), then scope
approval. Later launches reuse that project's choice without prompting again.
First use requires a terminal for review. `dungeon init` writes the private
config without starting anything; use it **before** first launch to choose an
image, transfer mode, gateway, or provider:

```sh
dungeon init                     # choose harness; other settings use setup defaults
dungeon plan                     # inspect policy and requested mounts
dungeon                           # first up, then the selected agent
```

The optional reviewed local image is not published, built or pulled by Dungeon.
`dungeon versions --check` reads the registry stable tag but does not download
or execute an update.
Existing bindings keep their selected image. Host configuration imports are off
by default; do not rely on host tools or host auth/config/plugins/themes in the guest.
Claude/Codex login is guest-owned (`dungeon auth claude|codex` after `up`);
OpenCode uses its own guest `/connect` flow. Changing image, policy, or scope
after creation fails identity checks rather than rebuilding in place.

The reviewed Claude CLI also supports an explicit guest-only browser login:
`dungeon run claude -- auth login --claudeai`. Open its vendor URL on your host;
if the browser gives a code, paste it into the guest terminal, not a chat or log.
No guest GUI or host authentication import is needed.

## 4. Headless use

No display, host agent CLI or terminal is required for explicit initialization,
lifecycle commands, non-TTY execution or native headless agent modes:

```sh
cd /path/to/project
dungeon init --yes --runtime container --agent codex
dungeon plan
dungeon run codex -- --version       # creates/resumes; no login needed
dungeon exec -- node -e 'console.log("hello from the guest")'
dungeon down
```

Use `--runtime openshell` instead for an already configured OpenShell gateway.
`--yes` approves the displayed configuration scope; it does not log in or bypass
the agent's permissions.
Bare `dungeon`, `attach`, `shell` and interactive login are terminal workflows;
do not use them in unattended jobs.

After authenticating **inside this guest**, use the harness's native mode:

```sh
dungeon run opencode -- run --format json "Summarize this project" > result.jsonl
dungeon run claude -- --print --output-format json -- "Summarize this project" > result.json
dungeon run codex -- exec --json "Summarize this project" > result.jsonl
```

These are alternative harness invocations, not an orchestration layer. Check
`dungeon run AGENT -- --help` for the image's installed version; the reviewed
OpenCode 2.1.2 also accepts `run --standalone`. Codex may require a trusted Git
project and its inner sandbox has a known nested-runtime limitation; do not
disable safeguards just to make a demo pass. Provider calls need guest/vendor
authentication and, on OpenShell, allowed endpoints. Dungeon does not inherit
host API keys, native authentication databases or sessions.

Keep the second `--` before the prompt when adding Claude's variadic options such
as `--tools`: `dungeon run claude -- --print --tools "" -- "Your prompt"`.
The first separator belongs to Dungeon; the later one is passed to the guest CLI.
Native Claude `--resume SESSION_ID` keeps the conversation across sandbox restarts;
Dungeon does not select or copy its history.

Runtime progress goes to stderr; stdout belongs to the workload and its nonzero
exit status is preserved. `exec` requires an already Ready guest. Ordinary
execution is not an owned cancellation guarantee; for bounded automated cleanup
and structured output use [machine execution](machine.md) on OpenShell.

For an AI validation loop, start with `dungeon exec -- <test command>`: capture
stdout/stderr and check the exit code. Use `dungeon machine --schema 1 resolve`
and `status` for JSON configuration/runtime observations. OpenShell machine
`exec --owned` adds JSONL output and confirmed-cleanup reporting; plain containers
support ordinary headless `run`/`exec`, not that streaming contract. Return files
through the live project, or explicit reviewed `export` in OpenShell copy mode.
These paths need no terminal and do not automate guest login or bypass native
agent permission prompts.

If original-source imports are blocked by SELinux before guest creation, run
`dungeon reload --harness-config copy --yes` (or `disabled`), then retry. This is
explicit scope approval, not an automatic fallback. It refuses any existing guest
binding to preserve its login/sessions; see [import recovery](configuration.md#harness-configuration-opencode-first-slice).

## 5. Daily use and deliberate disposal

For every configured harness, `dungeon` and `dungeon run` launch the same guest
CLI, creating/resuming its sandbox when needed. Bare `dungeon` requires a terminal;
explicit `dungeon run -- --help` accepts native flags without one. On an existing
binding, `dungeon run claude|codex|opencode` overrides only that invocation.
For automation, initialize explicitly as above; `--yes` never chooses a harness.

Compare harnesses with `run AGENT` in the **same binding**. Use separate project
checkouts for concurrent live environments: on SELinux, mounting the same host
directory in another sandbox can change its private label and deny the older
sandbox access. Do not disable SELinux or relabel host files to work around it.

On OpenShell, optional `dungeon attach` (not bare `dungeon`) starts an authenticated
guest OpenCode service, opens a temporary host-loopback forward and attaches your
installed host OpenCode UI. It checks
that host and guest versions match; it does not install or replace either CLI.
Dungeon pauses at `Press Enter to open OpenCode` so the startup and connection
details stay visible before the host UI takes over the terminal. Ctrl-C there
cancels attachment and cleans up the service and forward.
Exit the host UI or press Ctrl-C to stop the service and forward, keeping the
sandbox. If cleanup cannot be confirmed, choose to stop the sandbox (preserves
data; name confirmation required) or leave it running, then inspect it. A
noninteractive invocation refuses attachment. Ordinary launch uses guest CLIs;
the [manual service/forward flow](configuration.md#remote-first-attachment)
remains available. Codex and Claude's vendor-specific remote modes need their
own validation and still run as guest CLIs.

Guest conversation history survives `down` and normal attachment exit, but
Dungeon does not automatically resume the last conversation or adopt an
independently running service. Use the harness's native session selection.
After a crash/SIGKILL, inspect the sandbox before reattaching; `dungeon down`
stops it without deleting its state.

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
mount of the selected project (on a compatible bind-enabled gateway for OpenShell).
It does not mount your home or engine socket. Guest changes reach the host immediately;
upload/export do not apply. For a sandbox copy instead, explicitly choose
`dungeon init --runtime openshell --project-transfer copy`: first `up` uploads once,
resumed launches do not sync, and `export` is reviewed. Choose before creation. Old configurations
that omit the transfer field remain copy-mode.

`destroy` loses guest files, sessions and auth, but retains private configuration
for a fresh `up`. Export first if using copy mode. It is not a gateway or orphan
cleanup command. Installing a new CLI does not migrate an existing binding or
gateway. See [configuration](configuration.md) for detailed limits and recovery.
