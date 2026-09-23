# Dungeon

Terminal-first, policy-managed environments for coding agents. Part of the [Agent Archaeology](https://agentarchaeology.ai/) family. Future [Telltale](https://github.com/Dark-Roast-Cyber/telltale) integration is deferred until Telltale 0.7.0/Event4 provides a stable downstream contract.

**0.0.2 is the current development preview; v0.0.1 remains the latest published release. Neither is an audited isolation product.**

## What ships on main

- One Rust CLI. Python, Node, and Bun are not runtime dependencies.
- Direct OpenCode, Claude Code, Codex, shell, and arbitrary command execution.
- Vendor-native guest authentication for Codex device login and Claude Code browser login.
- Optional VS Code and SSH attachment.
- Explicit config, policy, project upload, and runtime identity checks.
- One starter OpenShell policy. Additional profiles will wait for validated scenario differences.
- A validated rootless-Podman/OpenShell path; broader runtime/platform support remains explicit future validation.

Dungeon targets **OpenShell 0.0.116**. The OpenShell version is intentionally pinned because Dungeon validates version-sensitive CLI and structured-output contracts; 0.0.116 is the current upstream release. Windows management is intended through WSL2 or a supported remote Linux host; native Windows workload containment is not claimed.

## Build

The repository pins **Rust 1.98.1** in `rust-toolchain.toml`:

```sh
cargo build --release
./target/release/dungeon --help
```

Install OpenShell separately and register a gateway.

The [local reviewed agent image](images/agent/README.md) pins current OpenCode,
Claude Code, and Codex distributions on the validated base. Build it with
rootless Podman and explicitly select `localhost/dungeon-agent:0.0.3-dev` with
`dungeon init --image`. It is not published or selected by default; publication
and provenance remain separate work.

## First environment

```sh
dungeon init --directory "$HOME/.config/dungeon/demo" --project "$PWD"
CONFIG="$HOME/.config/dungeon/demo/dungeon.json"

dungeon --config "$CONFIG" plan
dungeon --config "$CONFIG" create
dungeon --config "$CONFIG" upload --yes

dungeon --config "$CONFIG" run opencode
dungeon --config "$CONFIG" run claude
dungeon --config "$CONFIG" run codex
dungeon --config "$CONFIG" exec -- git status
dungeon --config "$CONFIG" shell
```

The project is copied to `/sandbox/project`; it is not bind-mounted and is not automatically synchronized back.

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
dungeon --config "$CONFIG" status
dungeon --config "$CONFIG" stop
dungeon --config "$CONFIG" start
```

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
