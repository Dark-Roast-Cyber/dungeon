# Configuration

## Read-only diagnostics

```sh
dungeon doctor
dungeon doctor --json
dungeon --config "$CONFIG" doctor --json
```

Doctor selects only an explicitly supplied configuration or an already existing
private binding for the current directory. Missing binding is informational and
does not prompt or create directories. Invalid selected config/policy is
blocking, but does not hide independent machine checks. Imports remain
**plan-only, runtime blocked**; nothing is imported or executed.

Diagnostic config/policy reads require a private user-owned configuration
directory and private user-owned regular files (no symlink, FIFO or hardlink).
Linux descriptor-relative `O_NOFOLLOW|O_NONBLOCK` opens, descriptor metadata and
bounded bytes feed the existing strict parser/validator; doctor does not reopen
the paths after a safety preflight. This is a diagnostic-only read restriction,
not a change to other commands' configuration-loading contract.

The supported diagnostic runtime boundary is Linux, OpenShell **0.0.116** and
rootless Podman. Other platforms report unsupported without invoking runtime
tools. Checks cover pinned CLI, rootless info/API socket readiness, local images,
`dungeon-local` registration, container, bridge network and gateway/driver health.
Queries never pull/install, register/select, start/stop, create a sandbox, exec,
upload, change policy or migrate a gateway. Only an existing default rootless
Podman store with an existing nonempty user-owned regular `db.sql` marker and
without a custom user `storage.conf` is queried. Missing, empty, legacy or custom
storage remains unknown rather than being initialized. Marker metadata is
checked; engine database contents are never read. A missing OpenShell
registration directory is not created.
Native tools may use their existing engine locks/caches; read-only here means no
Dungeon configuration/resource/lifecycle mutation, not syscall-level immutability.

Each query has a five-second deadline, 256 KiB stdout limit, discarded stderr,
null stdin and a cleared, narrowed HOME/XDG/locale/PATH environment. Executables
inside the current or selected project are refused, including symlink targets.
Every subprocess PATH also excludes both projects and canonical symlink-directory
aliases, so trusted runtime binaries cannot resolve helpers from those projects.
Query leaders are observed with `WNOWAIT`; their process-group identity remains
reserved until group teardown and reap, even if a descendant retains stdout.
Malformed/oversized/timed-out output becomes unknown/incompatible; raw output,
errors, provider values, certificates, passwords and environment dumps are never
printed. Findings contain fixed safe text and remediation links, not commands
executed on the operator's behalf.

Live findings distinguish `disabled-but-healthy`, `enabled-compatible`,
`unknown-incompatible`, and `requested-but-unavailable`. `keep_id_mapping` is
separate: it refers to **Podman driver** `userns = "keep-id:uid=998,gid=998"`,
not the gateway container's own user namespace. Ordinary `keep-id` is not enough.
Only the recognized Dungeon-owned local gateway config/container/image can
provide a configuration observation. This deliberately accepts the exact setup
config, its bind-enabled variant, and an optional exact driver userns line;
other TOML layouts/settings remain unknown. A config newer than the running
container is unknown, not assumed active. Local capability also remains unknown
unless the container reports exactly one matching private
driver-data `XDG_DATA_HOME` assignment and the identity-mounted path required by
the recorded live gate. Missing, wrong or duplicate assignments do not establish
compatibility. No complete environment values are returned by inspection.
Dedicated/foreign/remote gateways
remain unknown without an independently supported inspection contract. No local
file can establish a remote setting. These states are prerequisite observations,
not proof that a live bind works, that an agent can write it, or that containment
is enforced. Doctor does not promote live mode or enable it.

