# Architecture

Dungeon is a low-friction environment manager and adoption layer for isolated agent development. It should mostly disappear once an environment is running.

OpenShell owns workload isolation and policy enforcement. Dungeon owns setup, environment configuration, lifecycle, project scope and transfer, reviewed capability requests, authentication entrypoints, and attachment/integration surfaces. Reviewed images own installed tool and agent versions. Agent interfaces and external harnesses own prompts, models, sessions, orchestration, issue workflows, and review strategy. Telltale is not part of the active runtime architecture.

```text
developer / agent CLI / editor / external harness
  -> Dungeon environment interface
  -> explicit resolved configuration + native OpenShell policy
  -> pinned OpenShell adapter
  -> managed isolated environment
       -> agent + tools + project + guest-owned session state
       -> terminal / SSH / editor / future remote attachment
```

The design goal is native-feeling development inside isolation, not a new agent orchestration framework.

## Product responsibility

Dungeon owns:

- supported OpenShell setup and prerequisite diagnostics;
- environment identity and project binding;
- project scope;
- reviewed image/tool requirements;
- explicit read-only configuration imports;
- guest-private writable tool state;
- network/provider/credential requests;
- bounded guest environment values;
- lifecycle;
- command execution and PTY attachment;
- safe project upload and reviewed return of changes;
- machine-readable operations for external harnesses;
- clear unsupported, degraded, and drifted states.

Dungeon does not own:

- model selection;
- prompt construction;
- completion signals;
- iteration loops;
- issue tracking;
- parallel-agent scheduling;
- branch/review orchestration;
- vendor session formats;
- OpenShell enforcement internals;
- Telltale detection semantics.

A feature that belongs primarily to those orchestration concerns should live in the agent/harness layer unless Dungeon needs a small adapter to make the isolated environment usable.

## One implementation

`dungeon` is one Rust binary with plain responsibility modules:

- `cli`: command parsing and dispatch.
- `config`: config/state parsing, validation, strict JSON, and fingerprints.
- `policy`: the starter policy and policy safety validation.
- `openshell`: the single OpenShell subprocess boundary and lifecycle operations.
- `openshell::local_setup`: local gateway bootstrap within the same OpenShell boundary and sanitized executable resolution; OpenShell remains the registration/health authority, Podman the container/network authority.

There is no backend hierarchy, daemon, plugin ABI, embedded JS runtime, second policy language, or framework layer.

This remains the default architectural posture. A new abstraction must earn its existence through a real substitution boundary or repeated semantic responsibility rather than anticipated integrations.

Local setup keeps only private gateway TLS/configuration and persistent database files under the host XDG state directory; OpenShell registration and its mTLS client files stay under OpenShell's config directory. Dungeon does not mirror gateway metadata or select a different active gateway permanently. The rootless Podman socket is mounted only in the trusted gateway container, not in the guest.

The npm tree is distribution only: a tiny Node launcher selects a platform package and executes its Rust binary. It must never parse Dungeon config, apply policy, coordinate auth, or contact OpenShell.

## OpenShell adoption boundary

Dungeon should make supported OpenShell paths easier to adopt without pretending OpenShell is a Dungeon implementation detail.

For supported combinations, Dungeon may:

- detect the OpenShell CLI and supported version;
- detect runtime prerequisites;
- provision and verify Dungeon-owned gateway state when that can be done safely;
- report missing, incompatible, partial, healthy, degraded, or unsupported states;
- provide exact remediation guidance;
- expose the same diagnostics in machine-readable form.

Dungeon must not silently install privileged dependencies, overwrite incompatible OpenShell/runtime state, or fall back to weaker containment.

Setup health is not containment attestation.

## Runtime identity

Initialization does not execute project code or contact a runtime. Creation snapshots the loaded policy, disables automatic provider creation, and labels the sandbox with a configuration fingerprint. Dungeon records sandbox ID and observed-policy digest in host-side `runtime.json`.

Reuse verifies identity and policy drift. These checks are not kernel/image attestation and do not prove provider content stayed unchanged.

The lifecycle remains `up`, `run`, and `down` with one explicit config and one Runtime. In `copy` mode, first `up` creates the bound sandbox and uploads the project; resumed `up` starts it without upload, and is a no-op when it is already Ready. In the explicit pre-validation `live` candidate, creation derives exactly one read-write bind request from the canonical configured project to `/sandbox/project`; first and resumed `up` never upload. `run` starts a stopped bound sandbox when needed without upload in either mode. `down` stops it and retains environment state.

