# Dungeon

Run OpenCode, Claude Code, Codex and development tools in isolated project
environments, using their normal CLIs. Dungeon manages the environment; your
agent still owns prompts, models, permissions and conversations.
Part of [Agent Archaeology](https://agentarchaeology.ai/).

**Development preview:** Rust 0.0.2 is source-only. Published v0.0.1 is the older
Python implementation; Rust release archives and npm packages are not published
yet. Dungeon is not an audited isolation product.

## Overview

- **One environment per project:** review scope once, then run `dungeon` to
  create or resume the same guest and launch your chosen agent.
- **Your choice of runtime:** local rootless Podman or Docker CE for ordinary
  containers; optional [OpenShell](https://github.com/NVIDIA/OpenShell) for
  filesystem/network policy and gateway features.
- **Live files or reviewed copies:** mount the selected project read/write, or
  use OpenShell copy mode with explicit export back to the host.
- **Persistent guest state:** files, agent sessions and guest login survive
  stop/resume. Host authentication and session databases are not shared.
- **Terminal, headless or remote clients:** use guest CLIs, execute commands,
  attach a host OpenCode UI, or use supported SSH/editor connections.
- **Automation without another daemon:** native exit codes and workload output,
  plus a versioned JSON/JSONL machine interface for external harnesses.

| Runtime | What it provides | Important boundary |
| --- | --- | --- |
| Plain Podman / Docker CE | Agent CLIs, commands, live project files and lifecycle | Normal container networking; **no OpenShell policy enforcement**. Docker daemon access is host-root-equivalent. |
| OpenShell + rootless Podman | The above, plus policy, copy/export, named providers, host UI attachment, SSH, forwarding and owned execution | Requires the pinned OpenShell setup; validated combinations are specific Linux environments, not general containment certification. |

Dungeon does not orchestrate agents, select models, synchronize credentials or
provision remote machines. Tools must be installed **in the image**, not just on
your host. See [architecture](docs/architecture.md) and [security](SECURITY.md).

## Installation

### 1. Install the CLI

The supported runtime path is Linux. Install Git, Rust/rustup with Cargo, and
either rootless Podman or standard local Docker CE. The checkout pins
**Rust 1.98.1**; the first build may download that toolchain.

```sh
git clone https://github.com/Dark-Roast-Cyber/dungeon.git
cd dungeon
./scripts/install-local.sh --dry-run
./scripts/install-local.sh
export PATH="$HOME/.local/bin:$PATH"
dungeon --help
```

The helper builds the locked Rust binary and installs it under `~/.local/bin`.
It does not install an engine, OpenShell or an image. A different existing binary
is refused; review it before explicitly using `--replace`. The PATH change above
applies to this shell. For a trial without installation, use
`cargo build --release --locked` and `export PATH="$PWD/target/release:$PATH"`.
See [distribution and uninstall](docs/distribution.md).

### 2. Run interactive setup

```sh
dungeon setup
dungeon doctor
```

Choose **plain containers** (the simplest start) or **OpenShell** (policy and
remote-client features), then choose an image and approve the changes. Setup
discovers the service, offers user Podman socket activation when needed, pulls
images and saves defaults for new projects. Podman is preferred over Docker CE;
Docker needs an already running daemon and appropriate access.

OpenShell requires its **0.0.116** CLI and rootless Podman already installed.
Setup pulls the compatible pinned gateway image and creates `dungeon-local`
with mTLS on loopback port 17670. It never installs packages, uses sudo, changes
permissions, overwrites conflicting state or restarts an existing gateway.
`doctor` is read-only; `setup --dry-run` previews without changes and reports
missing prerequisites. See the [adoption guide](docs/adoption.md#2-choose-the-runtime).

For unattended setup, select explicitly and approve with `--yes`:

```sh
dungeon setup --yes --runtime container
dungeon setup --yes --runtime container --engine docker
dungeon setup --yes --runtime openshell
```

### 3. Latest by default; pin when needed

Both runtimes and all three harnesses default to the public image
`ghcr.io/nvidia/openshell-community/sandboxes/base:latest`. Setup refreshes that
image; first use pulls it if missing. Installed tool versions come from upstream,
not Dungeon's local recipe. Cached images are not refreshed on every launch.
Existing guests are never upgraded or replaced by setup.

Pin a version tag or, for immutable content, a digest:

```sh
dungeon setup --runtime container --image ghcr.io/example/agent:1.2.3
# Or override only a new project:
dungeon init --image ghcr.io/example/agent@sha256:YOUR_REVIEWED_DIGEST
```

The optional [reviewed local image](images/agent/README.md) includes pinned
agents, patched Node, Neovim and Rust. Build it explicitly and select it with
`--image localhost/dungeon-agent:0.0.3-opencode-2.1.2`; it is not published or
automatically built. Its validation does not attest the mutable upstream image.

## Using Dungeon

### Review once, launch normally

From your project directory:

```sh
cd /path/to/project
dungeon               # first run: choose agent, review image/access, approve and launch
dungeon exec -- node --version
dungeon down           # keep guest login, sessions and files
```

First run offers an optional reviewed copy of OpenCode configuration, **off by
default**. One scope approval saves that exact snapshot and launches; no separate
`init`, `reload` or `up` is required. Copies can expose credentials and execute
plugins; native login/session databases are never shared. Later launches reuse
the saved choices. No host agent installation is needed for guest CLI use.

Use `dungeon init` to review/save without starting, `dungeon plan` to inspect, or
`dungeon config --edit` to change an uncreated environment. The editor has guided
choices and explicit flags for automation; bare `dungeon config` stays read-only.

Commands run tools supplied by the selected image. The cached upstream base used
in validation has Node but no Cargo, Rust compiler or pnpm. For Rust tests, select
the [reviewed local image](images/agent/README.md) or your own toolchain image
before creating the guest; Dungeon does not install missing tools automatically.

Runtime selection uses saved setup defaults, otherwise plain containers, even if
OpenShell is installed. Use `init --runtime container` or
`init --runtime openshell` to be explicit. Existing bindings never switch runtime
or service when a dependency disappears.

**Live files are the default:** the selected project is writable at
`/sandbox/project`, and guest edits reach the host immediately. Review its contents
before launch; Dungeon is not a secret scanner. Use one binding to compare
harnesses; concurrent live environments should use separate checkouts to avoid
SELinux private-label conflicts.

### OpenCode and Claude Code

Switch agents within the same environment without changing the saved default:

```sh
dungeon run opencode
dungeon run claude
dungeon run codex
```

Authenticate **inside the guest**. OpenCode uses its `/connect` flow. The reviewed
Claude CLI supports an explicit browser login, followed by ordinary launch:

```sh
dungeon run claude -- auth login --claudeai
dungeon run claude
```

Open the vendor URL on your host; paste any login code into the guest terminal,
not a chat or log. Alternatively, after `dungeon up`, `dungeon auth claude` opens
the guest CLI for `/login`; `dungeon auth codex` starts device login. Login is
per sandbox, survives `down`, and is lost on destruction. OpenShell named providers
are a separate authentication option. See [authentication](docs/integrations.md#authentication).

### Headless use

For a new unattended binding, initialize explicitly with
`dungeon init --yes --runtime container --agent claude` instead of interactive
`init`. After guest authentication, pass native agent flags:

```sh
dungeon run opencode -- run --format json "Summarize this project" > result.jsonl
dungeon run claude -- --print --output-format json -- "Summarize this project" > result.json
dungeon run codex -- exec --json "Summarize this project" > result.jsonl
```

Bare `dungeon` requires a terminal; explicit `run` supports headless execution.
Runtime progress goes to stderr, workload output stays on stdout, and nonzero
exit codes propagate. The first `--` belongs to Dungeon; subsequent separators
are passed to the agent. Agents own session selection, including native resume
flags. See [headless examples](docs/adoption.md#4-headless-use).

### Commands to remember

| Task | Command |
| --- | --- |
| Launch the saved agent / override it once | `dungeon` / `dungeon run AGENT` |
| Create or resume without launching an agent | `dungeon up` |
| Run an image-provided tool | `dungeon exec -- node --version` (or `cargo test` with a Rust image) |
| Open Bash / an interactive tool | `dungeon shell` / `dungeon exec --tty -- nvim file.txt` |
| Inspect the current environment | `dungeon status` |
| List project bindings / all private bindings | `dungeon list` / `dungeon list -a` |
| Inspect configuration / its path | `dungeon config` / `dungeon config --path` |
| Review changes before first guest creation | `dungeon config --edit` |
| Compare reviewed/host OpenCode versions | `dungeon versions --check` |
| Stop, keeping guest files, login and sessions | `dungeon down` |
| Delete a stopped guest, keeping its configuration | `dungeon destroy` |

`run` creates/resumes as needed; `exec` and `shell` require an already Ready guest.
`destroy` requires a stopped guest and confirmation: **guest files, login and
sessions are lost**. Export copy-mode edits first. `list` is offline binding
inventory, not live runtime state or agent conversation history.

For copy mode, choose `init --runtime openshell --project-transfer copy` before
creation. First `up` uploads once; later launches do not sync. `dungeon export`
reviews guest changes before applying them to the host. Explicit
`dungeon upload --yes` refreshes the guest and **may overwrite guest edits**.
See [transfer and recovery](docs/configuration.md).

## Configuration options

Set these at `init`, before creating the sandbox. Use `dungeon COMMAND --help`
for complete flags. Common choices can also be changed with `config --edit`;
runtime/service/project identity changes still require separate initialization.

| Option | Purpose |
| --- | --- |
| `--agent opencode\|claude\|codex` | Saved agent choice; otherwise ask once |
| `--runtime container\|openshell\|auto` | Explicit runtime or normal discovery/defaults |
| `--engine podman\|docker\|auto`, `--endpoint unix:///absolute/socket` | Plain-container service selection; an endpoint needs an explicit engine |
| `--image OCI_REFERENCE` | Override setup's image; accepts a tag or immutable digest |
| `--project-transfer live\|copy` | Immediate host edits or OpenShell copy/review/export |
| `--project PATH`, `--name NAME` | Override the current project and generated environment name |
| `--gateway NAME`, `--workspace NAME` | Select an existing OpenShell gateway/workspace |
| `--provider NAME` | Attach an existing named OpenShell provider; repeatable, never a secret |
| `--harness-config disabled\|copy\|read-only` | Opt-in OpenCode imports; copy is reviewed/saved in the same operation; original-source read-only is advanced |
| `--directory PATH` | Custom private configuration directory; use `dungeon --config PATH/dungeon.json COMMAND` afterward |

Normal configuration lives under
`${XDG_CONFIG_HOME:-$HOME/.config}/dungeon/projects/<project-key>`, not in the
repository. Review `dungeon.json` and adjacent `policy.json` before creation;
bound scope/policy changes fail identity checks rather than silently rebuilding.
Read-only imports can still expose secrets; native authentication/session
databases are never imported. See [configuration](docs/configuration.md).

On enforcing SELinux, launching with original-source imports offers to review a
private copy or disable imports before first creation. For headless recovery use
`dungeon reload --harness-config copy --yes` (or `disabled`), then repeat `run`.
Existing guests are never reconfigured; their login and sessions are preserved.

## Remotes and host clients

These connection features require an **OpenShell environment**. They let a host
client work with guest tools/state; they do not make the host UI itself isolated.

### Host OpenCode UI

```sh
dungeon versions
dungeon attach
```

`attach` creates/resumes the guest, requires a matching installed host/guest
OpenCode version, starts an authenticated guest service and opens a temporary
loopback forward. Press Enter at the connection prompt to open the host UI.
Normal exit/Ctrl-C cleans up the service and forward, keeping the sandbox.
Bare `dungeon` remains the guest CLI, not host attachment. After a crash, inspect
the sandbox and use `dungeon down` if cleanup is uncertain.

### SSH, editors and port forwarding

Run `dungeon up` first:

| Connection | Command | Scope |
| --- | --- | --- |
| Native SSH configuration | `dungeon ssh-config` | Print configuration for deliberate client setup |
| Guest terminal editor with owned cleanup | `dungeon ssh --owned -- nvim file.txt` | Linux foreground SSH; requires Neovim in the image |
| Host VS Code | `dungeon code` | Requires VS Code/Remote SSH; may update host SSH configuration; GUI workflow needs separate validation |
| Guest service on host loopback | `dungeon forward --port 4096 --local-port 14096` | Foreground forwarding to guest loopback; neither starts nor authenticates the service |

Keep forwarded services authenticated. Interrupting a forward closes its host
listener, not an independently started guest service. OpenShell also provides
`dungeon exec --owned -- COMMAND` for bounded non-TTY process-group cleanup;
ownership assists lifecycle, not stronger containment.

**Remote machines and vendor desktops:** local container endpoints are Unix
sockets, not remote Docker contexts or SSH/TCP engines. Dungeon does not provision
remote hosts. Vendor browser/mobile/desktop connections need their own supported
setup and validation; generic SSH is not proof of desktop containment.
See [integration options and status](docs/integrations.md) and
[attachment details](docs/configuration.md#remote-first-attachment).

## Automation, limits and further reading

- [`dungeon machine --schema 1 ...`](docs/machine.md) exposes lifecycle JSON and,
  on OpenShell, workload-byte JSONL. Machine `init --runtime container --yes`
  supports explicit plain-container setup. External harnesses own orchestration.
- [Validation](docs/validation.md) records exact tested combinations, including
  real Claude guest workflows, native engines and connection checks. It does not
  establish every provider, token-refresh, desktop or platform combination.
- [Security](SECURITY.md) defines the boundary and remaining risks. No automatic
  host-home, engine-socket or native authentication database sharing is added.
- [Distribution](docs/distribution.md#clean-local-uninstall) covers destructive
  uninstall. Start with the installed binary's `dungeon uninstall --dry-run`;
  `--binary-only` retains configuration and runtime resources.
- Sandcastle integration is not implemented. Telltale is deferred, not part of
  the active runtime.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
node --test npm/launcher/test/platform.test.mjs npm/test/stage-packages.test.mjs
```

Read the [architecture](docs/architecture.md), [adoption guide](docs/adoption.md)
and [live validation runbook](docs/live-validation.md) for deeper procedures.

Apache-2.0. Third-party runtimes, agents, images and desktop products retain their
own licenses and service terms.
