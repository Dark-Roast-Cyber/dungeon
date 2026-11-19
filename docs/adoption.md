# Dungeon adoption cheat sheet (source preview)

Dungeon 0.0.2 has **no public binary release**. Do not use the published v0.0.1
Python package or unpublished npm commands for this Rust workflow. The supported
local path is
Linux with rootless Podman or standard Docker CE, and Rust **1.98.1** for building.
OpenShell **0.0.116** is optional for policy enforcement, copy/export, SSH and
forwarding. This is an engineering preview, not containment certification.

## Team pilot

**Start with a few named users on managed Linux x86-64 with rootless Podman.**
Use a disposable account or checkout with no customer data, credentials, or
production access. Plain containers give the guest write access to that
checkout and normal outbound networking. Do not use this pilot when enforced
egress restrictions or audited isolation are required.

Have one pilot owner supply a [verified prebuilt binary](distribution.md#internal-linux-pilot),
an approved `registry/image@sha256:...` reference, and a private support channel.
Use one checkout per participant; don't share a project directory between live
containers on SELinux. Do not disable SELinux, endpoint protection, or an agent's
own sandbox to get a pass.

### No-login smoke check

Run this in a disposable checkout after installing the binary. Set
`PILOT_IMAGE` to the approved digest reference from the pilot owner. The setup
command may start the user Podman socket, pull the image, and save defaults for
**new** projects. Review it before approving; it does not change existing
bindings. Use a disposable user if changing your defaults is not acceptable.

```sh
dungeon setup --runtime container --engine podman --image "$PILOT_IMAGE"
dungeon doctor --runtime container
dungeon init --agent opencode --image "$PILOT_IMAGE" --harness-config disabled
dungeon run opencode -- --version
dungeon exec -- sh -c 'printf "pilot\n" > "$HOME/dungeon-pilot-marker"'
dungeon down
dungeon run opencode -- --version
dungeon exec -- cat /sandbox/dungeon-pilot-marker
dungeon down
```

Pass means both launches report the same agent version, the final `cat` prints
`pilot`, and both stops succeed. Day to day use `dungeon` (or `dungeon -a claude`
or `dungeon -a codex` with an image that ships the agent). The smoke check does
not test provider sign-in. Test vendor login only inside a guest, with pilot
accounts and projects approved for that provider.

| Symptom | Next step |
| --- | --- |
| `dungeon --version` is wrong | Check `command -v dungeon` and PATH. Do not use v0.0.1 or unpublished npm packages. |
| Podman or socket unavailable | Run `dungeon doctor --runtime container` and inspect `systemctl --user status podman.socket`. Ask your Linux owner rather than changing permissions or using sudo. |
| Image pull fails or a guest tool is missing | Confirm the approved registry/digest and network access. Host-installed tools do not appear in the guest. A changed default does not update an existing binding. |
| OpenShell 0.1.x is already installed | Leave it installed. Use the plain-container pilot only if its boundary is acceptable; Dungeon's 0.1.x adapter is not enabled. |
| Fresh or edited guest commands die unexpectedly | Stop and collect the exact command and exit status for your endpoint-security owner. Do not disable protections or infer the cause from an exit code alone. |

For support, record the source revision, engine/OS versions, image digest,
failing step, and sanitized error. Review `doctor` output before sharing it;
omit private paths, credentials, configuration files, and session data. Follow
the [security reporting route](../SECURITY.md#reporting) for suspected vulnerabilities.
Use `dungeon down` to stop without deleting state. If you want to discard only
the pilot guest, run `dungeon destroy` after stopping it and review the
confirmation. Do not use global uninstall or an engine prune as pilot cleanup.

**Before expanding:** repeat install and smoke checks on clean machines
representative of the team's fleet; exercise one real provider workflow with
approved data; agree on ownership for image updates, egress, and endpoint
protection; and rehearse recovery and disposal. If the team requires OpenShell,
qualify Dungeon's production adapter before offering that path. The
[native 0.1.7 result](validation.md#reviewed-image-on-native-openshell-017-2026-11-18)
does not close that gate.

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

Run `dungeon` in a new project. If no defaults are saved, it asks you to approve
plain-container setup, then asks separately for project access. To choose
OpenShell, a different engine, or a different image, run setup first:

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
images or writes defaults. Setup is not a reset: changing defaults never replaces
a guest or rewrites a project binding. Containers mount
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
`dungeon init --image localhost/dungeon-agent:0.0.3-opencode-2.2.7`. The tag names
the OpenCode pin, not a required harness choice. Upstream image contents and
versions differ from this recipe; its native results do not validate `latest`.
To use the optional host OpenCode UI, separately install a host client
matching the guest pin; use `dungeon versions` to compare them.
Ordinary `dungeon` / `dungeon run` uses the guest CLI and needs no host harness client.

```sh
cd /path/to/project
dungeon                         # saved agent; OpenCode for a new project
dungeon -a claude               # use Claude; saves the choice only on a new project
dungeon -a codex                # use Codex this time in the existing project
```

First project review shows the agent, image and project access. Direct launch
does not enable host configuration imports on new projects. To opt in, use
`dungeon init --agent opencode --harness-config copy` before first launch.
Copies can expose credentials; native auth/session databases are never imported. If you cancel
project review after approving setup, the defaults stay saved and no project is
bound. Later launches reuse that project's choices even if setup defaults are
absent.
First use requires a terminal for review. `dungeon init` writes the private
config without starting anything; use it **before** first launch to choose an
image, transfer mode, gateway, or provider:

```sh
dungeon init                     # guided configuration; no guest started
dungeon config --edit            # optional guided changes before first guest creation
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
For optional headless OpenCode imports, add `--harness-config copy` to `init`;
the snapshot is captured/approved in that operation. Common unbound settings can
also be changed with `dungeon config --edit --yes --image OCI_REFERENCE` or
`--harness-config copy`. No implicit imports or runtime switching occurs.
`--yes` approves the displayed configuration scope; it does not log in or bypass
the agent's permissions.
Direct `dungeon` (including `-a`), `attach`, `shell` and interactive login are
terminal workflows; do not use them in unattended jobs.

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
and `status` for JSON configuration/runtime observations. Machine `init --runtime
container --yes` also initializes plain environments without OpenShell; see its
[explicit directory/image options](machine.md#selection-and-operations). OpenShell machine
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
