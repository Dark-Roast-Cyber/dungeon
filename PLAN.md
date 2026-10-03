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
Imports are plan-only/fail-closed. Telltale is not integrated.

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

Status: implementation and disposable local live gates completed; independent
review pending. Issue #28: **Make remote guest-service
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

Status: disposable local matched 2.0.14 gate PASS; parent independent review
required before publication. No host/guest upgrade or image rebuild performed.
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
No editor integration pass is claimed. Validate one integration at a time. GitHub
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

Issue #22 adds `doctor`/`doctor --json` read-only diagnostics; parent independent
review is required before delivery acceptance. Scope is Linux/OpenShell 0.0.116,
existing rootless Podman and selected existing private config only. Independent
checks, bounded sanitized native queries, explicit blocking exit semantics and
configuration-only live capability/mapping observations do not install/migrate,
promote live, implement imports or dynamically verify recorded integrations.
Unknown foreign gateways/custom engine storage remain explicit, not adopted.
Independent-review safety corrections cover descriptor-relative private
config/policy reads, all-project helper PATH exclusion, exact driver-data
environment identity, WNOWAIT query cleanup and empty-store initialization
avoidance. Focused regressions and native scratch evidence are recorded;
independent rereview remains the next acceptance gate.

## Deferred

Automate serve/forward/attach only after the relevant lifecycle/version gates
pass. Keep the native foreground transport; no new proxy or compatibility layer.
Live-mode promotion/defaults, reviewed imports, and Telltale are separate decisions.
Release/tagging and npm/image publication require separate explicit
authorization. Existing implementation/delivery authorization does not promote
live mode to the default or resolve these validation gates.
