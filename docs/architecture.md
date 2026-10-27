# Architecture

Dungeon is a low-friction environment manager and adoption layer for isolated agent development. It should mostly disappear once an environment is running.

OpenShell optionally owns workload policy enforcement. Plain containers use the
selected engine's standard isolation without OpenShell policy. Dungeon owns setup,
environment configuration, lifecycle, project scope and transfer, reviewed capability
requests, authentication entrypoints, and attachment/integration surfaces. Reviewed
images own installed tool and agent versions. Agent interfaces and external harnesses
own prompts, models, sessions, orchestration, issue workflows, and review strategy.
Telltale is not part of the active runtime architecture.

```text
developer / agent CLI / editor / external harness
  -> Dungeon environment interface
   -> explicit resolved configuration
   -> plain Podman/Docker container OR pinned OpenShell adapter/policy
  -> managed isolated environment
       -> agent + tools + project + guest-owned session state
       -> terminal / SSH / editor / future remote attachment
```

The design goal is native-feeling development inside isolation, not a new agent orchestration framework.

Remote-first means the familiar UI/editor can remain on the host while the
agent server, tools, credentials, and sessions run inside the guest. Live local
project transfer lets the host editor see guest writes immediately without
importing its configuration. This does not imply file synchronization across
machines or support for every vendor's desktop/SSH protocol.

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
- `machine`: versioned request parsing, selection and fixed protocol serialization;
  dispatches the same Runtime, not another backend.
- `output`: concrete human/quiet operation output and structured export review.
- `config`: config/state parsing, validation, strict JSON, and fingerprints.
- `policy`: the starter policy and policy safety validation.
- `runtime`: concrete dispatch between the two recorded contracts, shared by human
  and machine entrypoints; OpenShell-only operations refuse on plain containers.
- `container`: local engine discovery, direct lifecycle, inspected native identity
  and mount scope. It reuses the bounded native-query mechanism from setup.
- `openshell`: the single OpenShell subprocess boundary and lifecycle operations.
- `openshell::local_setup`: local gateway bootstrap, authoritative generated copy/live capability modes, shared bounded native queries and sanitized executable resolution within the same OpenShell boundary; OpenShell remains the registration/health authority, Podman the container/network authority.
- `openshell::diagnostics`: read-only prerequisite/capability reporting with bounded native queries, sharing setup predicates without invoking setup.
- `openshell::local_setup::removal`: explicit local uninstall; reuses setup's exact
  gateway ownership/native query predicates and Runtime's identity-checked
  stop/destroy. OpenShell owns registration and guest-resource deletion; Podman
  owns container/network absence. Private state and the binary are removed last.

There is no backend trait hierarchy, daemon, plugin ABI, embedded JS runtime,
second policy language, or framework layer. Two concrete runtimes now have an
independently useful contract: plain containers do not require OpenShell.

### Plain-container selection and lifetime

New bindings choose installed OpenShell or plain containers, unless explicitly
selected with `--runtime` or saved container defaults. Existing schema-2 configs
without `container` remain OpenShell with unchanged serialization/fingerprints.
`container` holds the requested engine and optional local Unix endpoint. First
creation discovers usable services (Podman first), then records engine, endpoint,
daemon identity and exact container ID in `runtime.json`; reuse never rediscovers.
`setup --runtime container` saves the resolved engine/socket for future projects
in private `container-defaults.json`. `init` overrides and private config remain
authoritative. No remote Docker contexts, engine environment overrides, or
repository-local settings are adopted.

Plain containers support live projects, guest CLI/exec/shell/auth, stop/resume,
status, destroy and standalone uninstall. Copy/export, providers, SSH/forwarding,
host UI attachment and owned/machine-stream execution require OpenShell. Missing
dependencies are reported at feature use; offline config/list/plan and explicit
binary-only uninstall need neither runtime. Doctor uses descriptor-checked config
selection for both contracts and does not initialize engine storage.

Creation uses a deterministic labeled container name, explicit non-root user,
bridge networking, fixed entrypoint, and only derived project/import binds. A
pending record precedes guest HOME preparation, so interruption blocks execution
but permits identity-checked cleanup. Configuration/mount drift blocks reuse.
The plain state's `observed_policy` field stores the stable native specification
digest, not an enforced OpenShell policy. Generated Podman metadata and consumed
SELinux relabel options are excluded; inspected bind sources/targets/modes remain
authoritative. Native engines retain writable guest state between stop/start.

The OpenShell-specific sections below describe that optional contract.

This remains the default architectural posture. A new abstraction must earn its existence through a real substitution boundary or repeated semantic responsibility rather than anticipated integrations.

