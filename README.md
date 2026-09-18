# Dungeon

Terminal-first, policy-managed environments for coding agents. Part of the
[Agent Archaeology](https://agentarchaeology.ai/) family, with optional
[Telltale](https://github.com/Dark-Roast-Cyber/telltale) integration.

**0.0.2 is the current development preview; v0.0.1 remains the latest published
release. Neither is an audited isolation product.** Dungeon
coordinates OpenShell. It does not implement a hypervisor, replace agent
interfaces, or contain an entire desktop application.

## What ships

- Direct terminal execution of OpenCode, Claude Code, Codex, and arbitrary
  commands. ChatGPT-account terminal use is through Codex, not a `chatgpt` CLI.
- Vendor-native guest authentication for Codex device login and Claude Code
  browser/manual login. Dungeon never handles the resulting credential bytes.
- Optional VS Code launch and SSH configuration for compatible desktop clients.
- Guided setup, one explicit configuration, and editable native OpenShell policy.
- `minimal` and `development` starter policies with mandatory Landlock.
- Explicit project upload, separate guest state, and identity/policy-drift checks.
- Optional in-guest Telltale diagnostics. Durable independent collection is not
  implemented in this preview.

The adapter targets **OpenShell 0.0.116**. Linux/macOS and Windows through WSL2
or a remote Linux host are the intended deployment paths. Native Windows
workload containment is not promised. Offline tests do not establish live
runtime, desktop-client, or endpoint-protection compatibility.

## Install

Python 3.11 or later is required. From a checkout:

```sh
python -m pip install .
dungeon --help
```

The distribution is named `agent-dungeon`; no PyPI publication is implied.
Install the [OpenShell CLI](https://github.com/NVIDIA/OpenShell/tree/v0.0.116)
and register a gateway separately. Its Python SDK does not install the CLI.
Dungeon never installs a privileged host service on your behalf.

## First environment

Keep configuration outside the project that will be uploaded:

```sh
dungeon init --directory "$HOME/.config/dungeon/demo" --project "$PWD"
```

The wizard asks for a gateway, name, image, default agent, and starter policy.
For automation, add `--non-interactive --name demo --gateway YOUR_GATEWAY`.
Use a reviewed image digest for repeatable deployments. The default upstream
community `base:latest` image is convenient but mutable and not certified here.

Review `policy.json`. Configure named providers in OpenShell, then add their
names to `providers` in `dungeon.json` **before creating the environment**.
Dungeon disables automatic provider creation and copies no native auth files.
An empty provider list does not establish an authenticated model connection.

```sh
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

### Vendor-native authentication

For ChatGPT-backed Codex on a remote or headless environment:

```sh
dungeon --config "$CONFIG" auth codex
```

Dungeon runs `codex login --device-auth` in the guest. Complete the displayed
URL and one-time code in your normal browser. The vendor CLI stores its
credential in guest state; Dungeon does not receive or persist the token.

For Claude Code:

```sh
dungeon --config "$CONFIG" auth claude
```

Dungeon starts Claude Code in `/sandbox`. Use `/login` and complete the
vendor browser flow. API-key users can instead configure an explicit OpenShell
provider when that better matches their security model. OpenCode authentication
remains provider-specific and is not wrapped by `dungeon auth`.

`upload` transfers the project to `/sandbox/project`, not a host bind mount.
It may overwrite guest files. Review secrets first: Git ignore rules are not a
secret scanner. No automatic download or synchronization back occurs.
`run` and `exec` use the guest project directory; `shell` opens `/sandbox` so it
also works before upload. Unsupported network destinations remain blocked by
the backend rather than being approved automatically.

## Optional attachment and diagnostics

```sh
dungeon --config "$CONFIG" code
dungeon --config "$CONFIG" ssh-config
dungeon --config "$CONFIG" status
dungeon --config "$CONFIG" stop
dungeon --config "$CONFIG" start
```

`code` delegates to OpenShell's VS Code connection, which can install its
managed SSH configuration. `ssh-config` only prints an entry for review.
See [integrations](docs/integrations.md) for desktop SSH and OpenCode
`serve`/`attach`. These paths are optional; direct terminal use needs neither.

A reviewed custom image containing `telltale` can run:

```sh
dungeon --config "$CONFIG" telltale
```

This performs an in-guest `scan --once --dry-run --no-local-config --root
/sandbox`. It is not an independent audit, background monitor, durable export,
or proof of complete telemetry. The default upstream image does not include
Telltale. See [architecture](docs/architecture.md).

## Persistence and development

Exiting a command retains the environment. Stop/restart behavior follows the
backend driver. Dungeon does not automate deletion, project export, native
session imports, or recovery in 0.0.1. Before deleting a sandbox, preserve its
project and sessions using a reviewed backend workflow.

```sh
python -m pip install .
python -m unittest discover -s tests -v
python -m pip wheel --no-deps . --wheel-dir dist
```

Read [configuration](docs/configuration.md), [security](SECURITY.md),
[validation](docs/validation.md), and [roadmap](ROADMAP.md).
Apache-2.0. Runtimes, images, agents, and desktop products retain their own
licenses and service terms; no third-party binaries are bundled.