`upload --yes` and `export` remain copy-mode operations. Upload is the operator's overwrite-capable refresh/recovery action. Export is a Ready-only operation for the bound sandbox on OpenShell 0.0.116: it downloads and stages the guest project, then presents a deterministic three-way diff against the last successful upload baseline and current host project. It applies only after interactive typed `yes` or explicit `--yes`. Both commands refuse in live mode before contacting OpenShell.

Low-friction project binding may remove repeated `--config` ceremony, but it must resolve to this same lifecycle and identity model. Convenience is not a parallel runtime path.

## Storage ownership

| Data | Owner |
| --- | --- |
| Host project | User-owned; live read-write workspace mount is the accepted default direction, narrowly guarded; copied upload/export remains the explicit automation/recovery surface |
| Guest project | Backend-owned explicit copy in the copy mode; the live workspace mount replaces it in the live mode |
| Agent sessions | Guest agent-owned |
| Config/runtime binding | Private host directory |
| Export baseline | Private config directory (`baseline.json`) |
| Credentials | OpenShell providers or vendor-owned guest login |
| Project capability requests | Untrusted request input, never authority |

No host dotfiles, editor caches, agent databases, private SSH keys, engine sockets, or credential directories are copied automatically.

## Low-friction adoption

The interactive project selector is a canonical host directory, not a repository
configuration surface:

```sh
dungeon setup
cd project
dungeon
dungeon ../other-project
```

`dungeon [PROJECT]` resolves one deterministic private environment for the
canonical project path and launches its configured default agent, bringing the
environment up first when needed. Subcommands without `--config` resolve the
current directory's same private binding.

There is no mutable project-binding registry and no repository scan. The
private config path is derived from the canonical project path under the user's
Dungeon config root. This keeps one authoritative configuration per binding and
eliminates ambiguous multi-match resolution. A rename or move intentionally
produces a new binding key; the old environment is reachable only through its
explicit config until the operator deliberately creates a new binding.

The architectural requirements are:

- a project resolves one private Dungeon environment without repeated config-path ceremony;
- explicit `--config` remains available and authoritative for automation/recovery;
- project-local material cannot grant security authority;
- first binding is interactive and shows project, transfer mode, and security-relevant scope before files or runtime state are created;
- first creation and security-relevant changes remain reviewable;
- ambiguous or stale bindings fail explicitly;
- the normal user can work through the native agent CLI, shell, VS Code, SSH, or other supported attachment rather than through a Dungeon-specific interaction model.

Dungeon should become less visible as the environment becomes healthier.

## Project and configuration authority

Dungeon keeps one authoritative environment configuration in private user-owned state.

A future repository-local file may describe bounded requests, but project content is untrusted. It cannot authorize itself.

The authority direction is:

```text
operator or organization ceiling
  -> private Dungeon environment configuration
  -> optional project requests
  -> invocation-time narrowing
  -> resolved plan
  -> OpenShell policy enforcement
```

Project requests or guided setup may ask for:

- tool requirements;
- configuration snapshots;
- guest-private writable state;
- network destinations;
- provider/credential requirements;
- bounded guest environment assignments.

They do not grant capability. `policy.json` remains the enforcement authority.

If organization policy is added later, it is a ceiling. Private host configuration, project requests, guided setup, invocation flags, or AI-proposed configuration may narrow or request within that ceiling but cannot weaken it.

## Declarative setup boundary

Reusable editor/tool setup remains data, not a host plugin mechanism. Reviewed OCI images remain the authority for installed binaries and toolchains. Dungeon does not add a second package manager or execute arbitrary host configuration scripts to construct an environment.

Guided commands and noninteractive configuration must resolve through the same model and produce the same capability/data diff.

The accepted request categories are:

- user-selected host files/directories to snapshot into the guest;
- expected binaries already supplied by a reviewed image;
- private guest-writable state/cache locations;
- requested network destinations;
- named credential/provider requirements;
- controlled guest environment variables needed to connect config and state.

Unsupported or unsatisfied requests remain explicit. Missing authority never causes automatic policy widening.

### Snapshot imports

The plan-only slice accepts a reviewed Neovim directory declaration in the authoritative private config and inventories it in `plan`. Runtime commands refuse it: OpenShell 0.0.116 uploads as the sandbox user, so guest-writable `/sandbox` cannot enforce a read-only imported subtree using uploaded file modes. Enforceable guest read-only snapshots remain an implementation gate.

Separately, the live read-write workspace transfer is now implemented as an explicit pre-validation candidate. It derives one bind mount from the canonical private-config project path to `/sandbox/project`; users cannot supply raw driver JSON or arbitrary mount lists. The workspace remains the only requested writable host path, imports remain a separate read-only snapshot lifecycle, and credentials, SSH keys, and engine sockets are never implicitly mounted. The copied upload/export path remains supported. Live does not become the generated/default mode until the OpenShell 0.0.116/rootless-Podman/Fedora validation gate passes.

