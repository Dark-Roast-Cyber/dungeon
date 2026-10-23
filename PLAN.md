# Remote-first work plan

Goal: host-native clients/editors, isolated agent execution, and visible local
project edits without weakening the boundary. Validation does not authorize
tool upgrades, client installation, release/tagging, or publication.

## Progress and evidence

Validated on the recorded Fedora/OpenShell 0.0.116/rootless-Podman combination
(durable summary: [validation](docs/validation.md#remote-first-service-forwarding)):

- Native host OpenCode remote AI edit and guest/host tests; live ownership/modes
  preserved. Forward interrupt/reconnect and stopped-sandbox refusal passed.
- AI edit -> same-sandbox restart -> same native session resume passed, including
  exact history persistence and recovery of a conversation-only marker.
- Configuration-drift refusal and bounded filesystem/egress/socket probes passed.
- Real terminal Ctrl-C through `exec --tty` stopped the guest server. Non-TTY
  host process-group interruption left it running; explicit guest TERM worked.

The earlier host 2.0.22/guest 2.0.14 evidence remains historical. A separate
private extracted host 2.0.14/guest 2.0.14 pair now passed the Issue #29 native
AI/session/lifecycle gate; the installed host client was unchanged.
Native TUI attachment passed previously, but interactive TUI message submission remains unverified.
Neovim imports have a delivered staged snapshot implementation with a contents-only read-only bind contract (PR #34, Issue #5 closed); native copy/live gates passed on the recorded disposable VM. Telltale is not integrated.

## Tracking

Use GitHub issues for durable status, decisions, blockers and acceptance criteria.
Publish only reviewed, sanitized summaries; raw transcripts, authentication
material, client sessions and runtime state stay in private local scratch.
Gitignore is not a security control. Do not retain credentials as test evidence.
This existing plan is the local preparation/handoff document, not a second issue
tracker. Published work items:

- [#28: guest-service shutdown](https://github.com/Dark-Roast-Cyber/dungeon/issues/28)
- [#29: matched OpenCode pair](https://github.com/Dark-Roast-Cyber/dungeon/issues/29)
- [#30: native remote validation](https://github.com/Dark-Roast-Cyber/dungeon/issues/30)

No duplicate tracking issues were created; use the named #28–#30 issues for
these gates.
Link closed image Issue #6 and live-validation Issue #1 as background; leave
project adoption #18 and live-mode promotion #24 scoped to their existing work.
Reuse [#11](https://github.com/Dark-Roast-Cyber/dungeon/issues/11) for authenticated
Claude Code CLI testing, not as proof of remote/desktop support.

## 1. Explicit guest-service shutdown

Status: implementation, disposable local live gates and independent review
accepted, including the test-portability correction at `3965d7d34e6e0ee28483fc0e68ebc708ef880c48`;
all seven jobs in [CI run 37113253413](https://github.com/Dark-Roast-Cyber/dungeon/actions/runs/37113253413)
passed. Issue #28: **Make remote guest-service
shutdown explicit without orphaned processes**.

The opt-in `exec --owned` implementation leaves generic exec/run/forward alone.
It uses existing guest Node primitives, a live group guardian, private IPC and
ephemeral nonblocking/no-follow control files, not user PID authority. Live
signal/concurrency/forward/relaunch and bounded unconfirmed-failure results are
recorded in [validation](docs/validation.md#issue-28-owned-foreground-command).
Matched native client/session resume remains #29, not a claim of this slice.

- Reproduce non-TTY interruption at the Dungeon/OpenShell boundary and determine
  what the native transport and vendor service support. Do not change generic
  exec semantics based only on one service's behavior.
- Prefer native supported shutdown or an explicitly owned foreground lifecycle.
  The validated TTY Ctrl-C path is the baseline, not proof of automatic cleanup.
- Acceptance: interrupt and normal exit stop only the owned guest service, leave
  no owned host/guest listener or foreground process, preserve session/project
  state, and allow clean relaunch. Forward interruption alone must continue to
  leave the independently started server alive.
- Use one OpenShell boundary. No broad process killing, daemon, persistent PID
  registry, password store, credential copier or policy changes.

## 2. Reviewed matched OpenCode client/server

Status: disposable local matched 2.0.14 gate PASS and independent review accepted.
No host/guest upgrade or image rebuild performed.
See [matched-pair evidence](docs/validation.md#issue-29-matched-opencode-pair-2026-10-03).
Issue #29: **Validate a reviewed
matched OpenCode remote client/server pair**; link image Issue #6.

- Choose a reviewed common version and record artifact provenance/hashes plus
  the guest image digest. Do not silently replace the user's host installation
  or rebuild/select an image during an existing validation record.
- Acceptance: exact version match, native authentication, remote guest location,
  actual AI edit/test, same-session restart/resume, and shutdown/forwarding gates
  from item 1. Keep version drift explicit rather than claiming all pairs work.
- Keep `OPENCODE_PASSWORD` ephemeral and client-only; no positional guest path
  passed to a host client, authentication bypass or host state import.

## 3. Separate editor/vendor remote gates

Status: prerequisite checkpoint complete; VS Code Remote SSH NOT TESTABLE
(client/extension/display unavailable), no installation performed. See
[editor checkpoint](docs/validation.md#issue-30-editor-prerequisite-checkpoint-2026-10-03).
The separate [native SSH/guest Neovim gate](docs/validation.md#issue-30-native-ssh-terminal-editor-2026-10-03)
passed real-PTY render/edit/save/test and saved-state reconnect/restart, with
degraded disconnect cleanup (editor processes survived until the dedicated
sandbox was stopped). This is partial #30 evidence, not VS Code, host Neovim UI,
vendor desktop support or full issue acceptance. Validate one integration at a time. GitHub
issue #30: **Validate native remote clients against Dungeon-backed environments**.

- **VS Code Remote:** repeated PATH/RPM/Flatpak and standard-directory inventory
  found no code/code-insiders/codium or extension installation; no GUI display
  was available in this session. GUI/extension prerequisites and explicit installation approval are
  needed. Verify workspace, remote tool execution, live edits and disconnect/
  reconnect. SSH-config output alone is not a pass.
- **Codex:** host 0.155.1 and guest 0.156.1 advertise app-server/remote endpoints.
  Verify the actual vendor transport and native service authentication; do not
  assume SSH or reuse OpenCode's protocol. A model task needs authorized guest
  authentication, without copying host credentials or bypassing the inner sandbox.
- **Claude:** host 2.1.277 and guest 2.1.267; distinguish CLI remote-control/cloud
  sessions from desktop products. Verify exact supported mode, account and
  platform prerequisites. #11 remains the account-dependent CLI test gate.
- **Desktop clients:** test only an available vendor-supported client/platform;
  otherwise record the missing prerequisite, not an inferred containment pass.
- Per integration: freeze versions, exercise one synthetic edit/test, establish
  guest execution/state ownership, check restart/reconnect/interrupt and expected
  denials, then clean up disposable resources. No host-home/auth sharing or policy
  widening. Help/version checks are inventory, not integration validation.

## Read-only diagnostics (Issue #22)

Issue #22 adds `doctor`/`doctor --json` read-only diagnostics; independent review
and safety rereview were accepted at `3965d7d34e6e0ee28483fc0e68ebc708ef880c48`
and delivered in merged PR #27. Fresh setup improvements followed in PR #33;
the broader adoption issue remains open. Scope is Linux/OpenShell 0.0.116,
existing rootless Podman and selected existing private config only. Independent
checks, bounded sanitized native queries, explicit blocking exit semantics and
configuration-only live capability/mapping observations do not install/migrate,
promote live, implement imports or dynamically verify recorded integrations.
Unknown foreign gateways/custom engine storage remain explicit, not adopted.
Independent-review safety corrections cover descriptor-relative private
config/policy reads, all-project helper PATH exclusion, exact driver-data
environment identity, WNOWAIT query cleanup and empty-store initialization
avoidance. Focused regressions and native scratch evidence are recorded;
accepted rereview does not establish new runtime/editor containment claims.

## Delivery and remaining work

1. SSH cleanup: `ssh --owned` implementation, accepted review and disposable native PTY gates
   are delivered on main. Generic SSH
   disconnect remains degraded and VS Code prerequisites remain unavailable.
2. PR #26 and PR #27 are merged with ancestry preserved; #28/#29 are closed after
   their recorded acceptance and delivery gates. Partial SSH evidence
   does not close the broader #30 integration gate.
3. #24: experimental fresh `setup --enable-live` and read-only `--dry-run`
   are implemented with exact copy/live verification and no downgrade. Copy-to-live
   migration refuses pending maintenance/concurrency and recovery validation.
   Static gates and native empty-HOME refusal passed. A separate disposable
   Fedora CoreOS 44/Podman 5.8.4/OpenShell 0.0.116 VM then passed actual fresh
   copy/live CLI setup, repeats/no downgrade, blocked migration preservation,
   doctor capability separation and copy/live synthetic lifecycle/ownership
   checks. Evidence: `/tmp/opencode/dungeon-setup24-vm-Mtue76/evidence`;
   [exact gate](docs/validation.md#issue-24-experimental-fresh-setup).
   Native JSON failures now return fixed context, with duplicate secret-key/ANSI
   leakage regression coverage. Accepted rereview is delivered in merged PR #33.
   The safety decision is to retain generated/default transfer `copy`; automatic
   live promotion is deferred until safe recognized migration/exclusion/recovery
   and adoption gates are met. This records a default decision, not user
   authorization to promote live mode or full #24 acceptance.
4. #5: staged Neovim snapshots use read-only binds, not uploaded modes. Actual stage/create in copy/live modes passed native content protection, drift/resume, private state, exact mounts and artifact-independent owned cleanup. Ancestor rename/replacement is an observed limitation; accepted scope is immutable mounted contents, not guest pathname or subsequent execution identity. See the [record](docs/validation.md#issue-5-staged-contents-only-snapshots-2026-10-04-utc). Final lock-release correction has deterministic fork/default-parallel coverage; the native matrix predates only that correction. Accepted rereview is delivered in merged PR #34; #5 is closed.
5. #16 minimal operation locking/copy-transfer correctness is delivered in
   merged PR #35; #16 is closed. All seven jobs in
   [CI run 37173607186](https://github.com/Dark-Roast-Cyber/dungeon/actions/runs/37173607186)
   passed on main `cdffe9d42e3747efe7a018fa46289f2b68a4b7dd`.
   Native macOS builds do not establish macOS transfer durability.
6. #20 schema-1 machine interface is delivered in merged
   [PR #37](https://github.com/Dark-Roast-Cyber/dungeon/pull/37); #20 is closed.
   All seven jobs passed on feature `8f5844efb6452ef1b148b484fdea0981c30c881b`
   ([CI 37197184382](https://github.com/Dark-Roast-Cyber/dungeon/actions/runs/37197184382))
   and main `1ce88a7a82795c62183355f4cbb9a95c41d457f9`
   ([CI 37197297984](https://github.com/Dark-Roast-Cyber/dungeon/actions/runs/37197297984)).
   Shared Runtime lifecycle JSON, bounded workload-byte JSONL, finite unowned
   stdin and existing owned raw interaction are implemented; revision-qualified
   native validation is recorded in [docs/validation.md](docs/validation.md#issue-20-machine-namespace-delivered-2026-10-04-utc).
   #21 remains open and unimplemented: whole-project reviewed transfers do not
   fit unchanged arbitrary-path copy/automatic sync. Read-only source analysis of
   Sandcastle `62307ab65ad9f8414f7a4893d96328a0353b0c3f` found pinned-layout blockers:
   [head creation](https://github.com/mattpocock/sandcastle/blob/62307ab65ad9f8414f7a4893d96328a0353b0c3f/src/SandboxFactory.ts#L525-L558)
   and [startup mounts](https://github.com/mattpocock/sandcastle/blob/62307ab65ad9f8414f7a4893d96328a0353b0c3f/src/startSandbox.ts#L135-L155)
   require project -> `/home/agent/workspace` and `.git` -> its absolute host path,
   unlike Dungeon's single canonical project -> `/sandbox/project`. A normal
   in-project `.git` needs no outside authority; the mismatch is the requested
   guest layout, not an inherent Git restriction. Isolated startup always uses
   git-bundle arbitrary-path copies; automatic host patch apply bypasses reviewed export.
   Per-command cancellation is not propagated; grace expiry may leave execution
   overlapping commit collection.
   full-run release calls `close` on failure and could signal/drain an owned child.
   No adapter or live experiment exists. Next is a bounded external design with
   provider-owned workspace mapping, unnecessary `.git` alias suppression and
   per-command cancellation semantics. Upstream implementation/branch strategy
   needs separate authorization, not blanket external-adapter scope. Copy stays
   default; explicit experimental live mode does not require automatic promotion.

### Issue #16 copy-transfer correctness (delivered)

Environment and canonical-project native locks cover upload/export end to end,
including different config paths under one HOME/XDG authority. Project locks
have separate private storage, never synthetic project bindings. Prepared
export recovery survives panic/signals; unfinished recovery blocks upload as
well as export. Unknown/legacy candidates require conservative manual inspection.
Baseline replacement/persistence failure is explicitly uncertain after host
apply, never described as an unchanged baseline. This is not general lifecycle
locking, an atomic transaction or power-loss recovery. Nested overlapping
projects, different authorities, project moves and external writers remain
excluded. Independent review and delivery are accepted in merged PR #35.

Owned interactive SSH now has live implementation evidence for native editor
input/resize/save/test, host signals, terminal/SSH loss, STOP/CONT, isolation and
bounded gateway/drift/guardian faults. See
[the recorded gate](docs/validation.md#owned-interactive-ssh-implementation).
This does not promote live mode, install clients or authorize publication.

## Harness configuration access (2026-10-21)

Status: development approved, in priority order **OpenCode > Codex > Claude Code**.
The OpenCode source-only first slice implements selected read-only file access,
copy/disabled modes, exclusions and unbound `reload`; formatting, all-target
Clippy, 177 locked Rust tests and three npm launcher tests passed on the combined
working tree. Source review found no remaining source-level blocker after the
mount-budget and Neovim-staging corrections, but the subsequently approved native
gate is **blocked**. Those initial gates did not authorize real host configuration/
credential reads, existing-host sandbox changes, installation or publication;
the later scoped publication grant is recorded below.

2026-10-22: user separately approved disposable synthetic native validation.
The completed QEMU/KVM fixture had its own Fedora CoreOS 44.20260913.3.2,
Podman 5.8.4 store, loopback namespace, HOME and fresh gateway, with Enforcing
SELinux and OpenShell 0.0.116. The imported reviewed OpenCode image ID was
`bd446d0373bcd22b6f3d5323944b268c9ab09d7bbd7dfc4ef085f25a975cc142`.
Release binary SHA-256:
`a7e4628990178e8a5e9cb5a6b3c6794125632363c5c794509a6826524be24d79`.
No real host configuration/authentication was an input. All four sandboxes,
the fixture gateway/network, QEMU child, SSH listener, synthetic keys/disks and
exported image archive are removed. The protected real host gateway/project
container IDs, states and start times matched before/after; source hashes still
match. Sanitized asserted evidence and cleanup proof:
`/tmp/opencode/dungeon-harness-native-evidence-20261022`.
Gate scripts/logs remain `/tmp/opencode/dungeon-harness-native-*`. No broader
policy, production image/default, host installation or publication was changed.
Doctor initially refused HOME-as-cwd roots; from the fixture project it was
healthy despite the native application failure. Truncated streamed-stdin fixture
scripts were corrected to uploaded files with null stdin, not counted as passes.

Native findings and decisions before continuing:

1. Reviewed image + leaf mounts create root-owned guest parents and prevent
   OpenCode log/state creation. A separate offline prototype with empty native
   config/data/state/cache directories owned by UID998 mode0700 enabled copy-mode
   config loading, an active local plugin, private writable state and same-ID
   resume. It is not amended-image validation. The recipe now prepares empty
   native directories/parents after verification cleanup and runs a separate
   final-layout verifier; no privileged runtime repair or host-parent mount.
2. Original-source reads failed with ordinary home labels under Enforcing SELinux;
   copies were readable without relabeling originals. Keep original-source
   relabeling and automatic fallback prohibited. Following user continuation,
   preserve the approved original-source default and add conservative early
   refusal on enforcing/unknown SELinux state for nonempty selections. Explicit
   copy/disabled remain available; no custom-label exception or snapshot-default
   substitution. Read-only selection/reload stays offline; new-work admission
   checks compatibility. Status/stop/owned cleanup remain independent.
3. Fresh 2.1.2 provider credential reuse was not established: a readable synthetic
   legacy auth file still produced an empty native credential list after model
   readiness. Current host auth/session databases remain unshared. A bounded
   native credential-import contract needs separate review; neither file exposure
   nor the server API's successful password authentication proves provider auth.
   Actual OAuth refresh/model inference/theme behavior remain untested.

Independent read-only native findings review: `ses_e9b4a23c9ffeE2hJILdlCMIABK`
(completed; no edits/runtime operations). See docs/validation.md for exact limits.

Correction continuation: source checks pass (179 Rust, three launcher, three
layout tests, fmt, all-target Clippy). Incremental read-only review
`ses_e9a419c6bffedn8NwOAptXzDwH` found no source defect. Corrected recipe built
distinct test image `b2ac1628e85d8757d932e1fb9c72e66d9d7608df3f6db9ae77fd08b81d2f3005`;
release SHA `8a7ea52ba868264b7777d02ab231d73796313d9acede90bf335b5c44d9107ef4`.
Fresh synthetic native revalidation **passed the incremental correction gate**
on the same FCOS/Podman/OpenShell/Enforcing combination. The actual corrected
image (not the prototype) passed final layout/tools checks, original-source
human/machine pre-runtime refusal, explicit copy config/plugin execution,
readable read-only leaves, guest-owned writable roots, private-state same-ID
resume, bounded denial and bound reload refusal. Original synthetic sources and
labels stayed unchanged. Provider credentials remained an empty native list.
The fixture sandbox/gateway/network/VM/listener/keys/disks/archive were removed;
protected host IDs/states/start times matched, and runtime/image/test source
hashes rechecked. The Linux CI quality job now runs the layout regressions.
Sanitized evidence: `/tmp/opencode/dungeon-harness-correction-evidence-20261022`;
scripts/logs/source hashes: `/tmp/opencode/dungeon-harness-correction-*`.
No ordinary image-tag promotion, host installation, real host config/credential
reads or existing-service change occurred. The user subsequently authorized
commit, push, merge and task cleanup; publish only this harness slice, preserving
unrelated machine/socket work and retained native evidence. No image release,
host installation or existing-service cleanup is included in that authorization.
Next unresolved boundary: separately review a bounded native provider-auth
contract without host DB/session copying or real secret access; original-source
non-enforcing runtime, OAuth refresh/inference/UI themes remain unvalidated.
Do not start Codex/Claude slices by pretending these OpenCode gates passed.

Publication gate: an index-exported snapshot excludes unrelated machine/socket
edits and passes fmt, all-target locked Clippy, 175 locked Rust tests (128+47),
and all 20 Node tests. Native results remain tied to their combined-tree hashes;
no new native claim follows from the staged-only source run. Use a feature PR,
wait for hosted CI, merge without bypassing checks, then remove only the task
branch/staged export and preserve unrelated working files/evidence/services.

OpenCode containment was inspected as inspiration for XDG paths and read-only
configuration, not reused for its auth/session database seeding or host hooks.
Version-matched OpenCode 2.1.2 source places current credentials in its database;
that remains unshared. Legacy file visibility did not establish fresh-guest auth
reuse in this native gate; it is not host-account synchronization. Read-only selection
uses per-file binds (no original-source relabeling), avoiding directory exclusion
leaks. Independent review identified Linux argument-size risk; the combined
derived mount request is now capped at 48 KiB before approval/capture/creation.

After the OpenCode decisions/gates above: Codex 0.156.1 uses `$CODEX_HOME/auth.json` with file/keyring/auto/ephemeral
backends; file save truncates/writes the auth file. Do not infer keyring sharing
or refresh compatibility from mounting that file. Native source:
[auth storage](https://github.com/openai/codex/blob/rust-v0.156.1/codex-rs/login/src/auth/storage.rs).
Resolve config/plugin path and guest-writable auth behavior before extending the
OpenCode-only declaration. Claude Code follows that slice.

- New bindings should propose credential-inclusive, read-only access to the
  selected harness's existing configuration in the scope-review prompt. Support
  OpenCode, Codex and Claude Code, including provider configuration/authentication,
  plugins and themes where their native behavior permits it. Existing bindings
  must not silently gain access.
- Provide reviewed snapshot/copy and disabled modes, plus explicit exclusions,
  especially provider auth. A directory bind cannot subtract excluded files;
  resolve narrow native paths or require a filtered snapshot rather than exposing
  an excluded credential through its parent directory.
- Read-only protects mounted contents from guest writes, not credential reading,
  use or exfiltration. Plugins/configuration may execute guest code. Keep native
  sessions/databases and writable caches guest-private; no whole-home/socket
  mounts, host execution fallback, automatic credential synchronization, gateway
  migration or policy widening.
- Map version-matched upstream native paths, embedded credentials, host-only path
  references, auth refresh and writable-state needs before implementation. Reuse
  config/import/OpenShell owners; distinguish host OpenCode UI from guest-server
  settings and verify original-source SELinux handling without blind relabeling.
- Validate exclusions, path safety, plugin execution, drift and legacy bindings
  with synthetic fixtures, then run repository-required checks. Live proof needs
  separately authorized disposable resources, never the existing project sandbox.

### Open question: authentication refresh and reload

**Read-only auth may prevent login refresh, and snapshots may become stale.**
Investigate an explicit `reload` command, not automatic synchronization. Separate
refreshing a reviewed snapshot from a harness's need to write refreshed auth:
reloading bytes alone does not solve write-required token rotation. Also verify
whether a running harness rereads files and whether native atomic file replacement
leaves a live file bind pointing at an old inode.

Any reload design must preserve exclusions, review changed scope, respect bound
runtime identity and existing snapshot refresh refusals, and report restart or
replacement requirements without silently restarting active work. Do not add a
writable host credential mount, host write-back or auth/session database sync as
a workaround. Guest-native login/refresh or an explicit unsupported state remain
options where safe read-only reuse cannot work. The first `reload` only supports
unbound configuration selection/capture and refuses every runtime binding;
active-sandbox refresh and guest token rotation semantics remain unsettled.

## Deferred

Automate serve/forward/attach only after the relevant lifecycle/version gates
pass. Keep the native foreground transport; no new proxy or compatibility layer.
Live-mode promotion/defaults, active-import refresh, and Telltale are separate decisions.
Release/tagging and npm/image publication require separate explicit
authorization. Existing implementation/delivery authorization does not promote
live mode to the default or resolve these validation gates.
