# Configuration

Use one private directory per environment and pass `--config` explicitly. Dungeon never discovers config from a repository.

`dungeon setup` needs no project configuration. On Linux it uses gateway `dungeon-local`, workspace `default`, port 17670, and rootless Podman. The gateway container's config, generated mTLS/JWT keys and database live in `${XDG_STATE_HOME:-$HOME/.local/state}/dungeon/local-setup` (private host state). OpenShell owns its gateway registration and mTLS client material in `${XDG_CONFIG_HOME:-$HOME/.config}/openshell/gateways/dungeon-local`; Podman owns the `dungeon-local` network and `openshell-gateway-dungeon-local` container. Neither stores Dungeon project policy. A conflicting or partial installation requires explicit operator inspection; setup does not overwrite it. The initial `dungeon init` prompt defaults to `dungeon-local`, but use `--image localhost/dungeon-agent:0.0.3-dev` only after building the reviewed local image; the community base remains the CLI image default until a separate publication decision.

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
`enable_bind_mounts = true` and the currently unvalidated
`userns = "keep-id:uid=998,gid=998"` mapping. Those are Podman driver settings,
not `policy.json` capability. Existing/default `dungeon init` remains
`copy`; select `--project-transfer live` only for the pending live validation.
The current `dungeon-local` setup is not migrated automatically, and no host
chmod/chown workaround is part of the design.

The three-way baseline is `baseline.json` in the private config directory,
bound to the config fingerprint and sandbox ID. It stores only pre-upload host
entries that exactly match post-upload guest entries (type, file hash, and Unix
executable bit). Filtered paths left stale in the guest and guest edits during
upload are not claimed as common state. An older config without a
baseline is refused: manually download and recover the guest project with
OpenShell, inspect it, then run `upload --yes` explicitly to create a new
baseline. Dungeon does not guess or synthesize the missing baseline.


## Reviewed import declarations (plan-only first slice)

Schema 2 now accepts an optional `imports` array in the same private `dungeon.json`. For the first Neovim case only, add an explicit directory source and the fixed destination:

```json
{"imports": [{"source": "/home/user/.config/nvim", "destination": "/sandbox/.config/nvim"}]}
```

Add the `imports` member to the existing config; the example omits its other required fields.

`plan` checks the directory and prints the source, destination, file hashes, requested image-owned `nvim`, and separate guest-private Neovim state paths. It does not copy or execute the files, contact OpenShell, or verify that the selected image contains `nvim`. No host content is sourced. Sources must be absolute, symlink-free ordinary directories with symlink-free regular contents, outside the project and private config; traversal, special entries, known credential/native-state paths, and duplicate destinations are rejected. Inspect all content yourself: these checks are not a secret scanner. There is no project-local discovery.

**Application is not implemented:** `create`, `up`, `run`, and other runtime operations refuse configurations declaring imports. OpenShell 0.0.116 uploads as the sandbox user into writable guest scope; merely changing the uploaded file modes would not make them read-only to that user. Do not treat the plan as an applied snapshot or an isolation test. Remove the declaration to use existing runtime operations; a safe, tested guest read-only mechanism is required before enabling import application. This does not change `policy.json`, the reviewed image, or environment propagation.

The live read-write workspace transfer candidate is implemented separately from imports. It is narrowly guarded to the canonical configured project and is not yet a supported/default path; it does not change the import model or remove the copied upload/export automation path. Runtime support remains blocked on the OpenShell 0.0.116, rootless Podman 5.8.4, Fedora/SELinux validation gate.

The accepted capability model continues to extend this single authoritative configuration, not a parallel plugin configuration. General declarative fields for the remaining request categories are not added in this first slice (the Neovim requirement/state paths shown in plan are derived, and existing `providers` remains the named provider binding):

- `requirements`: binaries/tool versions expected from the reviewed image;
- `state`: guest-private writable locations;
- `network`: destinations a setup would like to use;
- `credentials`: named provider/credential requirements only; and
- `environment`: bounded guest-only variable assignments required to connect config and state.

Raw credentials, shell snippets, host commands, arbitrary lifecycle hooks, privileged mounts, and engine sockets are not valid setup declarations.

The resolved plan must show the requested data and capability diff before anything is applied. Missing network/provider authority does not cause automatic policy widening. Project content may request setup in a future model, but project-local requests are untrusted and cannot grant themselves access.

A Neovim import remains the first application target: copy only selected config into an enforceably read-only guest snapshot, keep Neovim data/state/cache guest-private, and treat Lua/editor configuration as guest-executable content. The Rust toolchain is supplied by the reviewed OCI image rather than installed from a host script; Cargo network or registry credentials remain explicit policy/provider concerns.