JSON schema **1** contains `schema`, `supported_openshell`, `read_only`,
`containment_verified` (always false), `exit_code`, and `checks`. Each check has
`check` (stable identifier), `state`, `blocking`, `detail`, `remediation`, and
`reference`. States also include `missing`, `healthy`, `incompatible`, `partial`,
`unknown`, `unsupported`, `blocked`, and `recorded-not-verified`. Exit 0 means no
blocking finding; exit 1 means at least one blocking prerequisite/configuration
finding. Missing binding and recorded integration gates alone are nonblocking;
requested live without recognized capability/mapping is blocking. An unsupported
foreign gateway observation alone is informational, but missing local machine
prerequisites can still block. CLI usage errors retain normal CLI exit semantics.

Claude account, matched native-client, nested Codex sandbox and editor gates are
reported as **recorded-not-verified**. Doctor performs no login, auth database
inspection, AI task, guest probe or native-client compatibility test. Consult
[validation](validation.md) for the exact recorded combinations and remaining
gates.

## Environment configuration

Dungeon keeps one private directory per environment and never discovers configuration or policy from a repository. For normal interactive use, `dungeon` resolves the current directory, or an explicit positional project path, to a deterministic private binding under `${XDG_CONFIG_HOME:-$HOME/.config}/dungeon/projects/<project-key>`. Explicit `--config PATH` remains the authoritative automation/recovery surface and is never silently replaced by project resolution.

`dungeon setup` needs no project configuration. On Linux it uses gateway `dungeon-local`, workspace `default`, port 17670, and rootless Podman. The gateway container's config, generated mTLS/JWT keys, database, and private OpenShell driver data live in `${XDG_STATE_HOME:-$HOME/.local/state}/dungeon/local-setup` (private host state). The `driver-data` directory is mounted at the identical absolute path inside the trusted gateway and supplied as `XDG_DATA_HOME`; OpenShell 0.0.116 may later give a supervisor binary from that directory back to host Podman as a bind source when sandbox user namespaces require extraction. This path is gateway-only and is never mounted into the sandbox as Dungeon project authority. OpenShell owns its gateway registration and mTLS client material in `${XDG_CONFIG_HOME:-$HOME/.config}/openshell/gateways/dungeon-local`; Podman owns the `dungeon-local` network and `openshell-gateway-dungeon-local` container. Neither stores Dungeon project policy. A conflicting or partial installation requires explicit operator inspection; setup does not overwrite it. The initial `dungeon init` prompt defaults to `dungeon-local`, but use `--image localhost/dungeon-agent:0.0.3-dev` only after building the reviewed local image; the community base remains the CLI image default until a separate publication decision.

### Local setup modes

| State | `setup` | `setup --enable-live` |
| --- | --- | --- |
| Fresh, prerequisites established | Create copy-capable gateway | Create experimental live-capable gateway |
| Exact running copy gateway | Verify unchanged | Refuse; safe migration is blocked |
| Exact running live gateway | Verify unchanged, no downgrade | Verify unchanged |
| Stopped, unknown, foreign, partial or legacy layout | Refuse, no start/repair/adoption | Refuse, no start/repair/adoption |

Add `--dry-run` for bounded read-only preflight and the capability diff. Missing
prerequisites block; no directories, certificates, registrations, network,
container or setup lock are created. Native inventory can use existing engine
locks/caches. Missing/empty/custom default storage is not queried or initialized.
Setup never pulls images or enables the API service.

Live adds these **Podman driver** settings, using the pinned upstream key
`userns`, not `userns_mode`:

```toml
enable_bind_mounts = true
userns = "keep-id:uid=998,gid=998"
```

Gateway/certificate generation retains plain `keep-id`; sandbox mapping is a
separate setting. This enables a trusted driver ceiling only. A project still
needs explicit `project_transfer: live` in private configuration. Setup creates
no project mount, changes no policy and converts no binding. Generated/default
transfer remains copy.