- The host source path is selected explicitly from private Dungeon configuration.
- Snapshot preflight rejects special files and symlinks for the initial implementation.
- Imported editor configuration is treated as code that may execute inside the guest. Dungeon never sources or evaluates it on the host.
- Host home/config/cache trees, credential stores, browser state, SSH keys, agent databases, and shared caches are never implicitly imported.
- Writable tool state is separate from the read-only snapshot.
- Updates produce a new reviewed snapshot and data/capability diff.

Example boundaries:

| Setup | Image-owned requirements | Host snapshot | Private writable state | Network request | Credential request |
| --- | --- | --- | --- | --- | --- |
| Neovim config | `nvim` | user-selected Neovim config only | editor data/state/cache under guest-owned state | none added automatically; plugin endpoints must already be policy-authorized | none |
| Rust toolchain | `cargo`, `rustc` | none | Cargo/cache/build state inside guest | crates registry endpoints if needed, shown as unsatisfied unless already allowed | none by default; private registry auth uses an explicit provider/credential path |

Development Container Features may be useful as review input for image construction, but their install scripts, mounts, capabilities, and lifecycle hooks are not a Dungeon runtime plugin contract.

## Harness interoperability

Dungeon should expose a small versioned machine interface around environment capabilities it already owns.

The preferred first transport is the native CLI with stable JSON or JSONL where practical. A daemon, SDK, or plugin ABI should not be introduced until a concrete consumer requires one.

External harnesses may need:

- environment resolution/creation;
- lifecycle status/start/stop;
- command execution with faithful exit status;
- output streaming;
- interactive/PTTY execution;
- approved transfer into guest scope;
- reviewed return of guest changes;
- environment identity and capability diagnostics.

External harnesses own prompts, loops, model choices, branch strategies, issue queues, and review orchestration.

Sandcastle is the first concrete compatibility target because its isolated-provider seam closely matches this boundary. Functional compatibility is desirable. Copying Sandcastle's internal orchestration into Dungeon is not.

A Sandcastle adapter should live on the Sandcastle/integration side when possible and may be contributed upstream. It must not require Dungeon to add writable host bind mounts, a host-execution fallback, Node as a native runtime dependency, or automatic project merge that bypasses Dungeon's reviewed return path.

## Project return boundary

The host project remains authoritative. Dungeon does not make the sandbox a writable view of the host project.

The initial copy is uploaded on first `up`; subsequent starts do not silently refresh it. OpenShell uploads may filter ignored files and leave old guest files in place. Export downloads the complete guest tree.

After a successful upload, Dungeon records a common baseline only for pre-upload host entries that exactly match their post-upload guest entry, bound to the config fingerprint and sandbox ID. Differing entries are not asserted as successfully transferred common state. Export refuses to proceed if this binding is absent or mismatched.

Export compares baseline, current host, and downloaded guest paths. Host-only files remain untouched; changes on both sides are conflicts and are refused. The deterministic summary requires explicit approval. Special entries are rejected. Applied state is limited to regular file contents, directories, and the Unix executable bit. Failed apply retains private recovery material.

This is not an atomic transaction or live synchronization path. No writable host mount is introduced merely for harness compatibility.

## Agent image boundary

A reviewed Dungeon image may pin compatible agent/tool versions, but it does not own vendor authentication formats, models, prompts, or session protocols.

The local candidate in [`images/agent`](../images/agent/README.md) implements that version boundary with a digest-pinned OpenShell base, locked agent packages, and hash-verified Neovim/Rust artifacts. Agents are root-owned under `/usr/lib/node_modules/dungeon-agent`; Neovim and Rust are root-owned under `/usr/local`. Their entrypoints resolve under `/usr/local/bin`, within existing policy paths. Building or selecting the image does not change Dungeon policy or import host agent state. Neovim and Cargo use guest-private HOME-derived state under `/sandbox`; OpenShell does not propagate OCI image environment variables to executed commands by default.

## Optional Telltale boundary

There is no active Telltale command, dependency, collector, event model, or persistent collection state in Dungeon.

Telltale integration remains deferred until Telltale 0.7.0 is released with Event4 as the intended external telemetry contract and the relevant downstream boundaries are stable.

The long-term product direction is optional composition, not mandatory coupling. Teams should be able to use Dungeon without Telltale, and future Dungeon/Telltale integration should not make Telltale an isolation dependency.

Dungeon must not bind to transitional Event 3, NormalizedRecord, Canonical Observation v2 migration details, or current collector internals.

Issue #1 proved that a post-exit OpenCode SQLite snapshot can be transferred outside workload write authority on one Podman combination. That result is research, not an active Dungeon architecture contract.
