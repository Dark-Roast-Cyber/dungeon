# Validation and release boundary

CI checks formatting, Clippy, locked Rust tests, release compilation, CLI version execution, npm launcher behavior, distribution version synchronization, native archive creation, and full npm package assembly on explicit supported runners.

## Version policy

- Rust is pinned in `rust-toolchain.toml` to 1.98.1.
- `Cargo.lock` is committed and is the authoritative exact dependency graph for the application.
- GitHub Actions are pinned to immutable commit SHAs corresponding to stable action releases.
- CI uses explicit GA runner images: Ubuntu 26.04, macOS 26 arm64, macOS 26 Intel, and Windows Server 2025.
- npm launcher validation targets Node 24 LTS.
- OpenShell 0.0.116 is an intentional compatibility pin because Dungeon consumes version-sensitive CLI and structured-output contracts.

Offline tests cover strict duplicate-key JSON, safe names, schema rejection, starter-policy requirements, create arguments, auth command scope, removed provisional commands, native launcher selection, npm staging, and native archive production. They do not replace live isolation, vendor-auth, image/tooling, remote-editor, endpoint-security, or platform validation.

Issue #8 lifecycle contract: verify first `up` creates and uploads once; resumed
`up` starts the same bound sandbox without another upload and is a no-op when
Ready; `run` starts a stopped bound sandbox without upload; and `down` stops it
without deleting guest state. Verify the separate `upload --yes` path is explicit
and overwrites the guest project.

Issue #9 export contract: on OpenShell 0.0.116, verify export is restricted to
the bound Ready sandbox; upload records pre-upload host values only when they
exactly match post-upload guest entries (type, hash, Unix executable bit), bound
to the config fingerprint and sandbox ID. Gitignore filtering must not create
a false baseline for a stale guest path. Export downloads the complete guest
tree, presents a deterministic summary, and requires typed `yes` or explicit
`--yes`. Exercise host-only preservation,
non-conflicting guest additions/edits/deletions, three-way conflict refusal,
special-entry refusal, metadata limits, and failed-apply private adjacent
backups/manual recovery. Verify baseline-less legacy configurations are refused
and can recover only by manual OpenShell download/inspection followed by explicit
`upload --yes`. These checks must not be reported as passed until run; unit/mock
coverage does not establish live OpenShell transfer or filesystem apply behavior.

Issue #9 disposable live run used sandbox `issue9-live-0912` on the local
OpenShell 0.0.116 gateway with the `localhost/dungeon-agent:0.0.3-dev` image.
After first `up`, a noninteractive export listed `DELETE app.js`,
`MODIFY app.py`, `ADD guest-add.txt`, `ADD guestdir`, `ADD guestdir/inner.txt` and
refused apply without `--yes`. Approved export installed exactly those edits;
three subsequent exports reported no changes. Differing host/guest edits
produced `CONFLICT app.py` with no host mutation or baseline advance. A
host-only file remained untouched; guest deletions applied; a guest symlink
to `/etc/passwd` was rejected before mutation. In a controlled failure, a
guest-deleted directory containing a host-only file could not be removed;
original-file backups and a recovery marker were retained, baseline did not
advance, and retry refused until recovery inspection. That shape now reports
a conflict before mutation in focused tests. This is not a crash/power-loss
durability claim, nor cross-platform validation.

On the local OpenShell 0.0.116/rootless Podman 5.8.4 setup, a new synthetic
sandbox (`d8-live-926`, ID `a6a2f0e9-a672-4738-9642-83fc9dbba37d`) received
one initial upload. A guest-only `guest-edit.txt` survived `down`/`up` under
the same ID, and the original host fixture remained unchanged. A second `up`
reported already Ready. After another stop, `run opencode run` auto-started
that same sandbox and OpenCode read the guest-only edit from `/sandbox/project`
without an upload. The final stop was slow: after a 30-second client timeout
the backend reported `Stopping`, then transitioned to `Stopped`; a repeated
`down` was idempotent. This test did not check Claude login, Codex approvals,
export, or containment on other runtime combinations.

Issue #7 local setup uses Linux + rootless Podman + OpenShell 0.0.116 only. Gateway bootstrap uses a pre-pulled immutable OpenShell gateway image, user Podman API socket, generated mTLS/JWT material, the explicit Podman driver, and `default` workspace. `dungeon setup` checks a registered gateway and its live driver/version/health on repeats, without creating sandboxes or uploading projects. MicroVM is not a fallback (hard Landlock failed on the tested host). A healthy gateway is not an enforcement test or validation of every Podman/OpenShell combination. Setup does not install privileged prerequisites or publish the reviewed agent image. Issue #11's Claude-account check remains open.