Setup recognizes only its exact generated copy/live configuration and pinned
image, effective environment (including image defaults), gateway user/mappings,
command, security settings, network/loopback bindings and complete mount set
(source, destination and read mode). Extra/duplicate assignments or mounts,
partial bind/mapping settings and legacy non-identity driver-data are refused.
Local paths must be normalized, symlink-free and outside projects. Private
directory descriptors and bounded nonblocking regular-file reads refuse FIFO,
symlink, hardlink and unsafe-permission configuration. Native public certificates
may be 0644 inside private directories; keys and copied client material remain
private. Existing state is never recursively chmodded/chowned. Raw query output,
environment values and stderr are not printed.

Fresh creators serialize with a private `dungeon/setup.lock` under the state
root, recheck resource absence and create files exclusively. Native mutations
have bounded deadlines and are not blindly retried. Failure or interrupt may
leave partial new state requiring inspection; no automatic destructive rollback
or partial-state adoption is provided.

Copy-to-live migration needs separately proven maintenance/concurrency and
interruption recovery preserving registration, TLS, database, driver data,
sandbox IDs and volumes. A Dungeon lock cannot exclude other OpenShell clients.
This slice does not enumerate/drain workloads or stop/restart/remove an existing
gateway. Do not remove existing state merely to make `--enable-live` succeed.

