# Architecture

Dungeon owns setup and lifecycle coordination. OpenShell owns workload isolation and policy enforcement. Agent interfaces own conversations and credential stores. Telltale integration is intentionally deferred until evidence can be collected independently of the workload.

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

## Telltale boundary

There is no provisional in-guest Telltale command. The accepted future integration is independent collection with retained evidence and explicit source health. Dungeon should not add a weaker compatibility path before that work is validated.


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

## Telltale collection direction

The first independent Telltale integration should reuse Telltale's existing production OpenCode source and output contracts rather than introduce a Dungeon-specific source or event.

The intended first slice is:

```text
guest OpenCode opencode.db
    -> completed SQLite snapshot after OpenCode exits
    -> private host acquisition root
    -> telltale scan --client opencode
    -> current Telltale privacy/detection/state/durable output
```

Current pushed Telltale production still uses `NormalizedRecordV1` and Rule v1, with Event 3.0 as the frozen external compatibility contract. `opencode.sqlite` is the authoritative OpenCode production source identity. Dungeon must not make Event4, Canonical Observation v2, or Detection v2 activation a prerequisite for this integration.

For a rooted Linux scan, Telltale expects the staged OpenCode database at `<root>/.local/share/opencode/opencode.db`. The retained Telltale log/state/outbox belong outside the sandbox and outside agent write authority.

Issue #1 established this synthetic acquisition path with OpenCode 1.2.18 and OpenShell 0.0.116 on rootless Podman: the guest database at `/sandbox/.local/share/opencode/opencode.db` used WAL; after OpenCode exited, `opencode db "VACUUM INTO ..."` completed a single-file snapshot; `openshell sandbox download` placed it in a private host acquisition root; SQLite `integrity_check` returned `ok`. The completed snapshot was staged at `<root>/.local/share/opencode/opencode.db` for Telltale's existing `opencode.sqlite` source. No new Telltale source/schema is needed, and Event 3.0 remains the current compatibility output. Guest data can be tampered with or incomplete before acquisition; the completed snapshot and retained Telltale output must be stored outside workload write authority. This is not a live-main-file copying contract, a production collection guarantee, or a validated MicroVM path.
