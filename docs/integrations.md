# Agent and interface integration

The execution environment is the common boundary. Interfaces are alternatives,
not a requirement to use a particular agent protocol.

| Workflow | Dungeon 0.0.1 path | Validation status |
| --- | --- | --- |
| OpenCode CLI | `run opencode` | Argument contract tested; live session pending. |
| Claude Code CLI | `run claude` | Argument contract tested; live session pending. |
| Codex CLI | `run codex` | Argument contract tested; live session pending. |
| Other compatible CLI | `exec -- COMMAND ARG...` | Generic argument transport tested. |
| Interactive shell | `shell` | Delegates PTY to OpenShell; live terminal gate pending. |
| VS Code | `code` | Delegates OpenShell editor connection. |
| Claude Desktop / ChatGPT remote projects | `ssh-config`, then client setup | Documented integration target, not live-certified. |
| OpenCode remote TUI | Guest server plus protected tunnel | Manual alternative, not managed server lifecycle. |

Native Windows applications do not become Linux workloads by connecting to
Dungeon. Windows users can run the manager in WSL2 or connect through a supported
remote execution arrangement. Actual host/runtime combinations need validation.

## Terminal use

`run opencode --continue` passes flags to the installed guest agent. `exec --`
preserves separate arguments without evaluating a host shell. PTY allocation
is automatic for interactive `run`, explicit with `exec --tty --`, and forced
for `shell`. The backend owns terminal resizing and signal transport. Remote
exit codes propagate; no retry restarts an agent session automatically.

Provider authentication is configured through OpenShell before creation. The
launcher does not reuse every account on the host. A guest login is an explicit
choice to expose credentials inside that environment, not credential brokering.

## SSH, editors, and desktop clients

Run `ssh-config` and inspect the generated Host block. Install it deliberately
in your SSH configuration using the upstream workflow; do not blindly append
multiple duplicate entries. `code` uses OpenShell's managed SSH installation
instead. It may modify host SSH configuration. Select `/sandbox/project` in
the remote editor after upload.

A desktop client must support the resulting SSH ProxyCommand connection and
the remote Linux tools it launches. Configure Claude Code's SSH environment or
ChatGPT's remote project with that target. Account, app version, installation,
and authentication requirements remain those of the desktop vendor.

The desktop UI and local extensions/connectors are outside the workload.
Some desktop paths send actual credentials into the guest. They must not be
advertised as broker-only access. An arbitrary SSH connection is not evidence
that every desktop feature or all its tools execute inside the boundary.

## OpenCode server alternative

From `dungeon shell`, create a private server credential without placing it in
your shell history, then start the server:

```sh
cd /sandbox/project
umask 077
read -r -s -p 'Server password: ' OPENCODE_SERVER_PASSWORD; printf '\n'
export OPENCODE_SERVER_PASSWORD
opencode serve --hostname 127.0.0.1 --port 4096
```

From another host terminal, use the explicitly scoped OpenShell port-forward
workflow for port 4096. Confirm the listener is loopback-only, then connect the
host OpenCode client with `opencode attach http://127.0.0.1:4096 --dir
/sandbox/project` and its documented authentication mechanism. Use the guest
path, not the host project path. Verify the exact client version's flags.

Do not publish an unauthenticated server, place a password in a checked-in
configuration, or expose HTTP Basic Auth on an untrusted network. Keep the
forwarding process alive for the connection and stop it afterward. Dungeon does
not manage server credentials, service restarts, or forwarding lifecycle yet.

## Primary references

- [Pinned OpenShell CLI](https://github.com/NVIDIA/OpenShell/blob/v0.0.116/crates/openshell-cli/src/main.rs)
- [OpenShell environment management](https://docs.nvidia.com/openshell/latest/sandboxes/manage-sandboxes)
- [OpenCode server](https://opencode.ai/docs/server/)
- [Claude Code Desktop](https://code.claude.com/docs/en/desktop)
- [ChatGPT remote connections](https://learn.chatgpt.com/docs/remote-connections)

These establish upstream interfaces, not completed Dungeon compatibility tests.
