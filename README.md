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
reviewed OpenCode 2.1.2. **Build it explicitly before `up`** using the
[image procedure](images/agent/README.md); setup does not download or build it.
`dungeon versions --check` reads the upstream stable tag and reports if a newer
release needs review. Existing bindings and their image choices are unchanged.

```sh
podman build --pull=never -t localhost/dungeon-agent:0.0.3-opencode-2.1.2 \
  -f images/agent/Containerfile images/agent
```

Pull the image's exact base digest first if it is not already cached; see the
image procedure for the digest and offline verification.

```sh
cd my-project
dungeon init
dungeon plan
dungeon up
dungeon shell
```

`init` uses the current project, a generated name, private config storage and
one live read/write project mount. It shows the exact scope and asks once before writing configuration;
it starts nothing. `plan` lets you review the policy without runtime contact.
First `up` creates the sandbox with the selected host project at `/sandbox/project`;
`shell` opens Bash there. Guest writes appear on the host immediately. Choose
`dungeon init --project-transfer copy` for a sandbox copy and reviewed export.
**Review project contents and policy before `up`; transfer is not a secret scanner.**

## Everyday use

From the project directory, choose the interface you need:

| Task | Command |
| --- | --- |
| Launch the configured agent | `dungeon` |
| Choose an agent | `dungeon run opencode`, `dungeon run claude`, or `dungeon run codex` |
| Run a tool or tests | `dungeon exec -- cargo test` (requires Cargo in the image) |
| Run an interactive tool | `dungeon exec --tty -- nvim file.txt` (requires Neovim in the image) |
| Open a terminal | `dungeon shell` |
| Inspect sandbox state | `dungeon status` |
| Review and return guest edits | `dungeon export` |
| Stop, preserving guest state | `dungeon down` |
| See bindings in/under this directory | `dungeon list` |
| See all private project bindings | `dungeon list -a` |
| Delete a finished sandbox | `dungeon down`, then `dungeon destroy` |

No repeated config path is needed. In a terminal, `dungeon` initializes/resumes
and attaches the **host OpenCode UI** to an authenticated guest OpenCode server
through a temporary loopback forward. Host and guest versions must match. On
normal exit or Ctrl-C, Dungeon stops the service and forward and keeps the
sandbox. If it cannot confirm cleanup, a plain text prompt offers to stop the
sandbox (preserving data) or leave it running. `dungeon run opencode` still runs
the guest TUI; Claude and Codex run their guest CLIs. `dungeon ../other-project`
selects another project. Agents may
require [guest authentication](#authentication) before use.

On a new project, `dungeon run claude` (or `codex`) also requests the one-time
binding review, records that agent as the default, then creates and launches the
environment. An existing binding keeps its configured default; `run` selects an
agent for this invocation. A first `run` against an initialized but not-yet-up
environment performs the initial copy. Neither path grants new mounts or policy.

- `up` resumes the same sandbox without another upload; already Ready is a no-op.
- `run` can start a stopped bound sandbox, also without uploading. `exec`,
  `shell` and attachment commands require it already Ready.
- `down` preserves guest files, sessions and authentication.

`list` is an offline JSON inventory of private project bindings, not a query of
OpenShell or Podman. A `bound` row contains the recorded sandbox ID, not its
current phase; `configured` means no recorded sandbox. It cannot find sandbox
orphans, external gateways, or explicit `--directory` configs outside the
project-binding root. Use `status` for one binding's live state.

When finished with a bound environment, run `down` and then `destroy`.
`destroy` rechecks the stopped sandbox's identity, asks you to type its name
(or accepts explicit `--yes`), and requests OpenShell deletion. **Guest files,
sessions and authentication are lost.** Private `dungeon.json`, policy and
approved imports remain; the next `up` makes a fresh sandbox and initial copy.
Review/export copy-mode guest edits first. This is not a container prune or a
gateway teardown; if deletion or local cleanup fails, inspect before retrying.

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
| Agent | OpenCode | `--agent opencode\|claude\|codex` |
| Image | Reviewed local `dungeon-agent:0.0.3-opencode-2.1.2` (build first) | `--image OCI_REFERENCE` |
| Transfer | Live, one reviewed writable project mount | `--project-transfer copy\|live` |
| Gateway | `dungeon-local` | `--gateway NAME` |
| Workspace | `default` | `--workspace NAME` |
| Providers | None | `--provider NAME` (repeatable; existing providers, not secrets) |

For noninteractive configuration writing:

```sh
dungeon init --yes --project /path/to/my-project --agent codex
```

`--yes` approves saving configuration only; it does not start a sandbox or upload
files. Existing directories are never overwritten. Full configuration rules are
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
Credentials stay guest/vendor-owned. Dungeon never copies host credentials or
native sessions. OpenShell named providers are the separate brokered API path;
OpenCode login remains provider-specific.

## Optional attachment

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
- No host home, engine socket, credentials or dotfiles are shared automatically.
  Project files cannot authorize mounts or policy changes.
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
