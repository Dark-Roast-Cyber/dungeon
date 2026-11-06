# Remote-first work plan

Goal: host-native clients/editors, isolated agent execution, and visible local
project edits without weakening the boundary. Validation does not authorize
tool upgrades, client installation, release/tagging, or publication.

## Active handoff: forward-compatible OpenShell and simple onboarding (2026-11-06)

**Read this section first after compaction.** It supersedes the earlier exact-pin
proposal for the next batch. The history below remains evidence, not a current
claim that the new runtime works. This turn is planning only; start implementation
when the user starts the next phase. No installer, runtime upgrade or service
change was performed while preparing this handoff.

### 1. Outcome and user-selected compatibility policy

Deliver one practical path from a supported installation to the chosen guest
agent: `dungeon setup`, then `dungeon` in a project. Keep advanced/headless options.
Do not add a daemon, backend trait, multi-version adapter, installer framework,
credential copier, policy language or agent orchestration layer.

The user explicitly accepts some forward-compatibility risk:

- Port the existing adapter to **OpenShell 0.1.7 as the initial minimum**.
- Accept newer stable versions by default, including later minor/major versions.
  Do not impose an exact Dungeon pin, arbitrary upper bound or per-version override.
- This means **attempt newer releases**, not certify all future behavior. Required
  response, identity and enforcement checks still fail closed on incompatible data.
- Test the minimum and the current stable release. If they are the same release,
  record that honestly; synthetic future-version tests prove admission logic only.
- Check CLI/gateway/driver compatibility separately. Prefer a matching release
  pair unless documented negotiation establishes compatibility. Do not accept an
  old gateway merely because a newer CLI passes the minimum-version check.
- Provision only a reviewed compatible gateway artifact for the selected CLI,
  not a permanently hardcoded 0.0.116 image or a blindly floating gateway tag.
  Ordinary launch must not install/update the CLI, refresh services or migrate data.
- Prerelease, malformed and unrecognized version output remain unsupported in this
  batch. Diagnostics must distinguish an old version from an unparseable response.

Do not implement this by deleting the current version check. The 0.1.x boundary
changes deployment, policy, resource identity and APIs, not just a version string.

### 2. Resume checkpoint and existing evidence

- Checkout: `/home/christian/github/dungeon`; branch
  `feat/setup-wizard-shared-images`. Preserve its published history and local work.
- Runtime fixes already committed: `dfaa449` (fallible output/pre-creation errors),
  `7f49910` (container machine init), `5f6f556` (live-written-script native gate).
  `46aff11` was the previous planning commit; this section replaces its proposal.
  Those four commits had not been pushed when this handoff was prepared.
- Worktree audit: `git worktree list --porcelain` listed only this main checkout;
  `git worktree prune --dry-run --verbose` found no stale registrations. Nothing
  needed deletion. Do not remove the main checkout or delete branches as cleanup.
- Existing implementation still requires OpenShell **0.0.116**. It has not yet
  been ported or validated against 0.1.7.
- Last source release SHA-256:
  `d31078e016f35131e5af1de3c1bed5dab991b52fac4cf0ce7f6824cdc28e1566`.
  The installed `~/.local/bin/dungeon` remained the older
  `98b2ff87832a1a22b4fa51813152f1e884a7cd9599104b794bb47b995b80a550` binary.
  Validate a newly built `target/release/dungeon`, not an assumed PATH installation.
- Last combined checks: 249 Rust tests passed with serialized execution, 26 Node24
  tests passed, plus fmt/Clippy/release build and 40 native non-TTY smoke commands.
  Earlier ownership-test failures were intermittent and are not claimed fixed.
- **Native edit/test is still blocked on the current Podman host by exit137**.
  New/edited JavaScript fails on both cached images and in a Podman-only control.
  Existing scripts and inline Node pass; no cgroup OOM kills were recorded. The
  available logs did not identify the cause. An active security sensor is not proof
  that it caused the kill. No bypass, relabel repair or fallback was introduced.