Local setup keeps only private gateway TLS/configuration, persistent database files, and OpenShell driver data under the host XDG state directory; OpenShell registration and its mTLS client files stay under OpenShell's config directory. Because the containerized gateway controls host Podman through its API socket, driver data that OpenShell may later use as a host bind source must have path identity across that boundary. Dungeon therefore mounts its private `driver-data` directory at the same absolute host/container path and sets the gateway's `XDG_DATA_HOME` to it. That directory remains trusted gateway state, not sandbox or project authority. Dungeon does not mirror gateway metadata or select a different active gateway permanently. The rootless Podman socket is mounted only in the trusted gateway container, not in the guest.

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

Fresh plain `setup` creates a bind-capable gateway; `--copy-only` explicitly
chooses a gateway without project bind capability. Neither is project mount
authority. Plain setup verifies an exact existing live mode without changing it;
an exact existing copy-only gateway requires `--copy-only` verification. Migration is
blocked; setup never restarts an existing gateway. Fresh creators serialize with
a private setup file lock, not a runtime daemon or cross-client maintenance lease.
Dry-run creates neither state nor lock and queries only established default
rootless storage. Failed/interrupted creation can leave partial state requiring
inspection. Native validation of this CLI has a separate disposable Fedora CoreOS
record; it is not inferred from the manually provisioned live gateway.

`doctor` is a read-only diagnostic responsibility within the same OpenShell
subprocess owner, not another runtime implementation. It shares setup's pinned
version, rootless, registration and health predicates but never calls setup or
constructs Runtime. Checks remain independent when a prerequisite is missing.
Only native inventory queries run, with null stdin, a narrowed host environment,
project-excluded executable resolution, five-second deadlines and capped output.
Doctor's descriptor-checked config/policy bytes reuse the existing config schema
and policy validator; ordinary runtime loading retains its existing contract.
The diagnostic subprocess PATH excludes all selected/current project directories
for helper resolution too. Native engine inventory requires an established
store, and query leader identities remain reserved until group teardown/reap.
Foreign gateways are not adopted or inferred from local gateway files. No
credential/auth inspection or dynamic integration/containment testing occurs.

Service attachment reuses OpenShell's foreground gRPC forwarding, not a new
Dungeon server or proxy. `forward` checks the existing Ready binding and policy
before opening host loopback to a guest-loopback service. It neither starts the
service nor manages its authentication. For the reviewed OpenCode path, a
foreground explicit `dungeon attach` starts the guest service, holds its generated password only in memory,
forwards guest loopback to an available host-loopback port, verifies 401 without
authentication and matching authenticated server version, then runs the native
host UI. On exit it sends Ctrl-C through the service PTY and checks shutdown
before closing the forward. If cleanup is unconfirmed, a simple prompt offers
an explicitly confirmed sandbox stop or leaves it running; no automatic
gateway/project deletion. `run opencode` remains the guest TUI. There is no
daemon, password store, or cross-vendor remote abstraction.

Explicit `exec --owned` owns only one foreground guest process group. It uses
fixed guest Node code with a live group guardian and an outer status/cleanup
owner, through the same OpenShell subprocess boundary. A random temporary
invocation control directory is not a service/PID registry. The host handles
INT/TERM and verifies cleanup separately from command output; ordinary exec,
run, and forwarding remain native and unchanged. This is lifecycle assistance,
not protection against group escape, same-UID sabotage or lost transport.

`ssh --owned` is the distinct real-terminal path within that same ownership
boundary. Native OpenSSH/OpenShell own PTY allocation, raw input and resize;
Dungeon does not proxy terminal bytes. The host hands foreground access to its
private unreaped SSH leader and restores its original group/termios afterward.
The guest guardian remains the native session/foreground-group leader; a detached
silent observer owns exclusive invocation controls and a strict nullable-status
receipt. Only the live guardian signals its own group. The observer observes group
absence and never signals a reaped leader. Handshakes are bounded, transport faults
are sticky, and receipts cannot override primary transport or restoration errors.
The responsibility seam is `owned_ssh` under `owned`, not a second backend.

Machine execution adds `stream` under that same owned boundary: a bounded strict
frame parser and nonblocking Linux FIFO/Unix-stream-socket sink in the existing cleanup loop. The
guest outer status owner frames dedicated application descriptors outside the
killed group. It never frames wrapper/native diagnostics. Ordinary machine
lifecycle/export use quiet operation output, not global fd redirection or parsing
captured human output. [Schema 1](machine.md) owns the wire contract; remote
execution and selective Sandcastle transfer are not implemented.

