# Configuration

Dungeon keeps one private directory per environment and never discovers configuration or policy from a repository. For normal interactive use, `dungeon` resolves the current directory, or an explicit positional project path, to a deterministic private binding under `${XDG_CONFIG_HOME:-$HOME/.config}/dungeon/projects/<project-key>`. Explicit `--config PATH` remains the authoritative automation/recovery surface and is never silently replaced by project resolution.

`dungeon setup` needs no project configuration. On Linux it uses gateway `dungeon-local`, workspace `default`, port 17670, and rootless Podman. The gateway container's config, generated mTLS/JWT keys, database, and private OpenShell driver data live in `${XDG_STATE_HOME:-$HOME/.local/state}/dungeon/local-setup` (private host state). The `driver-data` directory is mounted at the identical absolute path inside the trusted gateway and supplied as `XDG_DATA_HOME`; OpenShell 0.0.116 may later give a supervisor binary from that directory back to host Podman as a bind source when sandbox user namespaces require extraction. This path is gateway-only and is never mounted into the sandbox as Dungeon project authority. OpenShell owns its gateway registration and mTLS client material in `${XDG_CONFIG_HOME:-$HOME/.config}/openshell/gateways/dungeon-local`; Podman owns the `dungeon-local` network and `openshell-gateway-dungeon-local` container. Neither stores Dungeon project policy. A conflicting or partial installation requires explicit operator inspection; setup does not overwrite it. The initial `dungeon init` prompt defaults to `dungeon-local`, but use `--image localhost/dungeon-agent:0.0.3-dev` only after building the reviewed local image; the community base remains the CLI image default until a separate publication decision.

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