- Evidence: `docs/validation.md#headless-follow-up-2026-11-06-source-checkout`,
  `/tmp/opencode/dungeon-headless-fixes/` and `/tmp/opencode/dungeon-headless-audit/`.
  The audit's `live-file-probe.py` is the detailed reproducer; the fixes directory's
  `native-control.py` reproduces outside Dungeon. Keep failed gate results visible.

The user's separate Debian-machine experience was: verbose doctor with many
dependent blockers; offline init appeared to promise readiness; upstream install
delivered 0.1.7; raw `openshell sandbox create` produced a guest without OpenCode;
Dungeon then rejected 0.1.7. Its old local-image default/customization text suggests
an older Dungeon build too, but confirm executable identity rather than assume it.

Primary sources already checked:

- [0.1.7 release](https://github.com/NVIDIA/OpenShell/releases/tag/v0.1.7), stable,
  published 2026-11-03; recheck the current stable release when implementation starts.
- [0.1.7 README](https://github.com/NVIDIA/OpenShell/blob/v0.1.7/README.md): the
  installer sets up a CLI/local gateway; the default guest is minimal Ubuntu with
  **no agent**. A raw demo guest is not Dungeon setup.
- [0.1.0 upgrade guide](https://github.com/NVIDIA/OpenShell/blob/v0.1.7/docs/upgrade/0-1-0.mdx):
  local 0.0.x cannot upgrade in place; persisted sandboxes must be recreated;
  gateway config is schema2; identities, policy and request/deletion semantics change.
  Mixed old/new peers and databases are not an upgrade strategy.

### 3. Read first and inspect the existing owners

Read `AGENTS.md`, `README.md`, `SECURITY.md`, `docs/architecture.md`,
`docs/validation.md` and the recent local `.opencode/decision-log.md` tail.
Use synthetic fixtures only. Never read `.env` variants or real credential/session
stores. Reuse adoption Issue #18 rather than create a second issue tracker here.

| Responsibility | Existing owner / main regression locations |
| --- | --- |
| Runtime version check and sole OpenShell subprocess boundary | `src/openshell.rs::Runtime::new`, `src/main.rs::OPEN_SHELL_VERSION` |
| Gateway setup/version/health/storage/ownership checks | `src/setup.rs::require_version`, `gateway_health`, setup predicates and embedded tests |
| Read-only diagnostics and human/JSON rendering | `src/doctor.rs` and embedded tests |
| Configuration validation, fingerprints and runtime markers | `src/config.rs`, `src/binding.rs`, `src/export.rs` locks |
| Guided init/setup/launch, summaries and versions | `src/cli.rs`, `src/runtime.rs`, `tests/cli.rs`, `tests/setup.rs` |
| Policy and runtime identity | `src/policy.rs`, `src/openshell.rs`, `tests/lifecycle.rs` |
| Imports and their bounded mount contract | `src/imports.rs`, `src/harness.rs`, `tests/harness.rs` |
| Owned execution, SSH, streaming and attachment | `src/owned.rs`, `src/owned.cjs`, `src/owned_ssh.rs`, `src/stream.rs`, `src/remote.rs`, `tests/machine.rs` |
| Machine wire contract / plain-container regression | `src/machine.rs`, `src/container.rs`, `tests/container.rs`, `docs/machine.md` |
| Image requirements and local installation | `images/agent/`, `scripts/install-local.sh`, `tests/install_local.rs`, `tests/container-live.sh` |

**Identity trap:** `src/config.rs` hashes `OPEN_SHELL_VERSION` into every config
fingerprint, including plain containers. Separate the minimum accepted runtime
version from persisted adapter/config identity before replacing that constant.
Raising the admission floor later must not invalidate already-created compatible
guests. Preserve old plain-container fingerprints. Legacy OpenShell bindings must
be recognized/refused with recovery guidance, not silently sent to a new runtime.
Use a minimal persisted contract marker only if needed to make that distinction;
do not build a migration framework or keep a second executable legacy adapter.

### 4. Ordered implementation slices

#### A. Establish the modern contract before production changes

In a disposable Debian fixture, inspect the pinned 0.1.7 CLI/source/docs and actual
JSON outputs. Record the CLI/gateway/artifact versions and selected compute driver.
Do not run the upstream installer on this host or against an existing gateway.

Map each current operation to its supported native counterpart:

- Gateway discovery/registration, health, mTLS, ownership, workspace selection and
  service/port conflicts. Determine how to use an explicitly reviewed native local
  gateway without starting a competing Dungeon-managed gateway on port17670.
- Create/get/start/stop/delete, canonical name + workspace selectors, persistent
  runtime identity, labels and policy readback. Distinguish async deletion or an
  uncertain mutation outcome from completion; do not blindly retry creates.
- Schema2 gateway config, bind admission, host-project scope, UID/GID mapping and
  selected read-only imports. Do not assume old `driver_config` JSON still grants
  the same bounded contract, or enable broader mounts to recover convenience.
- Upload/download filtering and copy baselines; native exec/stdin/exit behavior;
  SSH/forwarding and the existing owned cleanup/receipt path.
- Existing starter policy/image compatibility, including hard enforcement, native
  parent-directory writability and the chosen agent's real entrypoint.

Append a small mapping/status table under this section as evidence. Then choose
the smallest concrete port. If a required security/identity/import/cleanup contract
cannot be preserved, stop for a scoped decision; do not silently remove a feature
or report unsupported behavior as working. UX-only corrections can ship separately
from a blocked port, but must not advertise 0.1.x support prematurely.

#### B. Implement minimum-version admission and stable persisted identity

- One bounded version parser/checker, owned by the existing OpenShell boundary,
  shared by runtime, setup and doctor. No duplicated string gates or capability
  registry. Reuse existing helpers where they actually meet this contract.
- Accept normal `openshell VERSION` / `openshell vVERSION` output and stable
  semantic versions, including valid build metadata; reject malformed, overflowing,
  control-bearing, prerelease or below-floor values with sanitized diagnostics.
- Unit cases: 0.0.116 and 0.1.6 refuse; 0.1.7, 0.1.8, 0.2.0 and 1.0.0 admit to
  contract verification. Synthetic future-version admission is not native support
  evidence. A numerically newer CLI with incompatible required fields still fails.
- Report observed version, minimum and tested baseline. Newer-than-tested status
  is informational, not a repeated approval prompt or hard upper-version gate.
- Test CLI/gateway mismatch and the separation of admission version, selected
  gateway artifact and configuration identity. Preserve identity on restart and
  later minimum-floor changes; fail safely on legacy OpenShell state.

#### C. Port the one adapter and setup path

Update the existing concrete OpenShell implementation, policy owner and setup
predicates to the verified modern native contract. Keep direct Podman/Docker
execution separate and unchanged; npm remains distribution-only.

- Keep explicit canonical project scope, service identity, policy verification,
  manual approvals, `--no-auto-providers` semantics and import exclusions. If a
  native flag disappears, verify the replacement behavior instead of dropping it.
- Prefer native lifecycle/identity guarantees over another Dungeon registry. Keep
  unknown output, partial creation and mismatched identity conservative. Preserve
  truthful effect classification and the existing no-replacement recovery posture.
- Preserve machine schema1 and workload-byte/exit semantics. Adapt internal native
  response parsing; do not change the external wire contract to match every vendor
  release. Do not add plain-container JSONL/owned transport in this batch.
- Reuse only an explicitly selected, compatible local gateway after ownership,
  endpoint/auth and driver checks. Never switch the user's active gateway, overwrite
  foreign config or restart a native systemd service as an incidental repair.
- If fresh provisioning is necessary, use a verified artifact compatible with the
  discovered CLI and existing explicit setup approval. Do not install host packages
  or introduce a generalized package installer. Unknown provisioning paths must
  produce a precise manual prerequisite, not a shell pipeline executed by Dungeon.
- Preserve all old data. Document use of a retained old binary for legacy export/
  recovery; actually exporting, deleting or recreating legacy guests needs separate
  explicit authority. Do not mix old/new gateways against one database.

#### D. Make human diagnostics and first-run flow small

- Default doctor: readiness, selected runtime, installed/minimum/tested versions,
  independent root blockers, one next command/action and one relevant guide link.
  No long policy dump, repeated links or historical integration checklist by default.
- `doctor --verbose` retains the full observations; JSON retains complete findings,
  existing meanings and blocking exit status. Dependent checks may render as skipped
  for humans without suppressing failures or inventing healthy results.
- Reuse the same bounded read-only predicates for launch preflight. Check missing/
  incompatible prerequisites before first-launch questions or new project binding
  writes. Doctor must not create engine storage, activate sockets, pull images,
  launch workloads or read credentials. Do not turn it into a dynamic containment test.
- Explicit offline `init` stays usable without runtime contact, including `--yes`;
  say **configuration saved; runtime readiness not checked**, with the next setup/
  launch action. Do not add native probes or assert unobserved readiness merely to
  improve this message. Launch preflight, not offline init, owns readiness checks.
- Human setup/init review becomes a short summary of runtime, image, agent, project
  access and optional imports, followed by one explicit approval. Keep `plan`'s
  detailed JSON and bare `config` read-only; stop prescribing `init -> plan -> up`
  as the normal workflow. Retain unbound-only edit and headless behavior.
- Surface the selected Dungeon executable path and build revision when available
  through the existing version/diagnostic path and installation output. Do not add
  an updater daemon or run arbitrary project Git commands to obtain this metadata.
- Distinguish runtime unavailability from image/tool unavailability. Every error
  should explain what is wrong, whether anything changed and the next useful action.
  Escape untrusted paths/output and retain fallible output writes.

#### E. Verify the image and chosen agent, not just a running guest

- Always use the explicitly resolved image. Keep the user's existing upstream
  image default and tag/digest overrides unless a separate decision changes them.
  Never fall back to OpenShell's minimal Ubuntu default or silently switch runtimes.
- Through the selected runtime, verify the chosen agent's actual executable/version
  before announcing a successful agent launch. Probe only that configured agent;
  do not require all harnesses or every language toolchain in a custom image.
- Report a missing agent/tool with the selected image and an image-specific remedy.
  Do not install arbitrary guest tools automatically. Missing Cargo/pnpm does not
  mean a host installation will help; keep language tools image-owned.
- A failed post-create check must preserve the inspectable guest/config and explain
  deliberate recovery, not silently rebuild or overwrite guest auth/sessions. Keep
  config edits refused after binding. Stop/destroy/reinitialize remain explicit.
- Revalidate the existing reviewed recipe and native parent layout on 0.1.x; old
  image or `--version` success alone does not prove supervisor/policy compatibility.

### 5. Regression and acceptance matrix

Implementer runs focused checks per slice, then verifies the combined result:

| Gate | Required observation |
| --- | --- |
| Version policy | Minimum/newer admission, invalid/prerelease/old refusal, and meaningful installed-versus-required messages |
| Required native contract | Wrong/missing fields, unknown identity, gateway mismatch, policy drift and uncertain native outcomes fail safely |
| Persistence | Existing plain fingerprints unchanged; legacy OpenShell never retargeted; modern same-ID resume retains guest files/sessions |
| Read-only behavior | Doctor, verbose/JSON diagnosis and previews leave HOME/project/storage/services/images/config unchanged |
| Human first run | Real PTY: supported setup, short review, one approval, selected agent launches; cancel leaves scope unchanged |
| Error UX | Missing engine/CLI, wrong CLI, stale Dungeon path, tool-less image and foreign/port-conflicting gateway each have one useful remedy |
| Headless | Explicit flags/approval, no terminal reads, clean JSON or native stdout/stderr as appropriate, binary stdin, literal argv and nonzero exit preserved |
| Images/imports | Actual selected agent runs; project remains sole writable host bind; approved snapshots/read-only bytes and source labels remain correct |
| Edit/test | Host/guest-created scripts, in-place/atomic edits, real tests and post-launch Cargo doctests on the Rust image execute, not merely inline Node |
| Lifecycle/cleanup | Stop/resume preserves identity/state; owned OpenShell cleanup retains its evidence contract; ordinary container interruption remains documented as degraded |
| Containment boundary | Native expected file/network denials and engine-socket/home exclusions; no claim of desktop/EDR or general platform certification |

Use fresh Debian fixtures for the reported installation path and the actual
minimum/current stable runtime pair(s). Record host/kernel, engine/driver, CLI,
gateway and image versions/digests. No real-provider login/inference or real host
config is needed: use synthetic projects and a loopback provider where the approved
fixture policy permits it. A preseeded provider probe is not an edit/test gate.
Keep actual failures separate from mocked protocol tests and skipped prerequisites.
Verify plain Podman behavior after shared identity/config changes; use an available
disposable Docker gate for changed shared behavior, otherwise record that gap.

Repository-owned checks remain:

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
node --test npm/launcher/test/platform.test.mjs npm/test/stage-packages.test.mjs \
  tests/owned.test.cjs images/agent/verify-layout.test.cjs \
  scripts/probe-opencode-identity.test.cjs
sh -n tests/container-live.sh
```

Use the documented Node24 gate, uv for Python and pnpm for Node package operations.
Capture noisy output under a new task-specific `/tmp/opencode/` directory. Read
failures and preserve flaky-test evidence; do not weaken assertions/deadlines or
retry indefinitely until green. Serialized Rust execution can diagnose contention,
but must be labeled. Preserve cross-platform compile/warning gates.

**Exit137 is a separate P0 investigation, not permission to weaken enforcement.**
Identify the kill source from host/runtime/security telemetry or a controlled
comparison before selecting a fix. Do not disable EDR, SELinux, seccomp or policy,
use `node -e` as an execution workaround, or remove/skip the live-written-file gate.
Passing a disposable Debian fixture does not close a failure on the original host.
If it remains blocked, report that host boundary explicitly and do not claim full
native edit/test acceptance. Keep ownership-test flakiness separate from this cause.

### 6. Delivery, stop conditions and first action after compaction

- Continue the cohesive adoption workstream; inspect current Git/PR state before
  adding a branch or PR. Use meaningful implementation/test/doc commits, not one
  PR per slice. Local commit authority from the previous task is not a new push,
  merge, release, host-installation or runtime-migration grant.
- Update authoritative README/configuration/architecture/security/machine docs with
  changed behavior, and `docs/validation.md` with exact passed/failed native evidence.
  Keep implementation planning here; append important accepted decisions to the
  ignored main-checkout `.opencode/decision-log.md` without rewriting its history.
- Stop for a decision if the port requires wider mounts/network policy, credential
  access, host service takeover, destructive migration, a new backend or a materially
  different enforcement contract. Do not silently narrow existing advertised features.
- Do not upgrade or replace this host's working 0.0.116 gateway, stopped authenticated
  guests, installed Dungeon, OpenCode shared service or existing image tags. Only
  newly authorized disposable resources may be removed; inventory before/after.
- **First action:** inspect `git status`, current log/worktrees and guidance; read
  the sources in section3; then perform sliceA's bounded native-contract assessment.
  Summarize the concrete port/identity/setup decisions before changing production
  runtime behavior. Do not begin by bumping `OPEN_SHELL_VERSION` or executing an
  upstream `curl | sh` installer on the user's machine.

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

### User smoke-test follow-up: provider authentication (2026-10-22)

User installed merged PR #41 from a clean worktree and built the corrected image.
Their transcript shows approved copy capture (including legacy auth.json and
OpenCode JSON config), successful sandbox creation/upload and attached host UI,
but the model picker reports no available items. This is user-observed behavior,
not independent provider/discovery validation. File capture/startup did not meet
the original signed-in-provider reuse goal. OpenShell `providers: []` is a
separate mechanism, not proof about OpenCode account discovery. Do not inspect
the user's real config, credential snapshots or native databases to diagnose it.

Pinned upstream 2.1.2 exposes native credential list/create/activate APIs. Proposed
next contract: an explicit, reviewed one-way import into guest-owned credentials,
not database/session copying or ongoing synchronization. Develop against synthetic
credentials first. Metadata review must precede secret access; the credential
list endpoint returns **all secret values**, so transfer selection is not a
promise of selected-only host access. Do not invoke it against real host state
without separate approval. Resolve API-key, OAuth refresh/rotation and external
credential-reference semantics separately; external references are not portable
secrets. Never overwrite guest credentials or host state implicitly, log secrets,
send them on argv, or widen network policy. Credential-inclusive default intent
remains; selecting a safe native handoff contract needs review before implementation.
Sources: pinned `packages/protocol/src/groups/credential.ts`,
`packages/schema/src/credential.ts` and `packages/server/src/handlers/credential.ts`
in anomalyco/opencode v2.1.2. No host credential API or running sandbox was touched.

### Future requirement: one-command update

User requests `dungeon update` as the future single-command update path; it is
not implemented by this follow-up. It should review/update the CLI and compatible
reviewed agent image together, detect matching host-client requirements, verify
artifact versions/integrity, and preserve a rollback path. Source installs must
not build unrelated dirty checkout content. No implicit database migration,
credential transfer, gateway restart, running-sandbox replacement or bound-config
rewriting. Existing guest sessions/data must remain intact. Define update versus
explicit environment replacement before adding runtime behavior. OpenCode's UI
`/update` updates the vendor tool, not Dungeon's CLI/image contract; it must not
be presented as the solution for coordinated Dungeon upgrades.

### Accepted direction: sign in once, independent project environments

User approved development of user-scoped sign-in after reconsidering credential
import. The shared identity must survive project `up`/`down` and deletion, work
across directories for the selected compatible harness/account, renew OAuth
without routine prompts when safe, and remain restrictable for organizations.
Bare `dungeon` should approach native-harness convenience after initial enrollment.
This supersedes the proposed host-account import as the primary path, not the
existing binding/import contract. Configuration imports and authentication remain
separate; do not silently alter current bindings or widen their credentials.

Development authority does not include reading real credentials, native host
databases, altering existing services/sandboxes, installation or publication.
Preserve the ten unrelated dirty machine/socket paths. HEAD is merged `f19499e`.
User requested independent read-only design challenges: Claude adapter
`ses_e99579d29ffeDllDKFqytq20bC` (completed), adversarial
`ses_e99575217ffeiEDAP3EZJauoHh` (completed). No child edits authorized.

Source investigation of pinned OpenShell0.0.116 finds gateway refresh state,
rotated-secret persistence/recovery and endpoint-scoped placeholder resolution.
However `e2e/rust/tests/credential_gating.rs` explicitly refuses credentialed L4
and `tls: skip` endpoints unless an uninspected-credentials escape hatch is used.
Current Dungeon starter endpoints are L4; attaching a provider alone cannot be
treated as a secure functional broker. Any explicit REST inspection contract
must disclose gateway access to request contents and remain policy-reviewed;
never enable uninspected credentials to hide this incompatibility.

Native subscription adapters are another gate: pinned OpenCode's OpenAI plugin
switches endpoint, account headers, model eligibility and refresh behavior based
on a typed OAuth credential. An API-key/environment placeholder alone does not
prove ChatGPT subscription compatibility. OpenShell's generic refresh support is
not a native enrollment flow, and no real endpoint/provider API has been invoked.

Adversarial review challenges accepted for investigation: no cloned refresh
tokens/shared writable auth trees, no enrollment-only org policy checks, no
mutable default-account retargeting of existing bindings, no implicit provider
attachment to legacy or machine configs, no renewal version in sandbox fingerprint,
no claim that masking tokens prevents account spend/abuse. Its claim that a
hostile repository directly supplies private dungeon.json is outside the current
authority model; repository config remains non-authoritative. Same-user host
compromise is outside the container isolation claim, not solved by local brokers.

Next development gate: prove a real pinned OpenCode model request using only a
synthetic environment credential marker before adding user-facing shared-login
commands or automatic grants. Separate this harness-compatibility probe from
native OpenShell substitution/rotation/revocation acceptance. Do not label a
mock proxy or metadata-only provider registration as shared sign-in completion.

Claude review (native external session `ffcb2e68-291f-4882-9ae6-2dd7ef4eb6ba`,
requested Opus5.5/xhigh; reported canonical Opus5.5) also recommends reusing the
gateway, not adding a Dungeon broker. It challenges credential-inclusive legacy
auth imports as a poor default for a fresh-login architecture; do not change the
approved default silently. It recommends proving opaque bearer-token support
first; Claude subscription setup-token is a candidate, not a validated path.
Both reviews reject claiming generic OAuth sharing from provider registration.
They differ on when to build an organization ceiling: the parent will not add
speculative enterprise machinery before proving the credential path, and will
not ship automatic shared-account grants until enforceable restrictions exist.

Implementation started with `scripts/probe-opencode-identity.cjs`: actual pinned
OpenCode, network-disabled throwaway image, loopback synthetic model endpoint,
two independent guest HOME/state roots and a synthetic environment credential
marker only. Three native runs must return the model proof and discover the env
connection. This is a compatibility probe, not a second broker or production
credential storage. No shared-login CLI/options are introduced prematurely.

Completed synthetic gateway gate: a fresh disposable FCOS QEMU/KVM fixture used
only trusted CLI binaries, the user-built
reviewed-image variant `3947cf4ee2026ad960f3b641656046439708827160527382417dda10581f9dbd`,
synthetic local model/token fixtures, independent HOME/Podman store/loopback and a
fresh gateway. Scripts/logs `/tmp/opencode/dungeon-shared-auth-*`; protected host
baseline `/tmp/opencode/dungeon-shared-auth-host-before.txt`. No real credentials
or runtime inputs. Goal: measure actual OpenShell substitution and single-owner
refresh before production account management, not infer it from the mock probe.
Four fixture sandboxes, synthetic provider/profile, model/token containers,
gateway/network, QEMU/listener and owned disks/keys/archive are removed. Protected
service IDs/states/start times matched; the user's host image/bindings and ten
unrelated dirty paths are preserved. Evidence:
`/tmp/opencode/dungeon-shared-auth-evidence-20261022`.

Measured: native two-project model calls with opaque markers; two gateway-owned
manual rotations consuming replacement refresh material; existing long-lived
OpenCode service still worked; same-ID down/up and deletion/new-project reuse;
opted-out sandbox guessed marker denied; bounded guest-readable file scan found
no exact synthetic tokens (skips explicitly recorded). Attached provider delete
refused; explicit detach-all then delete prevented subsequent model calls, with
observed 61-second command completion, not instantaneous revocation proof.

Final adversarial evidence review accepted post-rotation resolution after reading
the fixture's strict latest-generation predicate: old access values are rejected
even before their declared expiry. This is not scheduled/vendor OAuth renewal.
The resume record is limited to successful down/up commands and identical Ready
identity; a separate intermediate Stopped observation was not retained. Fixture
sources, native lifecycle script/log and these limits are in the evidence manifest.

Failures retained: wrong token fixture namespace initially; static-to-refresh
live conversion had transient denied calls before supervisor propagation;
scan-prefix false positive in its own test code; initial probe scan bound too
small for vendor temporary binaries. Corrected assertions passed, not erased.
179 Rust/eight Node tests, fmt and Clippy passed on the combined tree. No new
production shared-signin command, credential storage, auto grants or org ceiling.

Next implementation boundaries, not implied existing features:
1. Complete inspected HTTPS/CA-trust and endpoint/body mismatch probes before
   offering shared opaque bearer/API-key enrollment. Finish refresh configuration
   before giving a sandbox its stable handles; verify automatic worker renewal,
   concurrency and crash/revocation semantics separately.
2. Native subscription OAuth needs provider-specific protocol/adapter evidence;
   OpenCode typed OAuth affects endpoints/models/headers. Do not duplicate refresh
   grants or quietly require subscription users to use API keys instead.
3. Implement explicit enrollment/account binding first, with per-project opt-out
   and immutable account identity; reserve automatic default grants until a real
   admin-controlled organization ceiling covers create/reuse/human/machine paths.
   A same-user editable file cannot enforce policy against that host user or
   clients calling OpenShell directly. No speculative broker/framework added.
4. Preserve credential-inclusive config behavior until its default is explicitly
   revisited; fresh identity architecture does not silently migrate existing
   imports. No install, commit, push, image publication or existing-service
   mutation is part of this development grant.

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
