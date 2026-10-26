# Agent and interface integration

The execution environment is the common boundary. Interfaces are alternatives,
not a requirement to use a particular agent protocol.

| Workflow | Dungeon path | Validation status |
| --- | --- | --- |
| OpenCode CLI | `run opencode` | Synthetic edit/test passed in one OpenShell Podman guest; other providers/accounts untested. |
| Claude Code CLI | `run claude` | Reached onboarding/login-method screen; authenticated task not tested (Issue #11). |
| Codex CLI | `run codex` | Synthetic task passed after two one-time approvals outside Codex's inner `bwrap` sandbox, within OpenShell. |
| Codex login | `auth codex` | ChatGPT device login succeeded in guest; retained guest login checked after restart. |
| Claude Code login | `auth claude`, then `/login` | Reached login-method screen; account-dependent completion not tested (Issue #11). |
| Other compatible CLI | `exec -- COMMAND ARG...` | Generic argument transport tested. |
| Interactive shell | `shell` | PTY/Ctrl-C tested on the Issue #1 OpenShell Podman combination. |
| VS Code | `code` | NOT TESTABLE: client/Remote SSH extension/display unavailable in the recorded Issue #30 checkpoint. |
| Guest terminal editor over native SSH | `ssh-config`, private OpenSSH config, guest `nvim` | Edit/save/test and saved-state reconnect/restart passed; SSH disconnect left editor processes alive (Issue #30 partial). |
| Claude Desktop / ChatGPT remote projects | `ssh-config`, then client setup | Documented integration target, not live-certified. |
| OpenCode remote client | Explicit `attach`; manual `serve` + `forward` remains available | Matched 2.1.2 auth, host UI rendering, Enter/Ctrl-C and forward-failure recovery passed on the recorded Linux combination. The older matched 2.0.14 AI/session gate is separate evidence, not a new-flow AI task pass. |
| Owned foreground service | `exec --owned -- COMMAND ARG...` | Explicit opt-in shutdown/live gates passed on the recorded Linux pair; not a daemon or containment guarantee. |
| Owned interactive command | `ssh --owned -- COMMAND ARG...` | Native guest Neovim/resize/save/test, bounded invocation cleanup and failure gates passed on the recorded Linux pair; generic SSH disconnect semantics unchanged. |
| Read-only diagnostics | `doctor [--json]` | Inventory/private-state safety checks passed; does not dynamically certify integrations. |

Native Windows applications do not become Linux workloads by connecting to
Dungeon. Windows users can run the manager in WSL2 or connect through a supported
remote execution arrangement. Actual host/runtime combinations need validation.

## Terminal use

Interactive `dungeon` and `dungeon run` launch the same configured guest harness.
First use selects a harness and approves project scope; later use reuses that
binding. `run AGENT` overrides only this invocation. No host harness CLI is needed.
Bare `dungeon` requires a terminal; explicit `run` also accepts noninteractive
native flags, for example `dungeon run -- --version`.

`run opencode --continue` passes flags to the installed guest agent. `exec --`
preserves separate arguments without evaluating a host shell. PTY allocation
is automatic for interactive `run`, explicit with `exec --tty --`, and forced
for `shell`. The backend owns terminal resizing and signal transport. Remote
exit codes propagate; no retry restarts an agent session automatically.

Ctrl-C handling belongs to OpenShell and the guest CLI; ordinary launch does not
inject a second interrupt or stop the sandbox on exit. Single-interrupt exit from
Codex's welcome screen is [validated](validation.md#onboarding-cleanup-2026-10-25-source-checkout),
not a guarantee for every harness, screen or active task.

## Authentication

Dungeon supports two explicit vendor-native guest login coordinators:

```sh
dungeon --config "$CONFIG" auth codex
dungeon --config "$CONFIG" auth claude
```

Codex runs `codex login --device-auth` in `/sandbox`, so the user completes
the displayed URL and one-time code in a normal browser. Claude starts its
interactive CLI in `/sandbox`; use `/login` and complete the browser flow.

These are vendor-owned login flows. Dungeon does not parse their output, copy
provider tokens, implement OAuth callbacks, mint credentials, or synchronize the guest
credential store to the host. Credentials therefore have the same trust and
lifetime as the retained guest environment.

API-key/provider authentication is configured through OpenShell before creation.
That path is distinct from guest OAuth and can avoid exposing the original
provider secret to the workload when OpenShell supports credential brokering.
OpenCode remains provider-dependent and has no `dungeon auth opencode` command.

## SSH, editors, and desktop clients

Run `ssh-config` and inspect the generated Host block. Install it deliberately
in your SSH configuration using the upstream workflow; do not blindly append
multiple duplicate entries. A private file passed to native `ssh -F` avoids
changing `~/.ssh/config`. Trust only the reviewed native OpenShell executable
and generated ProxyCommand, never a project/tool-output shell snippet. The
recorded terminal-editor run disabled host identity keys, agents and forwarding; see
[its scope and disconnect limitation](validation.md#issue-30-native-ssh-terminal-editor-2026-10-03).
`code` uses OpenShell's managed SSH installation instead. It may modify host
SSH configuration. Select `/sandbox/project` in
the remote editor after `up`.

For a single foreground guest editor/tool with explicit invocation cleanup, see
[owned interactive SSH](configuration.md#owned-interactive-ssh). Native terminal
Ctrl-C is application input/signal, not automatic Dungeon shutdown. This path
does not retrofit VS Code/desktop clients or change generic `ssh-config` sessions.

A desktop client must support the resulting SSH ProxyCommand connection and
the remote Linux tools it launches. Configure Claude Code's SSH environment or
ChatGPT's remote project with that target. Account, app version, installation,
and authentication requirements remain those of the desktop vendor.

The desktop UI and local extensions/connectors are outside the workload.
Some desktop paths send actual credentials into the guest. They must not be
advertised as broker-only access. An arbitrary SSH connection is not evidence
that every desktop feature or all its tools execute inside the boundary.

## Host OpenCode attachment

Explicit interactive `dungeon attach` (not bare `dungeon`)
starts/resumes the sandbox, checks that host and guest versions match, starts
the guest service and forwards to a temporary host-loopback port. It verifies
unauthenticated denial and authenticated guest version, keeps the generated
password in process memory and supplies it only through the host client's
ephemeral `OPENCODE_PASSWORD`. The plain Enter prompt leaves the connection
details visible before opening the native host UI.

Normal exit or Ctrl-C stops the service and forward, retaining the sandbox. If
cleanup is unconfirmed, a plain prompt offers an exact-name-confirmed sandbox
stop or leaves it running. This does not guarantee cleanup after SIGKILL/crash,
handle concurrent attaches, automatically select the last conversation, or
adopt another service. Host config, provider auth, plugins and themes are not
automatically made available inside the guest.

The [remote-first guide](configuration.md#remote-first-attachment) also retains
the manual foreground `serve` / `forward` / native `--server` workflow. In that
path, supply the server's password only to the host client, not config or argv.
Do not pass a guest directory as a host positional path: that triggers local
`chdir`, not remote workspace selection.

For explicit non-TTY service ownership, see [owned foreground execution](configuration.md#owned-foreground-execution).
Interrupting `forward` removes only its host listener, not the independently
started guest service. Dungeon stores no password and does not supervise service
restarts. See the [current attachment checks](validation.md#foreground-opencode-attachment-2026-10-21-source-checkout)
and [historical matched-pair gate](validation.md#issue-29-matched-opencode-pair-2026-10-03)
for their separate tested versions and remaining limits.

## Primary references

- [Pinned OpenShell CLI](https://github.com/NVIDIA/OpenShell/blob/v0.0.116/crates/openshell-cli/src/main.rs)
- [OpenShell environment management](https://docs.nvidia.com/openshell/latest/sandboxes/manage-sandboxes)
- [OpenCode V2 CLI and explicit server connection](https://opencode.ai/v2/docs/cli)
- [OpenCode V2 API](https://opencode.ai/v2/docs/api)
- [Claude Code Desktop](https://code.claude.com/docs/en/desktop)
- [ChatGPT remote connections](https://learn.chatgpt.com/docs/remote-connections)

These establish upstream interfaces, not completed Dungeon compatibility tests.