## Runtime identity

Initialization does not execute project code or contact a runtime. Creation snapshots the loaded policy, disables automatic provider creation, and labels the sandbox with a configuration fingerprint. Dungeon records sandbox ID and observed-policy digest in host-side `runtime.json`.

Reuse verifies identity and policy drift. These checks are not kernel/image attestation and do not prove provider content stayed unchanged.

The lifecycle remains `up`, `run`, and `down` with one explicit config and one Runtime. New CLI bindings select `live` after operator review: creation derives exactly one read-write bind request from the canonical configured project to `/sandbox/project`; first and resumed `up` never upload. Explicit `copy` mode creates the bound sandbox and uploads the project once; resumed `up` starts it without upload, and is a no-op when already Ready. Existing configs omitting the transfer field retain copy semantics. `run` starts a stopped bound sandbox without upload in either mode. `down` stops it and retains environment state.

`upload --yes` and `export` remain copy-mode operations. Upload is the operator's overwrite-capable refresh/recovery action. Export is a Ready-only operation for the bound sandbox on OpenShell 0.0.116: it downloads and stages the guest project, then presents a deterministic three-way diff against the last successful upload baseline and current host project. It applies only after interactive typed `yes` or explicit `--yes`. Both commands refuse in live mode before contacting OpenShell.

Low-friction project binding may remove repeated `--config` ceremony, but it must resolve to this same lifecycle and identity model. Convenience is not a parallel runtime path.

## Storage ownership

| Data | Owner |
| --- | --- |
| Host project | User-owned; new CLI bindings default to a reviewed single-project live mount; explicit/legacy copy uses upload/export |
| Guest project | Backend-owned explicit copy in the copy mode; the live workspace mount replaces it in the live mode |
| Agent sessions | Guest agent-owned |
| Config/runtime binding | Private host directory |
| Export baseline | Private config directory (`baseline.json`) |
| Approved Neovim snapshot | Private config `imports/<digest>/nvim`; read-only guest bind, original source separate |
| Credentials | OpenShell providers or vendor-owned guest login; reviewed OpenCode config/legacy auth can additionally expose host credentials |
| Project capability requests | Untrusted request input, never authority |

No host home, editor caches, agent databases, private SSH keys, engine sockets,
or whole credential directories are copied automatically. Explicit OpenCode
imports propose narrowly selected config/legacy auth access after scope review;
old bindings and machine initialization do not inherit it.

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

`init` separates configuration review from runtime creation: project, name and
private directory have deterministic defaults, with explicit flags for overrides.
The human CLI first asks for a harness if none was explicitly selected, then
asks once before saving scope. No harness is assumed or inferred from host tools.
The choice is stored in the project binding, without a global preference registry.
`init --yes --agent AGENT` is explicit noninteractive approval and selection.
Host harness imports are disabled by default; read-only/copy access is opt-in.
Human explicit and implicit initialization share one configuration-default
constructor; CLI options are overrides rather than a second set of defaults.
The same config writer and validator serve direct binding and machine
initialization. `config` exposes resolved values/paths without a new registry;
`config` and `plan` never initialize missing bindings or contact OpenShell.

Bare `dungeon` is interactive-only for every harness and reuses `run`'s guest-CLI
launch path, with no host harness client requirement. Explicit `run` accepts
noninteractive native arguments and invocation-only harness overrides. Host
OpenCode attachment remains an explicit, vendor-specific `attach` operation.

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
- first binding shows project, transfer mode, and security-relevant scope before saving, and requires interactive confirmation or explicit initialization approval; implicit direct-launch binding remains interactive;
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

## Shared sign-in direction (development, not an existing capability)

Sign-in should be user/account-scoped rather than owned by a project sandbox:
one enrollment across compatible projects, persistent across `down`/`up` and
project deletion. A single credential owner should serialize refresh and persist
rotated grants. Project guests must not receive duplicated refresh tokens or a
shared writable native authentication directory. Account selection is separate
from imported themes/plugins/settings, which may themselves contain secrets.

Prefer OpenShell's existing gateway provider store, refresh and endpoint-bound
credential substitution over another Dungeon daemon. The pinned gateway requires
inspected credentialed endpoints: adopting it needs an explicit network/inspection
contract, not silent changes to the current pass-through starter policy. Generic
OAuth support is not proof of native subscription compatibility. OpenCode OAuth
can select different endpoints/headers/models from its API-key path; each native
integration requires separate validation.

