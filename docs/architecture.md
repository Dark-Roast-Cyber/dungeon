# Architecture

Dungeon owns setup and lifecycle coordination. OpenShell owns workload isolation and policy enforcement. Reviewed images own the installed agent/tool versions. Agent interfaces own conversations and credential stores. Telltale is not part of the active runtime architecture.

```text
Host CLI
  -> explicit configuration + native OpenShell policy
  -> pinned OpenShell adapter
  -> managed environment: agents, tools, project, session state
       ^ terminal / optional SSH editor or desktop connection
```

## One implementation

`dungeon` is one Rust binary with four plain responsibility modules:

- `cli`: command parsing and dispatch.
- `config`: config/state parsing, validation, strict JSON, and fingerprints.
- `policy`: the starter policy and policy safety validation.
- `openshell`: the single OpenShell subprocess boundary and lifecycle operations.

There is no backend hierarchy, daemon, plugin ABI, embedded JS runtime, second policy language, or framework layer.

The npm tree is distribution only: a tiny Node launcher selects a platform package and executes its Rust binary. It must never parse Dungeon config, apply policy, coordinate auth, or contact OpenShell.

## Runtime identity

Initialization does not execute project code or contact a runtime. Creation snapshots the loaded policy, disables automatic provider creation, and labels the sandbox with a configuration fingerprint. Dungeon records sandbox ID and observed-policy digest in host-side `runtime.json`.

Reuse verifies identity and policy drift. These checks are not kernel/image attestation and do not prove provider content stayed unchanged.

## Storage ownership

| Data | Owner |
| --- | --- |
| Host project | User-owned; never mounted by Dungeon |
| Guest project | Backend-owned explicit copy |
| Agent sessions | Guest agent-owned |
| Config/runtime binding | Private host directory |
| Credentials | OpenShell providers or vendor-owned guest login |

No host dotfiles, editor caches, agent databases, private SSH keys, engine sockets, or credential directories are copied automatically.

## Product boundary

Dungeon's near-term product is a low-friction contained development environment:

```text
developer
  -> Dungeon setup/lifecycle/project transfer
  -> OpenShell isolation and native policy enforcement
  -> reviewed agent image
  -> agent + tools + isolated project copy
```

The host project remains authoritative. Dungeon does not make the sandbox a writable view of the host project. Normal development should converge on explicit upload plus safe staged export rather than weakening containment with a writable bind mount.

A reviewed Dungeon image may pin compatible agent/tool versions, but it does not own vendor authentication formats, models, or session protocols.

## Declarative setup boundary

Reusable editor/tool setup must remain data, not a host plugin mechanism. The accepted first design keeps the reviewed OCI image as the authority for installed binaries and toolchains. Dungeon does not add a second package manager or execute host configuration scripts to construct an environment.

A future setup request may describe only bounded inputs and requirements:

- user-selected host files/directories to snapshot into the guest;
- expected binaries already supplied by the reviewed image;
- private guest-writable state/cache locations;
- requested network destinations;
- named credential/provider requirements; and
- controlled guest environment variables needed to point a tool at its snapshot/state.

Those declarations are requests, not authority. `policy.json` remains the enforcement authority. Setup cannot silently widen filesystem, network, process, provider, mount, or credential access. An unsatisfied request must remain visible in the plan and fail closed or require explicit operator action.

Guided setup and noninteractive configuration must resolve through the same model and produce the same capability/data diff. A future AI helper may propose that same request, but cannot approve its own requested access.

### Snapshot imports

The first implementation should support reviewed read-only snapshots only, not live host mounts.

- The host source path is selected explicitly from Dungeon's private configuration, never discovered from project content.
- Snapshot preflight rejects special files and symlinks for the initial implementation rather than trying to preserve ambiguous link semantics.
- Imported editor configuration is treated as code that may execute **inside the guest**. Dungeon never sources or evaluates it on the host.
- Host home/config/cache trees, credential stores, browser state, SSH keys, agent databases, and shared caches are never implicitly imported.
- Writable tool state is separate from the read-only snapshot so the guest cannot mutate the host source or turn a reviewed config snapshot into a bidirectional sync channel.
- Updates produce a new reviewed snapshot and a data/capability diff. They do not mutate permissions implicitly.

Example boundaries:

| Setup | Image-owned requirements | Host snapshot | Private writable state | Network request | Credential request |
| --- | --- | --- | --- | --- | --- |
| Neovim config | `nvim` | user-selected Neovim config only | editor data/state/cache under guest-owned state | none added automatically; plugin endpoints must already be policy-authorized | none |
| Rust toolchain | `cargo`, `rustc` | none | Cargo/cache/build state inside guest | crates registry endpoints if needed, shown as an unsatisfied request unless already allowed | none by default; private registry auth must use an explicit provider/credential path |

Development Container Features may be useful as **review input** for image construction, but their install scripts, mounts, capabilities, and lifecycle hooks are not a Dungeon runtime plugin contract. Dungeon should not execute arbitrary Feature install hooks as part of this first setup model.

If organization policy is added later, it is a ceiling. Private host configuration, project requests, guided setup, invocation flags, or AI-proposed configuration may narrow or request within that ceiling but cannot weaken it.

## Deferred Telltale boundary

There is no Telltale command, dependency, collector, event model, or persistent collection state in Dungeon.

Telltale integration is deferred until Telltale 0.7.0 is released with Event4 as the intended external telemetry contract and the relevant downstream integration boundaries are stable. Dungeon must not bind to transitional Event 3, NormalizedRecord, Canonical Observation v2 migration details, or current collector internals.

Issue #1 proved that a post-exit OpenCode SQLite snapshot can be transferred outside workload write authority on one Podman combination. That result is preserved in deferred Issue #2 only as research. It is not an active Dungeon architecture contract.