## Native release gate

A release tag must be exactly `vMAJOR.MINOR.PATCH` and match the package version in `Cargo.toml`. The release workflow rebuilds from the tagged source, runs locked quality/tests, executes each native binary, emits four archives, generates `SHA256SUMS`, and creates a GitHub prerelease.

No release is created by ordinary CI, pull requests, or branch pushes.

## npm package gate

Ordinary CI stages the real native binaries into temporary platform packages and creates five npm tarballs: four platform packages plus the launcher. Checked-in platform templates remain private and unchanged.

This validates packaging only. npm registry publication remains blocked until the package scope, provenance, and publishing credentials are explicitly established.

For the first end-to-end execution, use [live-validation.md](live-validation.md) and record the exact tested runtime combination.

## Issue #1 live result (synthetic fixture)

The first live run used OpenShell CLI/gateway 0.0.116, rootless Podman 5.8.4, and community base image `ghcr.io/nvidia/openshell-community/sandboxes/base@sha256:aeef1c63f00e2913ea002ccb3aaf925f338b5c5d70e63576f0d95c16a138044e`. The initial PR head was `6c4d16d9397aefacec1a99d1eb5a0c664f08eb23`. Create/status identity, explicit project upload after a local fix, bounded path/network denials, allowed endpoints, exec exit codes, shell PTY/Ctrl-C, stop/start persistence, config mismatch, policy drift, and generic SSH succeeded. OpenCode 1.2.18 exposed a WAL SQLite database; a post-exit `VACUUM INTO` snapshot was downloaded through OpenShell into a private host root and passed SQLite integrity checking (SHA-256 `39d2dd1b1fb9405ee1ebf801971a58c3ca1c176096629e71c653b68cd06ff854`).