After one enrollment review, intended ordinary use is bare `dungeon` in another
directory without a second login. New bindings should pin a non-secret account
identity, not follow mutable defaults or credential-generation hashes. Secret
rotation must not rebuild sandboxes; account changes must not retarget existing
ones silently. Stopping a sandbox stops execution, not authorization; sign-out
and project deletion have different scopes. Revocation cannot retract completed
requests or promise that every already-open stream is terminated.

Before automatic grants ship, enforceable restrictions must cover human/machine
launches and reuse as well as creation: explicit account selection, project-local
or disabled auth, and organization restrictions that users/project configuration
cannot widen. No current organization ceiling is claimed. Opaque credentials
limit extraction, not account abuse or spend through allowed endpoints. Keep
`--no-auto-providers` until native compatibility and these authority gates pass.

The current implementation remains per-project guest login or explicit named
OpenShell providers. `scripts/probe-opencode-identity.cjs` is a synthetic native
harness compatibility gate only, not a credential broker or shared login.

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

The OpenCode harness slice extends the same private config, fingerprint and import
lock, with file selection/capture in `imports::harness` (`src/harness.rs`). It does
not generalize Neovim's arbitrary source selection into a raw mount API. Native
OpenCode config/plugin/theme leaves and optional legacy `auth.json` become
individual read-only binds; original live files are never relabeled, private
copies may be. Exclusions precede file opens. Database auth/session state and
writable caches remain guest-private. `reload` only reselects unbound inputs;
active refresh and write-required authentication are unresolved, not automatic
credential synchronization. See the [bounded contract](configuration.md#harness-configuration-opencode-first-slice).

The first native gate found inaccessible original sources and missing guest-owned
parents. New work now refuses nonempty original-source imports on enforcing
SELinux hosts before runtime contact, without custom-label exceptions or an
automatic copy fallback. The image recipe creates empty UID998 mode0700 native
parents after tool-verification cleanup and separately verifies that final layout.
Existing images/bindings are not repaired. The rebuilt test image passed native
copy config/plugin/state/resume checks, but did not establish legacy auth reuse; see
[validation](validation.md#authorized-native-gate-2026-10-22-blocked).

The Neovim slice accepts zero or one explicit directory declaration in the authoritative private config. `imports stage` captures bounded descriptor-checked files, reviews the captured hashes/data diff and selects an approved digest. The manifest is an artifact, not another configuration registry. Creation derives one read-only bind from the private digest-addressed snapshot, never from its original source. Copy project upload and optional live project transfer remain separate. Imports require the exact existing Dungeon-owned local bind-enabled UID998 gateway; setup predicates, not the weaker aggregate diagnostic report, authorize creation.

The contract protects mounted **contents**, not their guest pathname. Native proof observed that UID998 could rename `/sandbox/.config` and create a replacement `/sandbox/.config/nvim`. The mounted original bytes stayed read-only. Dungeon does not guarantee which configuration subsequent Neovim invocations execute and does not mount the parent or change policy to prevent redirection.

The selected digest enters existing config/runtime identity; source contents never enter identity dynamically. `plan` observes drift without refreshing. Staging refuses any existing runtime binding, including stopped or invalid state; refreshing an active environment requires a separately initialized environment. Stage/create share a narrow native file lock and reread authority. This is not general lifecycle serialization or crash/power-loss durability. Missing/tampered artifacts block work launch, not identity/policy checks used for status, stop and owned cleanup.

Separately, live read-write workspace transfer derives one bind mount from the canonical private-config project path to `/sandbox/project`; users cannot supply raw driver JSON or arbitrary mount lists. New CLI bindings request it only after operator review; legacy omitted fields stay copy. The workspace remains the only requested writable host path, imports remain a separate read-only snapshot lifecycle, and credentials, SSH keys, and engine sockets are never implicitly mounted. The copied upload/export path remains supported. Validation is limited to the recorded OpenShell 0.0.116/rootless-Podman/Fedora combinations; default promotion does not broaden that evidence.

- The host source path is selected explicitly from private Dungeon configuration.
- Snapshot preflight rejects special files, symlinks, hardlinks, unsafe ownership/permissions and known credential/native-state names; limits bound depth, entries and bytes.
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

A Sandcastle adapter should live on the Sandcastle/integration side when possible and may be contributed upstream. It must not require arbitrary additional writable host binds, a host-execution fallback, Node as a native runtime dependency, or automatic project merge that bypasses Dungeon's reviewed copy-mode return path. Explicit live mode remains limited to the canonical configured project bind.

## Project return boundary

The host project remains authoritative. In explicit live mode, the canonical
project is a writable guest view. The staged return contract below applies only
to copy mode.

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
