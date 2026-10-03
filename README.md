# Dungeon

Low-friction, policy-managed environments for coding agents. Dungeon helps individuals and teams adopt [OpenShell](https://github.com/NVIDIA/OpenShell) where practical, run agents inside isolation while keeping terminal/editor workflows familiar, and prepare for optional [Telltale](https://github.com/Dark-Roast-Cyber/telltale) integration once its downstream contract is stable. Part of the [Agent Archaeology](https://agentarchaeology.ai/) family.

Dungeon is an environment manager and adoption layer, not an agent orchestrator. Agent CLIs, editors, desktop clients, CI systems, and external harnesses should be able to use Dungeon-backed environments while Dungeon mostly disappears into the background.

**0.0.2 is the current development preview; v0.0.1 remains the latest published release. Neither is an audited isolation product.**

## Product boundary

Dungeon owns the user-facing setup and environment-management layer:

- help users install, verify, and operate supported OpenShell configurations;
- define project scope and environment identity;
- configure reviewed tools, read-only configuration imports, guest-private state, network requests, and provider requirements;
- coordinate lifecycle, authentication entrypoints, project transfer, and safe return of changes;
- expose terminal, agent CLI, VS Code, SSH, and future remote attachment paths that feel native;
- provide stable machine-readable operations so external harnesses can use Dungeon as an isolation provider.

OpenShell owns workload isolation and policy enforcement. Agent tools and harnesses own prompts, model selection, iteration loops, issue processing, parallel-agent scheduling, review workflows, and other orchestration.

Dungeon must not weaken isolation for convenience. Project content may request capabilities in future configuration surfaces, but it cannot grant itself filesystem, network, credential, provider, mount, or privilege access.

## Current implementation

- One Rust CLI. Python, Node, and Bun are not runtime dependencies.
- Direct OpenCode, Claude Code, Codex, shell, and arbitrary command execution.
- Vendor-native guest authentication for Codex device login and Claude Code browser login.
- Optional VS Code and SSH attachment.
- Foreground forwarding from host loopback to guest-loopback services.
- Explicit config, policy, typed project-transfer intent, runtime identity, and safe copy-mode upload/export checks.
- One starter OpenShell policy. Additional profiles wait for validated scenario differences.
- A validated rootless-Podman/OpenShell path; broader runtime/platform support remains explicit future validation.

Dungeon targets **OpenShell 0.0.116**. The OpenShell version is intentionally pinned because Dungeon validates version-sensitive CLI and structured-output contracts. Windows management is intended through WSL2 or a supported remote Linux host; native Windows workload containment is not claimed.

## Build

The repository pins **Rust 1.98.1** in `rust-toolchain.toml`:

```sh
cargo build --release
./target/release/dungeon --help
```

For the supported Linux path, install OpenShell **0.0.116** and rootless Podman separately. Start the user Podman API socket (`systemctl --user enable --now podman.socket`) and explicitly pull the immutable OpenShell gateway image before setup:

```sh
podman pull ghcr.io/nvidia/openshell/gateway@sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645
dungeon setup
```

Setup provisions a local mTLS gateway named `dungeon-local` on loopback port 17670 with the Podman driver and workspace `default`. Repeat `dungeon setup` to verify it; incompatible existing registrations/containers are never overwritten. It does not create a sandbox, upload a project, install packages with privilege, download OpenShell, or fall back to MicroVM. Native Windows and macOS setup are not supported. Only the recorded Linux/Podman/OpenShell combination has live validation; setup health does not prove containment on every host.

The [local reviewed agent image](images/agent/README.md) pins reviewed OpenCode, Claude Code, Codex, Neovim, and Rust versions on the validated base. Build it with rootless Podman and explicitly select `localhost/dungeon-agent:0.0.3-dev` with `dungeon init --image`. It is not published or selected by default; publication and provenance remain separate work.

## First environment

After machine-level setup, the normal interactive launcher is project-oriented:

```sh
dungeon setup
cd my-project
dungeon
```

With no positional path, `dungeon` uses the current directory. Pass a directory
to launch another project directly:

```sh
dungeon ../other-project
dungeon /home/user/projects/demo
```

Dungeon canonicalizes the selected project and resolves one deterministic
private environment under
`${XDG_CONFIG_HOME:-$HOME/.config}/dungeon/projects/<project-key>`. It never
loads policy or security authority from the repository. On first use, an
interactive terminal is required: Dungeon shows the project, private config
path, copy transfer mode, gateway, image, and default agent, asks for
confirmation, writes the private configuration, then brings the environment up
and launches the configured agent.

After that, lifecycle commands resolve the same private environment from the
current directory without repeated config-path ceremony:

```sh
dungeon up
dungeon run opencode
dungeon code
dungeon down
```

The explicit configuration flow remains supported for deterministic automation,
recovery, and hand-authored environments:

```sh
dungeon init --directory "$HOME/.config/dungeon/demo" --project "$PWD" --gateway dungeon-local --image localhost/dungeon-agent:0.0.3-dev
CONFIG="$HOME/.config/dungeon/demo/dungeon.json"
dungeon --config "$CONFIG" plan
dungeon --config "$CONFIG" up
```

Explicit configs are not scanned or silently adopted as project bindings.
`--config PATH` remains authoritative when supplied.

The generated/default transfer mode is `copy`. On first `up`, copy mode creates the sandbox and uploads the configured project to `/sandbox/project`. A later `up` resumes the bound sandbox without uploading; if it is already Ready, `up` does nothing. `run` starts a stopped bound sandbox when needed, also without uploading. `down` stops the sandbox and preserves guest state.

Copy-mode export is a separate reviewed operation, allowed only while the bound sandbox is Ready on OpenShell 0.0.116. `dungeon --config "$CONFIG" export` downloads and stages guest changes, prints a deterministic summary, then requires interactive typed `yes` or explicit `--yes` before applying. Conflicts and special entries are refused. Upload remains a deliberate refresh/recovery action: `dungeon --config "$CONFIG" upload --yes` overwrites guest project contents and can discard guest-side edits.

Issue #24 also provides an explicit `--project-transfer live` pre-validation candidate. It derives one read-write Podman bind request for the canonical configured project at `/sandbox/project`, performs no upload, and refuses upload/export. It is not the generated/default path. The dedicated gateway bind-mount and keep-id settings have one recorded OpenShell 0.0.116/rootless-Podman/Fedora result, and `dungeon setup` does not enable or migrate them automatically.

## Authentication

```sh
dungeon --config "$CONFIG" auth codex
dungeon --config "$CONFIG" auth claude
```

Codex uses `codex login --device-auth`. Claude starts inside the guest so the user can run `/login`. Dungeon does not parse, copy, store, refresh, or escrow OAuth credentials. OpenShell providers remain the separate path for brokered API credentials. OpenCode authentication remains provider-specific.

## Optional attachment

```sh
dungeon --config "$CONFIG" code
dungeon --config "$CONFIG" ssh-config
dungeon --config "$CONFIG" forward --port 4096 --local-port 14096
dungeon --config "$CONFIG" status
```

Remote/editor attachment should remain a first-class way to use an isolated environment. Dungeon manages the environment boundary; the user should otherwise experience the normal CLI, editor, or remote workflow.

Start remote-first: keep the editor and its configuration on the host, and run
the agent/tools inside the guest. With explicit `live` transfer on the validated
local gateway, guest edits appear immediately in the host checkout; no Neovim
config import is needed. Copy mode still requires reviewed export.

`forward` exposes only a guest-loopback service on host `127.0.0.1`, runs in the
foreground, and requires the bound sandbox to be Ready with unchanged identity
and policy. It does not start a server, publish a container port, or manage
credentials. See [configuration](docs/configuration.md#remote-first-attachment)
for the OpenCode serve/forward/attach workflow and its validation limits.

## Harness interoperability

Dungeon will expose a small stable machine interface so external tools can treat it as an isolated execution environment without learning OpenShell internals.

Sandcastle is the first concrete compatibility target. The goal is functional compatibility with its isolated-provider seam, not an internal port of Sandcastle and not a Node dependency in Dungeon. Sandcastle and similar systems should own orchestration while Dungeon owns isolation-facing lifecycle, execution, transfer, identity, and capability contracts.

## Optional Telltale integration

Telltale is not part of the active Dungeon runtime. Integration remains deferred until Telltale 0.7.0 and Event4 provide the intended stable downstream contract.

The long-term goal is optional composition: teams may use Dungeon alone, Dungeon with OpenShell, Dungeon with Telltale, or the combined path where each component keeps a clear responsibility boundary.

## Distribution

GitHub release binaries are the native distribution contract. The intended npm convenience surface is:

```sh
npx @agent-archaeology/dungeon
npm install -g @agent-archaeology/dungeon
```

The npm package is only a launcher for the same Rust binary. Node 24 LTS is the supported launcher runtime; Node is not required by the native binary. The npm scope is provisional and is **not published yet**. See [distribution](docs/distribution.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test npm/launcher/test/platform.test.mjs
```

CI uses explicit GA runner images rather than mutable `*-latest` labels and pins third-party actions to immutable commits corresponding to stable releases.

Read [architecture](docs/architecture.md), [configuration](docs/configuration.md), [security](SECURITY.md), [validation](docs/validation.md), [live validation runbook](docs/live-validation.md), and [distribution](docs/distribution.md).

Apache-2.0. Third-party runtimes, agents, images, and desktop products retain their own licenses and service terms.
