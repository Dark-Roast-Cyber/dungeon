# Dungeon

Terminal-first, policy-managed environments for coding agents. Part of the
[Agent Archaeology](https://agentarchaeology.ai/) family, with optional
[Telltale](https://github.com/Dark-Roast-Cyber/telltale) integration.

**0.0.1 is an engineering preview, not an audited isolation product.** Dungeon
coordinates an installed OpenShell runtime. It does not implement a hypervisor,
replace agent interfaces, or claim to contain an entire desktop application.

## Scope

- Direct terminal execution of OpenCode, Claude Code, Codex, and other commands.
  ChatGPT subscription-backed terminal use is through Codex, not a `chatgpt` CLI.
- Optional VS Code and SSH attachment for compatible desktop clients.
- One explicit environment configuration and native OpenShell policy.
- Guided initialization, reviewed launch plans, and explicit project upload.
- No automatic host home, configuration, credential, or session-database sharing.
- An optional Telltale diagnostic scan. Durable external telemetry collection is
  a follow-on gate, not a capability claimed by this preview.

The first backend targets **OpenShell 0.0.116**. Windows users may operate through
WSL2 or a supported remote execution host; native Windows workload containment
is not promised. CLI tests do not establish live platform, endpoint-protection,
or desktop-client compatibility.

## Install

Python 3.11 or later is required. Install from this repository; no PyPI release
is implied:

```sh
python -m pip install .
dungeon --help
```

Install the OpenShell CLI and register a gateway separately using its
[upstream instructions](https://github.com/NVIDIA/OpenShell/tree/v0.0.116).
Dungeon never installs a host daemon or changes its privileges on your behalf.
The OpenShell Python SDK does not install the CLI.

## First environment

Initialize a private configuration directory outside the project being opened:

```sh
dungeon init --directory "$HOME/.config/dungeon/demo" --project "$PWD"
```

The wizard asks for a gateway, environment name, image, default agent, and
starter policy. Noninteractive equivalents are available in `dungeon init --help`.
The upstream community `base` image includes the initial agent CLIs. Use a
reviewed image digest for repeatable deployments; mutable tags are not a supply
chain guarantee.

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

`upload` copies the selected project to `/sandbox/project`; it does not bind
mount the host working tree. Review project contents first. Git ignore rules
are not a secret scanner. Upload is separate from creation and may overwrite
files inside that guest project. No automatic synchronization back occurs.

Configure named providers in OpenShell, then list their names in Dungeon's
configuration before creation. Dungeon disables automatic provider discovery
and never copies native agent authentication files.

## Optional attachment

```sh
dungeon --config "$CONFIG" code
dungeon --config "$CONFIG" ssh-config
dungeon --config "$CONFIG" status
```

`code` uses OpenShell's VS Code integration, including its managed SSH
configuration. `ssh-config` prints an entry for deliberate installation into
your SSH configuration. Compatible Claude Desktop and ChatGPT remote-project
clients can use that SSH target. Desktop compatibility remains a live-validation
gate; local UI integrations are outside the workload boundary.

OpenCode `serve`/`attach` is an optional documented workflow, not a requirement
for terminal use. See [integrations](docs/integrations.md).

## Persistence and safety

An environment is retained when a command exits. `stop` and `start` delegate to
the backend. Persistence through backend stop/restart depends on its driver.
Deleting a sandbox can destroy its project and sessions. Dungeon 0.0.1 does not
automate deletion, export, native session imports, or recovery.

Policy templates request mandatory Landlock and blocking inspected rules.
OpenShell owns their enforcement. Configuration fingerprints detect accidental
reuse with different launch inputs; they are not policy attestations. A host or
gateway administrator remains trusted. See [security](SECURITY.md).

## Development

```sh
python -m pip install .
python -m unittest discover -s tests -v
python -m pip wheel --no-deps . --wheel-dir dist
```

See [architecture](docs/architecture.md), [configuration](docs/configuration.md),
[validation](docs/validation.md), and [roadmap](ROADMAP.md).
Apache-2.0; upstream runtimes, images, agents, and desktop products retain their
own licenses and service terms.