Three Dungeon fixes resulted: enforce OpenShell's 19-character sandbox-name limit, remove nonexistent `/app` from the hard-requirement starter policy, and upload source `.` from the selected project directory to `/sandbox/project`. Claude Code 2.1.156 could not pass its startup check because the starter policy denied `platform.claude.com:443`; Anthropic's [network requirements](https://code.claude.com/docs/en/network-config#network-access-requirements) identify this host as required for OAuth token exchange and refresh. The endpoint was added exactly; targeted revalidation reached Claude's vendor login-method screen, without completing authentication. Codex 0.117.0 device login succeeded, but its default model was no longer available to the ChatGPT account, while a current model required a newer guest CLI. [Guest image/tooling freshness](https://github.com/Dark-Roast-Cyber/dungeon/issues/6) is separate from Dungeon auth/model routing.

An OpenShell MicroVM attempt on this host remained Provisioning with a guest Landlock-unavailable finding even after removing `/app`. **Only the recorded Podman combination is validated.** OpenCode agent edits, completed Claude login, Codex edits, VS Code, desktop clients, sandbox replacement, and EDR remain unproven. Policy readback and mock tests are not kernel enforcement attestations.

## Live gates

Issue #5 plan-only first slice: synthetic tests exercise a Neovim directory
declaration in the existing private schema-2 config, config fingerprint change,
duplicate destination, traversal, project/config-tree import, credential-state
entry, symlink, and socket refusal. `plan` inventories selected files but does
not apply a guest import. All runtime commands fail closed for a declared import;
the OpenShell 0.0.116 upload path runs as the guest sandbox user and cannot by
itself enforce a read-only destination under writable `/sandbox`. No live import
or guest read-only claim has been established.

A live read-write workspace mount of the configured project is now the accepted
default-direction decision for the contained-development loop. This decision is
architecture-level; it has not been implemented or validated. A guardrail
design (root/home/parent-escape and credential-path refusal, imports read-only,
no engine sockets) and live validation evidence are required before any live
mount ships, and the copied upload/export path remains available.

### Issue #6 local image candidate (historical)

The [image build instructions and supply-chain boundary](../images/agent/README.md)
describe a local-only candidate on Issue #1's unchanged base digest. The reviewed
package set is OpenCode V2 2.0.14, Claude Code stable 2.1.267, and Codex 0.156.1,
with base-provided Node 22.22.1 and build-only pnpm 10.33.0. These are observations
for this candidate; `images/agent/package.json` and its lock are the build inputs.

The frozen-lock installation and offline vendor setup scripts are followed by
`verify.cjs`, which checks exact CLI output against the manifest. Direct container
checks must run without authentication; vendor login and edit/test evidence must
come from a separate OpenShell sandbox with the existing hard Landlock policy.
Image build/version success alone does not establish contained agent usability.

The Issue #6 corrected local amd64 candidate built on rootless Podman 5.8.4. Its historical image ID:
`c0d3c28cdc9da1c7bae2ca06870e73505eee3003905caa42ee2a129397e7959b`;
local digest: `sha256:8ee097043582f4f3cd1b2f8e0981553b7f0889e496ceecc2dab13dcf153e48de`.
Containerfile SHA-256:
`60926d39e141b7cc498ed56bdb23660d600a6285c21e0a2885fc7b25e40fead5`.
The image is 4,434,995,801 bytes including inherited base layers. Offline direct
checks passed for all three version outputs, UID/GID 998, writable `/sandbox`,
root-owned non-writable tool installation, basic bash/git/curl/Node availability,
and absent engine sockets. Build-only pnpm caches are absent from the final
filesystem. History inspection found no added credentials or secret inputs.
The final-stage version check removes its transient guest state/cache, logs,
and helper paths before the image is committed. A no-cache rebuild of the
versioned package set produced identical vendor binary SHA-256 sums; a final
cache-cleanup-only rebuild preserved those same sums. OCI image metadata/digest
was not bit-for-bit identical.

Repository formatting, Clippy, all 12 Rust tests, release build, and all four
Node launcher/package tests passed. The Node tests passed under both host Node
22.22.2 and the supported Node 24.21.0 runtime, downloaded from nodejs.org and
verified against its published SHA-256 sums. Launcher packing with pnpm also
succeeded (host Node 22 emitted the expected engine warning).

The same package-version candidate, before the final cache/pruning-only image
rebuild, reached Ready under OpenShell 0.0.116 with the unchanged
hard Landlock policy. Upload landed directly at `/sandbox/project`; protected
reads/writes and `https://example.com` were denied. Allowed OpenCode and Claude
endpoints responded, and no engine socket was present. An initial image defect
(broken relative paths in symlinked pnpm shims) was fixed by linking directly to
vendor-declared binaries and adding final non-root PATH checks.

OpenCode completed the synthetic read/edit/test task and exited successfully
using its own default `mimo-v2.6-flash-free` selection. Dungeon did not select or
override that model. The test script's hash remained unchanged. Claude reached
interactive onboarding through `dungeon auth claude`. Claude authentication,
edit/test, and authenticated stop/start persistence are **NOT TESTABLE** in this
run because the operator has no Claude account. This is an account prerequisite,
not an observed image failure. [Issue #11](https://github.com/Dark-Roast-Cyber/dungeon/issues/11)
requests a tester with an eligible account; the user explicitly deferred these checks.

No OpenCode free-tier refusal occurred in this Issue #6 run: its agent-selected
model completed the synthetic task. That result does not generalize to every
OpenCode provider or account.

Codex `0.156.1` authenticated through official ChatGPT device login inside the
guest. `codex login status` reported `Logged in using ChatGPT`; only directory
ownership and modes were inspected (`/sandbox/.codex` owned by `sandbox`,
`auth.json` mode 600), not credential contents. Its TUI launched in
`/sandbox/project` with its own GPT-6-Astra default, without a Dungeon model
override. Codex's built-in Linux `bwrap` sandbox could not create a user
namespace within this OpenShell Podman guest. It offered per-command approvals
to execute within the guest but outside the **inner Codex** sandbox. Two
one-time approvals for the synthetic read and edit/test commands allowed the
task to complete; `./test.sh` passed and the unchanged test-script hash was
confirmed. Therefore task behavior is **DEGRADED** (requires approvals), not
fully autonomous. The Issue #1 stale-CLI/model incompatibility did **not**
recur, but inner sandbox availability is still a daily-workflow limitation.

After `dungeon stop`/`start`, the same sandbox ID returned Ready, the synthetic
project and test result survived, `codex login status` still reported logged in,
and the guest-owned state paths retained their modes. Protected `/etc/shadow`
reads and `/etc` writes were denied, `https://example.com` returned a denied
CONNECT (403), and no engine socket was present. Network responses from
`auth.openai.com` and `chatgpt.com` after login were HTTP 403; the successful
device login and model task are stronger evidence of functional Codex access
than these unauthenticated curl probes.

The OpenShell gateway needed mTLS/JWT bootstrap and a sandbox-reachable Podman
callback endpoint to get past provisioning. Those were dedicated gateway
configuration steps, not changes to Dungeon policy or the agent image.
Publication/provenance and default-image selection were not authorized by
this candidate. Issue #7 local setup was validated separately above.

### Issue #17 local image tooling (2026-09-26)

At Dungeon source HEAD `615e2f50bd8bb4f611ac331f4f369418b0dabaf3`
plus the uncommitted image/docs convergence that removes redundant XDG and
Cargo image environment variables, a local Linux amd64 candidate built with
rootless Podman 5.8.4 and `--pull=never` from the same exact base digest.
The final image ID is
`f41665067d11e4f2738a3134c93ea69ea47b8cf61a301f91a3a10e466398cdde`,
local digest `sha256:5a5353c130370361934d4826ee792e2f8038de40763f2713025dbb4ba83e0ce5`,
and Containerfile SHA-256
`73da7df24610e4e485ffb948a8dc83d55d85a4e49d0b0706926e62fe96863518`.
The pnpm, Neovim, `rustc`, `cargo`, and `rust-std` archives passed their
reviewed SHA-256 checks. The build's final network-disabled non-root
`verify.cjs --path` passed, as did a separate network-disabled final-image
run: Neovim 0.12.5, rustc 1.98.1, and cargo 1.98.1 resolved exactly under
`/usr/local/bin`; their immutable tool/support paths were root-owned and
non-writable to UID/GID 998 (`sandbox:sandbox`). Image history showed no
added build secrets or credentials.

With only `HOME=/sandbox` and no XDG or `CARGO_HOME` overrides, a synthetic
read-only Neovim config executed headlessly, rustc compiled and ran a trivial
binary, and a dependency-free Cargo project built offline and ran. Neovim
reported data, state, and cache under `/sandbox/.local/share/nvim`,
`/sandbox/.local/state/nvim`, and `/sandbox/.cache/nvim`; Cargo wrote under
`/sandbox/.cargo` and the build target under `/sandbox`. Direct Podman checks
also refused `/usr` and `/etc` writes and `/etc/shadow` reads; checked engine
sockets and credential paths were absent.

The same candidate reached Ready in a disposable Dungeon sandbox on the
healthy local OpenShell CLI/gateway/Podman driver 0.0.116 combination, using
the unchanged starter policy with Landlock `hard_requirement`. Without
per-command environment overrides, the same Neovim, rustc, and offline Cargo
tasks passed and wrote to those guest-private paths. A synthetic root-owned
read-only config staged solely for this test was readable and executable by
the guest but could not be written or chmodded by it. Protected writes and
`/etc/shadow` reads failed, common engine sockets and checked credentials
were absent, and unapproved `https://example.com` received CONNECT 403.
OpenShell exec exposed `HOME=/sandbox` but not the removed OCI XDG/Cargo
variables; it sanitizes the child environment. These tasks do not require
Dungeon-managed `--env` propagation. The disposable sandbox was deleted;
neither image publication nor a default-image/policy change was performed.
Issue #17's local image/tooling acceptance is PASS on this tested Linux
amd64/Podman combination, unblocking Issue #5's first image-owned prerequisites.
It does not validate Issue #5 imports, other platforms, desktop clients, or
EDR containment.

### Remaining gates

| Gate | Evidence |
| --- | --- |
| Runtime | exact CLI/gateway/driver/kernel/image and create/start/stop |
| Enforcement | denied path/egress/raw-network/privilege attempts |
| CLI agents | auth, fixture edit/test, PTY, interrupt, exit, resume |
| Persistence | project/session survival and deletion behavior |
| Upload | destination, symlinks, partial failure, overwrite |
| Remote | VS Code and each desktop path with credential boundary |
| Agent image | pinned versions launch/authenticate and can perform a bounded synthetic task |
| Project return (Issue #9) | Local OpenShell 0.0.116/rootless Podman: staged edit/add/nested add/delete, approval refusal, conflict, host-only preservation, symlink refusal, idempotence and recovery refusal exercised; broader platforms and interruption durability remain unvalidated |
| EDR | supported sensor observes/controls attributed guest process |

## Release

Creating the reviewed version tag is the explicit native-release authorization step. npm publication remains a separate future gate. Runtime support claims remain limited to the exact validated combinations. Issue #5's read-only configuration imports remain a separate implementation and validation gate after the Issue #17 image-owned tool prerequisites; the live workspace mount direction is tracked separately and is not yet implemented or validated.
