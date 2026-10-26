# Dungeon

Run coding agents and development tools in isolated project environments, using
their normal CLIs. Dungeon manages setup, private configuration, lifecycle and
reviewed project transfers. [OpenShell](https://github.com/NVIDIA/OpenShell) owns
isolation and policy enforcement; your tools own prompts, models and sessions.
Part of [Agent Archaeology](https://agentarchaeology.ai/).

**Development preview:** this README describes the source-only Rust 0.0.2
implementation on main. Published v0.0.1 is the older Python implementation.
Neither is an audited isolation product.

## Quick start

For the short installation/adoption sequence, including an existing stopped
gateway and copy/live choices, use the [adoption cheat sheet](docs/adoption.md).

### 1. Build the CLI

For the supported Linux path, install Git, a Rust toolchain manager providing
`cargo`, OpenShell **0.0.116**, and rootless Podman separately. The repository pins
**Rust 1.98.1** in `rust-toolchain.toml`. Build the current preview from source:

```sh
git clone https://github.com/Dark-Roast-Cyber/dungeon.git
cd dungeon
cargo build --release --locked
export PATH="$PWD/target/release:$PATH"
dungeon --help
```

The PATH export applies to this shell. Native Rust release archives and the npm
launcher are **not published yet**. See [distribution](docs/distribution.md).
For a user-local install from this checkout, run
`./scripts/install-local.sh --dry-run` and then `./scripts/install-local.sh`;
an existing different `~/.local/bin/dungeon` requires explicit `--replace`.
The helper installs only the Rust binary, not OpenShell or a gateway.

### 2. Set up the local gateway

Start the user Podman API socket and pull the pinned gateway image explicitly:

```sh
systemctl --user enable --now podman.socket
podman pull ghcr.io/nvidia/openshell/gateway@sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645
dungeon setup               # fresh bind-capable gateway; no project mounted yet
dungeon doctor
```

Setup creates a local mTLS gateway, `dungeon-local`, on loopback port 17670 using
rootless Podman and workspace `default`. Repeating it verifies compatible running
state; it never overwrites conflicts, repairs stopped gateways, installs
prerequisites or falls back to weaker isolation. `doctor` is read-only and can
also be used before setup to identify missing prerequisites.

### 3. Review a project, then start it

Tools must be installed in the selected sandbox image; Dungeon does not use your
host's tools. New CLI bindings select the versioned local image candidate with
reviewed OpenCode, Claude Code and Codex CLIs. **Build it explicitly before `up`** using the
[image procedure](images/agent/README.md); setup does not download or build it.
`dungeon versions --check` reads the upstream stable tag and reports if a newer
release needs review. Existing bindings and their image choices are unchanged.

```sh
podman pull ghcr.io/nvidia/openshell-community/sandboxes/base@sha256:aeef1c63f00e2913ea002ccb3aaf925f338b5c5d70e63576f0d95c16a138044e
podman build --pull=never -t localhost/dungeon-agent:0.0.3-opencode-2.1.2 \
  -f images/agent/Containerfile images/agent
```

The reviewed image is a local Linux amd64 candidate, not a published or
cross-platform default. See the image procedure for offline verification.
Ordinary launch needs no host harness client. Optional `dungeon attach` requires
a separately installed matching **OpenCode 2.1.2** host client.

```sh
cd my-project
dungeon init                    # choose OpenCode, Claude Code or Codex; approve scope
dungeon plan
dungeon                         # create/resume, then launch your chosen guest CLI
# dungeon up; dungeon shell     # Bash instead
```

`init` asks only for a harness choice and scope approval. It uses the current
project, a generated name, private config storage and one live read/write project
mount; it starts nothing. Host configuration imports are off by default.
The choice is saved per project, not asked again on launch. `plan` reviews policy
without runtime contact. You can also run `dungeon` directly for this first-use review.
First `up` creates the sandbox with the selected host project at `/sandbox/project`;
guest writes appear on the host immediately. Choose
`dungeon init --project-transfer copy` for a sandbox copy and reviewed export.
**Review project contents and policy before `up`; transfer is not a secret scanner.**

## Everyday use

From the project directory, choose the interface you need:

`dungeon --help` groups everyday commands, setup/configuration, and advanced or
recovery commands. Use `dungeon COMMAND --help` for the options of any command.

| Task | Command |
| --- | --- |
| Launch the configured agent | `dungeon` |
| Launch it with native flags | `dungeon run -- --help` |
| Attach the host OpenCode UI explicitly | `dungeon attach` |
| Choose an agent | `dungeon run opencode`, `dungeon run claude`, or `dungeon run codex` |
| Compare host/reviewed guest versions and check upstream stable | `dungeon versions --check` |
| Run a tool or tests | `dungeon exec -- cargo test` (requires Cargo in the image) |
| Run an interactive tool | `dungeon exec --tty -- nvim file.txt` (requires Neovim in the image) |
| Open a terminal | `dungeon shell` |
| Inspect sandbox state | `dungeon status` |
| Review and return guest edits (copy mode only) | `dungeon export` |
| Stop, preserving guest state | `dungeon down` |
| See bindings in/under this directory | `dungeon list` |
| See all private project bindings | `dungeon list -a` |
| Delete a finished sandbox | `dungeon down`, then `dungeon destroy` |

No repeated config path or harness choice is needed. Interactive `dungeon` and
`dungeon run` launch the same configured guest CLI for all three harnesses,
creating/resuming the sandbox as needed. Bare `dungeon` requires a terminal;
use explicit `run` with native flags for noninteractive work.
`dungeon ../other-project` selects another project. Agents may require
[guest authentication](#authentication) before use. Harnesses own conversation
selection; guest session history remains in the retained sandbox.

**Changed from the earlier preview:** bare OpenCode launch no longer attaches a
host UI. Use explicit `dungeon attach` for the matched host/guest service path;
see [optional attachment](#optional-attachment).

On a new project, `dungeon run claude` (or `codex`) also requests the one-time
binding review, records that agent as the default, then creates and launches the
environment. An existing binding keeps its configured default; `run` selects an
agent for this invocation. A first `run` against an initialized but not-yet-up
environment creates its configured live mount, or uploads once in copy mode.
It does not add authority beyond the reviewed binding.

- `up` resumes the same sandbox without another upload; already Ready is a no-op.
- `run` can start a stopped bound sandbox, also without uploading. `attach`
  creates/resumes before connecting. `exec`, `shell`, `code`, `ssh-config`,
  `forward` and `ssh --owned` require it already Ready.
- `down` preserves guest files, sessions and authentication.

`list` shows bindings for the current directory and its descendants; `list -a`
shows all private bindings. It is offline JSON, not a query of OpenShell or
Podman or a list of agent conversations. A `bound` row contains the recorded
sandbox ID, not its current phase; `configured` means no recorded sandbox. It cannot find sandbox
orphans, external gateways, or explicit `--directory` configs outside the
project-binding root. Use `status` for one binding's live state.

When finished with a bound environment, run `down` and then `destroy`.
`destroy` rechecks the stopped sandbox's identity, asks you to type its name
(or accepts explicit `--yes`), and requests OpenShell deletion. **Guest files,
sessions and authentication are lost.** Private `dungeon.json`, policy and
approved imports remain; the next `up` makes a fresh sandbox using the same
transfer mode. Live-project host files are not deleted; copy-mode guest edits
must be exported before destruction. This is not a container prune or a
gateway teardown; if deletion or local cleanup fails, inspect before retrying.

### Uninstall (Linux user-local installation)

Close Dungeon clients and export any copy-mode work you need to keep, then run
the **installed binary**:

```sh
~/.local/bin/dungeon uninstall --dry-run
~/.local/bin/dungeon uninstall          # type `uninstall dungeon` to confirm
# ~/.local/bin/dungeon uninstall --yes  # destructive test-environment automation
```

This stops and destroys recorded local sandboxes, verifies their containers are
gone, removes the recognized `dungeon-local` gateway/container/network and its
OpenShell registration, then deletes private Dungeon config/state and the
user-local binary. **Guest files, sessions, authentication and saved configuration
are permanently lost.** Host projects, original imports, other gateways/containers,
cached images, OpenShell and Podman remain. This is file removal, not secure erasure.

Use `--bin-dir /absolute/install/directory` for a custom local install. A different
binary or symlink at that destination is refused. Unknown/unbound sandboxes,
foreign bindings, unsafe paths and incompatible or unavailable prerequisites
block removal rather than trigger a prune. A stopped gateway must be explicitly
started first; uninstall never starts it. Custom `init --directory` configurations
outside the private binding root must be destroyed separately and are not erased.
See [uninstall scope and test gate](docs/distribution.md#clean-local-uninstall).

### Returning changes in copy mode

Guest edits stay in the sandbox until `dungeon export`. Stop active writers in
the guest and on the host, review the staged summary, then type `yes` to apply.
Conflicts and special entries are refused; `--yes` explicitly skips the prompt.

Host edits are not automatically synchronized either. `dungeon upload --yes`
refreshes the guest project and **may discard guest edits**. It is a deliberate
overwrite operation, not ordinary startup.

Failed first upload blocks new work. Inspect with `status`, preserve guest edits,
use `start` if stopped, then recover deliberately with `upload --yes`.
`up` does not retry a failed initial upload.

## Configure your environment

Select options for a new binding (instead of plain `init` above), without a
separate wizard:

```sh
dungeon init --agent codex
dungeon config                  # resolved values and config/policy paths
dungeon config --path           # just the dungeon.json path
```

| Setting | Default | Initialization flag |
| --- | --- | --- |
| Project | Current directory | `--project PATH` |
| Config directory | Private project binding | `--directory PATH` |
| Environment name | Generated from the project path | `--name NAME` |
| Harness | Ask once per project; no assumed choice | `--agent opencode\|claude\|codex` |
| Image | Reviewed local `dungeon-agent:0.0.3-opencode-2.1.2` (build first) | `--image OCI_REFERENCE` |
| Transfer | Live, one reviewed writable project mount | `--project-transfer copy\|live` |
| Gateway | `dungeon-local` | `--gateway NAME` |
| Workspace | `default` | `--workspace NAME` |
| Providers | None | `--provider NAME` (repeatable; existing providers, not secrets) |
| OpenCode host configuration | Disabled; explicitly opt in | `--harness-config read-only\|copy\|disabled` |

For noninteractive configuration writing:

```sh
dungeon init --yes --project /path/to/my-project --agent codex
```

`--yes` approves saving configuration only; it does not choose a harness, start a
sandbox or upload files. Noninteractive init requires `--agent`. Existing
directories are never overwritten. Full configuration rules are
in [configuration](docs/configuration.md#initialize-and-customize).

Configuration lives under
`${XDG_CONFIG_HOME:-$HOME/.config}/dungeon/projects/<project-key>`, never in the
repository. Edit adjacent `policy.json` for network/filesystem policy and
`dungeon.json` for environment choices **before creating the sandbox**. Changes
to a bound configuration fail identity checks; Dungeon does not silently rebuild
or widen policy. Use `--directory PATH` for a separately managed environment and
`dungeon --config PATH COMMAND` to select it explicitly.

Live transfer exposes only the selected host project read-write and requires
a recognized bind-enabled gateway. Fresh `setup` creates that capability by
default, but never mounts a project by itself. `setup --copy-only` and
`init --project-transfer copy` keep the isolated copy/review workflow. Existing
copy gateways cannot be migrated automatically; existing bindings keep their
recorded mode. See [setup modes and limits](docs/configuration.md#local-setup-modes).

## Authentication

```sh
dungeon auth codex
dungeon auth claude
```

Run `up` first. Codex uses device login; Claude opens its guest CLI for `/login`.
Guest login credentials stay guest/vendor-owned. OpenShell named providers are
the separate brokered API path. Native host session/authentication databases are
never shared.

New bindings do not import host harness configuration. Explicit OpenCode
`init --harness-config read-only` proposes selected host configuration,
plugins/themes and legacy `auth.json` during scope review. **Read-only
does not prevent credential use or exfiltration.** Current OpenCode 2.1.2 database
credentials are not imported; guest login may still be required. **This first
slice is not ready for normal use:** the native gate found original-source
access and guest-directory blockers in the old reviewed image. The recipe now
precreates guest-owned directories; new work with original-source imports refuses
enforcing SELinux before runtime contact. Copy remains an explicit choice, and
provider-auth reuse is still unestablished. See the
[native result](docs/validation.md#authorized-native-gate-2026-10-22-blocked).
Existing bindings and Codex/Claude Code configuration sharing are unchanged.

```sh
dungeon init --agent opencode --harness-config read-only --exclude-provider-auth
dungeon init --agent opencode --harness-config read-only --harness-exclude plugins/private
dungeon init --agent opencode --harness-config copy  # then dungeon reload to capture/review
```

**Read-only auth may prevent login refresh, and snapshots may become stale.**
`reload` currently refreshes unbound selections only, not active sandboxes or
guest tokens. See [harness configuration](docs/configuration.md#harness-configuration-opencode-first-slice).

## Optional attachment

`dungeon attach` explicitly starts an authenticated guest OpenCode service,
forwards it to host loopback and opens a matching host OpenCode UI. It pauses at
`Press Enter to open OpenCode` before the UI takes over. Normal exit/Ctrl-C cleans
up the service and forward, keeping the sandbox; unconfirmed cleanup offers a
confirmed sandbox stop or leaves it running for inspection. Attachment requires
a terminal and does not select the last conversation or adopt detached services.
A crash/SIGKILL may leave a service running; inspect and use `dungeon down`.

```sh
dungeon code
dungeon ssh-config
dungeon forward --port 4096 --local-port 14096
dungeon ssh --owned -- nvim file.txt
```

These require a Ready binding. Forwarding is foreground and host-loopback only;
it neither starts nor authenticates the guest service. Interrupting forwarding
stops the listener, not the service. Explicit `exec --owned` and `ssh --owned`
assist bounded command cleanup on Linux, not stronger containment; guest Node
and `/proc` are required. See [attachment](docs/configuration.md#remote-first-attachment),
[owned execution](docs/configuration.md#owned-foreground-execution) and
[reviewed Neovim snapshots](docs/configuration.md#reviewed-neovim-snapshots).

## Automation and limits

- Harnesses use [`dungeon machine --schema 1 ...`](docs/machine.md) for lifecycle
  JSON, workload-byte JSONL and explicit whole-project transfer. No daemon or
  agent orchestration is added. Sandcastle integration is not implemented.
- Runtime evidence covers specific Linux/OpenShell **0.0.116**/rootless-Podman
  combinations, not general desktop, EDR, macOS or native Windows containment.
  Setup health and tests are not security attestation.
- No host home, engine socket or native auth/session database is shared.
  Selected OpenCode configuration/legacy auth requires binding scope approval;
  project files cannot authorize mounts or policy changes.
- Telltale integration remains deferred until its 0.7.0/Event4 contract stabilizes.
  It is not part of the active runtime.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test npm/launcher/test/platform.test.mjs
```

CI uses explicit GA runner images rather than mutable `*-latest` labels and pins
third-party actions to immutable commits corresponding to stable releases.

Read [architecture](docs/architecture.md), [configuration](docs/configuration.md), [security](SECURITY.md), [validation](docs/validation.md), [live validation runbook](docs/live-validation.md), and [distribution](docs/distribution.md).

Apache-2.0. Third-party runtimes, agents, images, and desktop products retain their own licenses and service terms.