The fresh plain/flagged CLI and verification paths have synthetic coverage and
one separate disposable Fedora CoreOS 44/Podman 5.8.4 native record, including
gateway health, native certificate/database access and UID 998 workspace writes.
See [the exact setup gate](validation.md#issue-24-experimental-fresh-setup).
This is not migration, live-default promotion or broader platform attestation.

`dungeon.json` schema 2:

```json
{
  "schema": 2,
  "name": "demo",
  "gateway": "local",
  "workspace": "default",
  "project": "/home/user/projects/demo",
  "project_transfer": "copy",
  "image": "ghcr.io/nvidia/openshell-community/sandboxes/base:latest",
  "agent": "opencode",
  "providers": []
}
```

Schema 2 deliberately removes the old `policy` field. `policy.json` is always the adjacent policy file, so making its path configurable created an invalid state without adding capability. This pre-1.0 preview does not carry schema-1 compatibility code.

Names are lowercase DNS labels; sandbox `name` is limited to 19 characters by OpenShell 0.0.116. `project` must be an absolute project directory, not root/home, and Dungeon config must be outside it. `project_transfer` is the closed domain `copy` or `live`; omission preserves copy semantics and the same fingerprint as explicit `copy`. Selecting `live` changes environment identity. Providers are names, never secret values. Local image build paths and unqualified image names are rejected.

## Owned foreground execution

```sh
dungeon --config "$CONFIG" exec --owned -- opencode serve --hostname 127.0.0.1 --port 4096
```

`--owned` is explicit lifecycle assistance, not process containment or a
background-service manager. It requires a Linux host/guest, guest `/usr/bin/node`
with the built-in Node APIs used by the shim, and readable Linux `/proc`.
Missing primitives refuse execution; Dungeon installs nothing and changes no
policy. The reviewed image already supplies Node. This first slice is non-TTY,
rejects `--tty`, supplies null stdin, and requires an existing Ready binding;
it does not create/start a sandbox or upload files.

A fixed guest-only shim forwards argv without shell evaluation. Its live
session/group guardian keeps group identity reserved until final group KILL.
An outer guest wrapper receives the primary command status over private IPC,
reaps the guardian, verifies kernel process-group absence, and records completion in an exclusive ephemeral guest
`/tmp/dungeon-owned-<invocation>` directory. The workload inherits neither
control descriptors nor IPC. Secondary OpenShell exec requests STOP; it never
accepts a PID or process-group ID. Regular control files are opened nonblocking
without following symlinks, without truncation, and through a held directory
descriptor. Completion is read separately from workload output; successful
confirmation removes the control directory without recursive deletion.

Normal command exit (including nonzero) cleans up lingering group members and
returns that exit code. Host INT/TERM returns 130/143 only after confirmation.
The guardian sends TERM to its own group, then KILL after one second; it remains
the live leader through the final signal.
External/OOM guardian death alone never confirms cleanup: the outer wrapper
requires an `ESRCH` result from a signal-zero group existence query within two
seconds. It never signals a reaped guardian's group. Surviving members retain
the group ID; ID reuse can cause conservative refusal, not hide those members.
Unreaped zombies can likewise cause unconfirmed cleanup, even if no work runs.
Host transports run in separate groups
and are killed/reaped without signalling a recycled host PID. Identity/policy
are checked before launch and cleanup against the original binding. Cleanup and
each prelaunch identity check are bounded to ten seconds. Transport failure,
invalid/missing completion or identity drift produces `owned cleanup unconfirmed`
and nonzero exit; temporary state and guest work may remain for inspection.
A failed primary transport remains an error even when INT/TERM is also pending;
130/143 require both confirmed guest cleanup and a valid primary transport exit.

There is no guarantee after abrupt host SIGKILL or total transport loss, no
protection against same-UID guest sabotage, and no cleanup guarantee for processes
that escape the group (for example daemonizing/`setsid` descendants). Do not use
`--owned` around vendor background-service start commands or infer ownership
from `/api/info` PID. Forced KILL can affect application durability; Dungeon never
deletes project/session state. No password/credential store or persistent PID
registry is added. Ordinary `exec`/`run` retain native transport semantics;
interrupting an independently launched `forward` stops its listener only and
leaves the service running.

## Owned interactive SSH

```sh
dungeon --config "$CONFIG" ssh --owned -- nvim /sandbox/project/file.txt
```

This explicit Linux-only command requires a real foreground host terminal on
stdin/stdout/stderr, host OpenSSH outside the selected project, guest
`/usr/bin/node` and Linux `/proc`, and an existing Ready binding. It does not
create/start/upload, install tools, change policy, or offer arbitrary SSH options.
The command starts in `/sandbox/project`. Arguments remain data, including quotes
and newlines; only fixed guest code is executed. Do not use this mode for daemon
launchers or job-control shells whose children leave the invocation group.

Dungeon checks the exact OpenShell 0.0.116 SSH template before using a temporary
private 0600 `-F` configuration. Unexpected directives/template drift refuse
execution. Native none-auth SSH goes through the trusted OpenShell mTLS proxy;
upstream host-key checking/storage remain disabled. Host identity files, agent,
password/key authentication, forwarding, multiplexing, local commands and SSH
escape sequences are disabled. No `~/.ssh` modification or credentials copying
occurs. The narrowed Runtime environment supplies existing OpenShell registration
paths; it does not copy host HOME/XDG contents into the guest.

Native SSH owns terminal input, raw mode and resizing. Typed Ctrl-C retains the
application's terminal semantics: it can cancel a Neovim command without shutting
down the guardian. Explicit host HUP/INT/TERM requests STOP instead. Native SSH
suspension is unsupported: observed STOP/CONT is a sticky transport error.

A foreground guest guardian retains the native controlling terminal and launches
the application in its own group, without control/IPC descriptors. A detached
silent invocation observer exclusively creates the same ephemeral control files,
acknowledges startup and observes final kernel group absence. Startup and status
acknowledgements are bounded; failure enters guardian shutdown. A HUP/TERM fault
during a pending normal acknowledgement sends a new sticky-fault revision; an
older acknowledgement cannot authorize normal completion. Only the still-live
guardian sends group TERM then KILL after one second. The observer never authorizes
signalling based on a receipt/PID and never signals the guardian's former group.

The strict receipt has exactly `token`, `clean`, `stopped`, `fault`, `status`.
`status` is explicitly null or exactly `{code, signal, spawn_error}`: observed
0–255 code, recognized Linux signal, or acknowledged spawn error, never a guessed
application exit. Unknown/duplicate fields, inconsistent combinations and invalid
types refuse confirmation. Normal exit returns the observed application status;
spawn failure returns 127. Confirmed explicit host signals return 129/130/143 as
host policy, even if application status is absent. Null status without a host
interrupt is an error. Upstream SSH reports the guardian's final group KILL as
exit 1; only that expected primary result plus a valid receipt permits success.
Failed/signalled/stopped/continued transport, guardian faults and failed terminal
restoration remain errors, regardless of a pending host signal or cleanup receipt.
The host makes its final status/signal decision after private transport teardown
and terminal restoration, while signal handlers remain active. A late interrupt
without acknowledged STOP is an error, not a swallowed normal exit.

Ready identity/policy checks precede launch and cleanup. Startup readiness and
cleanup each have a ten-second budget. Lost cleanup transport or drift returns
bounded unconfirmed error and can leave guest work/controls for inspection.
Detected terminal hangup also requests cleanup; a physically vanished terminal
cannot be restored and remains an error. Handlers stay active through host private
group teardown/reap and terminal restoration. Host SIGKILL, total transport loss,
group escape, same-UID sabotage and unreaped zombies retain the earlier ownership
limits. Saved project/editor state is preserved, but forced KILL can lose unsaved
buffers. No whole-sandbox stop is used for invocation cleanup. Ordinary
`exec --owned`, `exec`, `run`, `forward`, `ssh-config` and `code` are unchanged.

## Native policy

`policy.json` is the single editable OpenShell policy input. `dungeon init` generates one starter development policy. Additional named profiles are deferred until live validation establishes real scenario-specific semantics.

The starter policy requires Landlock `hard_requirement`, the non-root sandbox account, read-only system paths, writable guest `/sandbox` and `/tmp`, model endpoints, `platform.claude.com:443` for Claude Code authentication, common development package endpoints, GitHub, and VS Code endpoints.

Dungeon does not widen policy automatically. Inspected REST/TLS endpoints must use `enforcement: enforce`.

Review `plan` before `up`. `runtime.json` is Dungeon-owned identity bookkeeping and must not be edited to bypass mismatch checks.

## Private project binding

Direct launch accepts an optional project path:

```sh
dungeon
dungeon ../other-project
```

Omitting the path uses the current directory. Dungeon canonicalizes that path,
rejects filesystem root/home scope, derives a stable private project key, and
resolves only the matching Dungeon-owned config directory. It does not scan the
repository or other config directories.

If no private binding exists, direct launch, `up`, `create`, or `plan`
may initialize one only in an interactive terminal. Before writing files,
Dungeon shows the canonical project, private config path, transfer mode,
gateway, image, and default agent and requires confirmation. The generated
binding currently starts in `copy` mode; Issue #24's live candidate remains
explicit pending a separate promotion decision.

A moved or renamed project gets a different private project key and is never
silently rebound. The old explicit config remains usable through `--config`
for inspection or recovery. A partial or mismatched private binding fails
closed rather than being guessed or overwritten. Private binding directories
use the existing Dungeon 0700 directory and 0600 file creation rules.

Subcommands without `--config` resolve the current directory's private
binding. The positional project path is intentionally the direct-launch surface;
for a subcommand against another project, run it from that directory or use the
explicit config.


Use the same explicit config for lifecycle commands: `dungeon --config PATH up`,
`run opencode` (or another supported command), and `down`.

In `copy` mode, first `up` creates the sandbox and uploads the configured
project once. Subsequent `up` resumes without uploading, and does nothing if
the sandbox is already Ready. `run` starts a stopped sandbox already bound to
this config without uploading. `down` stops but preserves guest state. To
intentionally refresh/recover the guest project, use
`dungeon --config PATH upload --yes`; it overwrites the guest copy and may
discard guest-side changes. To return changes, use
`dungeon --config PATH export` only while the bound OpenShell 0.0.116 sandbox
is Ready. Dungeon downloads/stages the complete guest tree, prints a
deterministic summary, and waits for typed `yes`; `--yes` explicitly approves
without the prompt. Host-only files are preserved and three-way conflicts or
special entries are refused. It applies regular contents, directories, and the
Unix executable bit only. On failed apply, affected host files have private
adjacent backups for manual recovery.

`live` is an explicit pre-validation candidate. First `up` derives one
Podman bind request from the canonical configured project to
`/sandbox/project`, read-write, with the OpenShell 0.0.116
`selinux_label = "private"` candidate, and performs no upload. Resume/start
reuses the same sandbox without upload. `upload` and `export` refuse before
contacting OpenShell. Dungeon does not accept raw driver JSON or arbitrary
mount lists.

The live candidate requires trusted gateway configuration with
`enable_bind_mounts = true` and
`userns = "keep-id:uid=998,gid=998"`. Those are Podman driver settings, not
`policy.json` capability. The 2026-10-01 Fedora/OpenShell/Podman record is in
[validation](validation.md); it does not make live the generated default.
Existing/default `dungeon init` remains `copy`. The current `dungeon-local`
setup is not migrated automatically, and no host chmod/chown workaround is
part of the design.

The three-way baseline is `baseline.json` in the private config directory,
bound to the config fingerprint and sandbox ID. It stores only pre-upload host
entries that exactly match post-upload guest entries (type, file hash, and Unix
executable bit). Filtered paths left stale in the guest and guest edits during
upload are not claimed as common state. An older config without a
baseline is refused: manually download and recover the guest project with
OpenShell, inspect it, then run `upload --yes` explicitly to create a new
baseline. Dungeon does not guess or synthesize the missing baseline.


## Remote-first attachment

Keep UI/editor configuration on the host; run the agent, tools, credentials,
and sessions in the guest. A host editor can watch the configured checkout
directly when `project_transfer` is explicitly `live` on the validated local
Podman gateway. This is a local bind, not cross-machine file synchronization:
the source must exist on the gateway's Podman host. A remote gateway does not
make a laptop checkout live automatically. Copy mode remains available with
reviewed export.

Existing `code` and `ssh-config` use OpenShell's native remote attachment.
Run `up` first. VS Code Remote and each vendor's SSH/desktop integration need
their own validation; generic SSH does not establish support for Codex remote
mode or Claude Desktop. Editor configuration need not be imported just to use
these paths. Guest-installed extensions and tools still execute inside isolation.

For HTTP clients, `forward` uses OpenShell 0.0.116's gRPC service forwarding:

```sh
dungeon --config "$CONFIG" up

# Terminal 1: interactive foreground service in /sandbox/project
dungeon --config "$CONFIG" exec --tty -- opencode serve --hostname 127.0.0.1 --port 4096

# Terminal 2: forward only while needed
dungeon --config "$CONFIG" forward --port 4096 --local-port 14096
```

Both commands run in the foreground. `forward` accepts nonzero ports only;
`--local-port` defaults to `--port`. Both ends are fixed to `127.0.0.1`.
It verifies the existing Ready sandbox's identity and policy before delegating
to OpenShell, does not create/start a sandbox, and preserves OpenShell's exit
status. In a real terminal, Ctrl-C through `exec --tty` stopped the guest
server on the tested pair. Non-TTY process-group interruption did not guarantee
server cleanup and left it running. Ctrl-C in Terminal 2 stops only the forward,
not the guest server or sandbox; stop the server separately when finished. No background registry, host listener daemon,
SSH config mutation, container port publication, or policy widening is added.

OpenCode V2 generates and prints its own server password. Treat it as a secret;
do not put it in Dungeon config, logs shared with others, shell arguments, or
project files. The forwarded API grants control of the guest agent: trust the
client, keep service authentication enabled, and remember that host loopback
is reachable by other local users/processes. Dungeon does not store the password
or synchronize host agent credentials/configuration.

In a third host terminal, use the server password printed in Terminal 1.
OpenCode V2's native client reads `OPENCODE_PASSWORD` for remote Basic auth;
enter it without putting it in shell history or command arguments:

```sh
read -r -s -p 'Guest OpenCode server password: ' OPENCODE_PASSWORD
printf '\n'
OPENCODE_PASSWORD="$OPENCODE_PASSWORD" opencode --server http://127.0.0.1:14096
unset OPENCODE_PASSWORD
```

Do not supply `/sandbox/project` as a host positional directory: OpenCode tries
to `chdir` locally. With no directory argument, the tested TUI opened the
guest server's `/sandbox/project` location. This uses a native client setting,
not a new Dungeon password option or store ([V2 source](https://github.com/anomalyco/opencode/blob/v2.0.22/packages/cli/src/services/server-connection.ts)).

**Validated observation:** host OpenCode 2.0.22 authenticated to guest 2.0.14
(with a version warning), opened the remote TUI, and completed a native remote
AI read/edit/test task. The same session and exact message history survived
guest restart and resumed successfully. A real terminal Ctrl-C through
`exec --tty` stopped the guest server. By contrast, interrupting non-TTY host
Dungeon exec left the guest service running; interrupting `forward` stops only
the forward. Interactive TUI message submission remains unverified, and this
version pair is not a matched-version validation. See the
[recorded evidence](validation.md#remote-first-service-forwarding).

## Reviewed import declarations (plan-only first slice)

Schema 2 now accepts an optional `imports` array in the same private `dungeon.json`. For the first Neovim case only, add an explicit directory source and the fixed destination:

```json
{"imports": [{"source": "/home/user/.config/nvim", "destination": "/sandbox/.config/nvim"}]}
```

Add the `imports` member to the existing config; the example omits its other required fields.

`plan` checks the directory and prints the source, destination, file hashes, requested image-owned `nvim`, and separate guest-private Neovim state paths. It does not copy or execute the files, contact OpenShell, or verify that the selected image contains `nvim`. No host content is sourced. Sources must be absolute, symlink-free ordinary directories with symlink-free regular contents, outside the project and private config; traversal, special entries, known credential/native-state paths, and duplicate destinations are rejected. Inspect all content yourself: these checks are not a secret scanner. There is no project-local discovery.

**Application is not implemented:** `create`, `up`, `run`, and other runtime operations refuse configurations declaring imports. OpenShell 0.0.116 uploads as the sandbox user into writable guest scope; merely changing the uploaded file modes would not make them read-only to that user. Do not treat the plan as an applied snapshot or an isolation test. Remove the declaration to use existing runtime operations; a safe, tested guest read-only mechanism is required before enabling import application. This does not change `policy.json`, the reviewed image, or environment propagation.

The live read-write workspace transfer candidate is implemented separately from imports. It is narrowly guarded to the canonical configured project and is not the generated/default path; it does not change the import model or remove the copied upload/export automation path. The recorded OpenShell 0.0.116, rootless Podman 5.8.4, Fedora/SELinux gate is in [validation](validation.md). Promotion remains a separate decision.

The accepted capability model continues to extend this single authoritative configuration, not a parallel plugin configuration. General declarative fields for the remaining request categories are not added in this first slice (the Neovim requirement/state paths shown in plan are derived, and existing `providers` remains the named provider binding):

- `requirements`: binaries/tool versions expected from the reviewed image;
- `state`: guest-private writable locations;
- `network`: destinations a setup would like to use;
- `credentials`: named provider/credential requirements only; and
- `environment`: bounded guest-only variable assignments required to connect config and state.

Raw credentials, shell snippets, host commands, arbitrary lifecycle hooks, privileged mounts, and engine sockets are not valid setup declarations.

The resolved plan must show the requested data and capability diff before anything is applied. Missing network/provider authority does not cause automatic policy widening. Project content may request setup in a future model, but project-local requests are untrusted and cannot grant themselves access.

A Neovim import remains the first application target: copy only selected config into an enforceably read-only guest snapshot, keep Neovim data/state/cache guest-private, and treat Lua/editor configuration as guest-executable content. The Rust toolchain is supplied by the reviewed OCI image rather than installed from a host script; Cargo network or registry credentials remain explicit policy/provider concerns.
