# Validation and release boundary

CI checks formatting, Clippy, locked Rust tests, release compilation, CLI version execution, npm launcher behavior, distribution version synchronization, native archive creation, and full npm package assembly on explicit supported runners.

## Version policy

- Rust is pinned in `rust-toolchain.toml` to 1.98.1.
- `Cargo.lock` is committed and is the authoritative exact dependency graph for the application.
- GitHub Actions are pinned to immutable commit SHAs corresponding to stable action releases.
- CI uses explicit GA runner images: Ubuntu 26.04, macOS 26 arm64, macOS 26 Intel, and Windows Server 2025.
- npm launcher validation targets Node 24 LTS.
- OpenShell 0.0.116 is an intentional compatibility pin because Dungeon consumes version-sensitive CLI and structured-output contracts.

## Issue #22 read-only diagnostics

`dungeon doctor [--json]` has synthetic coverage for independent inventory when
OpenShell is missing, an exact query allowlist, malformed/untrusted version
output, incompatible registration refusal without gateway contact, bounded
time/output, project-owned executable refusal, separate bind/mapping states,
stale/unrecognized local configuration and unchanged fixture state. The native
CLI integration test repeats diagnosis from an unbound project and verifies
stable schema-1 JSON/nonzero blocking status with no private binding creation;
invalid explicit selection is sanitized and remains blocking. These checks do
not establish live-bind, authentication, editor, nested-sandbox or containment
claims. See [the diagnostic contract](configuration.md#read-only-diagnostics).

A read-only local inventory run on Fedora 44/kernel 7.2.5-200.fc44.x86_64,
OpenShell 0.0.116 and rootless Podman 5.8.4 returned blocking `partial` for the
already stopped local gateway without starting it. Before/after private Dungeon
and OpenShell registration-tree metadata and container identities/names/states
were unchanged. Repeated diagnosis was identical. With the installed native
CLIs but an isolated empty HOME/XDG/project, doctor returned missing/unknown
prerequisites and left those directories empty. No live mount, authentication,
AI task, sandbox creation or containment probe was performed. Sanitized reports,
source/binary hashes and corrected metadata comparisons are retained at
`/tmp/opencode/dungeon-doctor22-K7TkW6`; that pre-review release binary SHA-256 was
`887750363ec58c00ae92c6ac0fabbd5f88d4bdac602b0aec6eda6174200dd716`.

Independent-review corrections added private descriptor-relative config/policy
reads with FIFO/symlink/mode refusal, selected-project helper PATH exclusion,
exact single driver-data environment-assignment observation, and `WNOWAIT`
query-group cleanup after leader exit. A native Podman 5.8.4 `info` probe with
explicit disposable root/runroot initialized an empty storage directory
(including `db.sql`, locks and network/storage directories). Doctor therefore
requires an established regular user-owned nonempty `db.sql` marker before
engine inventory. With the corrected binary and installed native CLIs, an empty
isolated store remained empty and HOME/XDG/project metadata unchanged; engine
checks reported unknown. The scratch engine store was removed. The native
boolean-only inspect template parsed successfully against the older local
container, whose missing driver-data assignment returned `[]` (unknown), not
enabled-compatible. Synthetic cases cover absent/wrong/duplicate assignments
and leader-exited descendants retaining stdout. Corrected source/binary hashes
and comparisons are under `review-*` in the same evidence root; no live mount or
AI/owned-lifecycle matrix was repeated.

## Existing runtime and transfer evidence

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

Issue #24 now has a static pre-validation candidate: schema 2 resolves omitted
project transfer to copy, accepts only copy/live, fingerprints live distinctly,
derives one OpenShell 0.0.116 Podman bind from the canonical configured project
to /sandbox/project with read_only=false and the private SELinux-label candidate,
and keeps upload/export copy-only. Static tests must prove no raw driver-config
or arbitrary mount authority, no live upload on fresh/resumed up, and no policy
widening. This is not live runtime evidence.

The Issue #24 live gate was recorded on 2026-10-01 against Fedora 44, kernel
7.2.5-200.fc44.x86_64, SELinux Enforcing, OpenShell 0.0.116, and rootless Podman
5.8.4. Dedicated gateway `issue24-brntyu` used `enable_bind_mounts=true`,
`userns=keep-id:uid=998,gid=998`, and an identity-mounted `XDG_DATA_HOME`. An
earlier failed attempt is recorded separately below. Source was
`321e4a300fe58997874df0353e3121dc5515ce0e` plus the uncommitted cache fix;
the disposable gateway was provisioned explicitly with the corrected layout,
not by migrating or rerunning `dungeon setup` against `dungeon-local`. The agent
image digest was
`sha256:5a5353c130370361934d4826ee792e2f8038de40763f2713025dbb4ba83e0ce5`.
On the corrected gateway, sandbox
`47ea5729-4105-44de-8b9c-75064709c997` became Ready with no upload. Guest
identity was 998/998. Ordinary host files at 1000:1000 modes 755/644 were
writable from the guest without chmod/chown; both edit directions appeared
immediately and host ownership/mode stayed unchanged. Stop/start reused that
sandbox without upload. Private SELinux labeling relabeled the project to
`container_file_t:s0:c81,c893`; the unconfined host user could still read and
write it, and enforcement stayed on. Opening mode-1777 `/var/tmp` and `/boot`
was denied, while `/tmp` writes and `/etc/passwd` reads succeeded. `/etc/shadow`
reads were denied, but that file is also mode 640. Unapproved
`https://1.1.1.1` and `https://example.com` returned CONNECT 403. No engine
socket, host home, SSH key, credential store, or parent path was accessible.
The only Dungeon-requested writable host bind was the project at
`/sandbox/project`. Live upload and export refused locally. Separate copy
sandbox `ef02d75c-504c-4c60-a429-40d84f2ea8fd` still uploaded once, resumed
without upload, preserved guest-only state, auto-started through `run`, and
exported only after review. Generated/default transfer remains copy. This
record does not promote live, certify other platforms, or prove `sudo` (the
binary was absent).

The historical first Issue #24 live attempt on Fedora 44, kernel 7.2.5-200.fc44.x86_64,
SELinux Enforcing, OpenShell 0.0.116, and rootless Podman 5.8.4 stopped before
sandbox creation. The dedicated gateway was healthy and the offline plan was
correct, but host Podman failed `statfs` on a supervisor path rooted at
`/var/openshell/.local/share/openshell/podman-supervisor/...`. The gateway
container had created that cache internally, but the host Podman service could
not resolve the container-only absolute path.

OpenShell 0.0.116 derives this supervisor cache from `XDG_DATA_HOME`. The
required correction was to give the disposable trusted gateway a private driver-data directory
mounted at the identical absolute host/container path and set
`XDG_DATA_HOME` to that path. No project chmod/chown, policy widening, SELinux
weakening, or additional sandbox-visible mount was permitted. This failed
bootstrap establishes no result for guest identity, workspace writability,
SELinux behavior, containment denials, or copy regression; those gates were
unproven by that attempt. The corrected-gateway run recorded above is separate
evidence and does not turn the failed bootstrap into a successful validation.

### Remote-first service forwarding

A synthetic local run at `e90d24de9d18804a646ee0534b7e99c06588988a` plus
uncommitted cache/forward changes used the same Fedora/kernel/SELinux,
OpenShell 0.0.116, rootless Podman 5.8.4, and agent-image digest as the live gate
above. Dedicated gateways and sandbox `remote-probe` were removed afterward.
The guest ran OpenCode 2.0.14 `serve` on `127.0.0.1:4096`; Dungeon forwarded
host `127.0.0.1:14096` through OpenShell's gRPC service transport.

| Check | Result |
| --- | --- |
| Guest server reachable through host-loopback forwarding | PASS; `/api/info` reported guest OpenCode 2.0.14 and its guest PID |
| Service authentication | PASS; unauthenticated and Bearer requests returned 401; the generated password with HTTP Basic authentication succeeded |
| Remote file write appears immediately on host | PASS; server filesystem API wrote synthetic `probe.txt`, preserving host owner/group/mode |
| Host write appears through remote server | PASS; remote file read returned the host edit |
| Host native client authentication | PASS; client-only `OPENCODE_PASSWORD` authenticated `opencode api --server ... get /api/info` |
| Native-client remote shell edit | PASS; its `/api/shell` request appended to guest `probe.txt`, immediately visible on the host |
| Forward listener removed on interrupt | PASS; host connection failed after SIGINT |
| Native host OpenCode TUI attachment | PASS; host 2.0.22 rendered the guest `/sandbox/project` location when supplied the generated password, with a version warning |
| Interactive TUI shell submission | NOT TESTABLE; the automated terminal input did not establish a command result |
| Explicit guest path passed to host client | FAIL for that invocation; `/sandbox/project` caused a local `chdir` error; omit the positional argument |
| Remote model/AI task, VS Code, Codex remote, Claude remote/desktop | NOT TESTABLE in this run |

The initial forwarding probe above was limited to API/live-file, native-client
shell, and TUI-attachment observations. A subsequent run using the same recorded
Fedora/OpenShell/Podman combination and agent-image digest established:

| Check | Result |
| --- | --- |
| Native remote AI task | PASS; synthetic `add.js` edit and guest test succeeded; separately approved host tests passed before and after restart/resume |
| Restart and resume | PASS; same sandbox ID reused, exact native session history persisted, and a conversation-only synthetic marker was recovered without being re-supplied; guest tests passed |
| Forward lifecycle | PASS; interrupt removed the local listener, reconnect reached the same guest server process, and forwarding a stopped sandbox was refused |
| Configuration and bounded probes | PASS; synthetic default-agent drift was refused; `/var/tmp` write and unapproved network requests were denied, `/tmp` write succeeded, and engine socket paths were absent |
| Non-TTY exec interrupt | DEGRADED; host process-group SIGINT left the guest server running |
| Real-terminal `exec --tty` Ctrl-C | PASS; the foreground command and guest server stopped, and guest port 4096 was absent |
| Guest server after sandbox restart | Explicitly relaunched; restart did not itself promise service supervision |

Interactive TUI message submission remains unverified. Forward interruption
removes only its host listener and does not stop the independently started
service. Do not conflate these distinct shutdown paths. Before setting
`OPENCODE_PASSWORD`, the native client rejected the health response with `UnsupportedContentType` and
showed no password prompt. Authenticated revalidation resolved that failure;
it did not establish a version incompatibility. Client/server versions differed
and the client warned but continued. Keep service authentication enabled and
do not alias guest paths on the host. Prefer matching reviewed versions for
the next matched-version workflow check. The sanitized evidence summary and
separate TTY report were retained locally; this page records the durable claims
without copying raw session or credential material.

Focused mock tests cover nonzero-port parsing, both fixed loopback addresses,
gateway/workspace scope, Ready/identity/policy refusal, unchanged lifecycle,
and faithful forwarding exit status. They do not establish remote UI support.

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

### Issue #28 owned foreground command

`exec --owned` has focused synthetic guest-shim coverage for argv/no-stdin,
nonzero primary exit with a lingering descendant, TERM-resistant work,
concurrent ownership/unrelated sentinel preservation, early/repeated STOP,
absent/stale controls, exclusive creation and substituted-symlink refusal.
Rust tests cover opt-in/non-TTY parsing, normal exit/binding preservation and
prelaunch policy-drift refusal. Run `node --test tests/owned.test.cjs` alongside
the normal Rust/launcher gates. These do not establish live OpenShell cleanup.
The disposable live gate on Fedora 44, kernel 7.2.5-200.fc44.x86_64, SELinux
Enforcing, OpenShell CLI/gateway 0.0.116 and rootless Podman 5.8.4 passed:
normal exit 23 with a TERM-resistant lingering child; PID INT and process-group
TERM; repeated startup INT/TERM; concurrent owned services with unrelated service
preservation; forwarding interruption leaving its independently owned service
alive; primary transport SIGKILL followed by confirmed guest cleanup and error
exit; clean relaunch and synthetic guest/project state preservation. Native
OpenCode 2.0.14 foreground `serve` stopped on TERM/INT, relaunched cleanly and
retained its database file. This is not native conversation/session-resume
validation, which remains Issue #29.

Pausing the dedicated gateway during shutdown produced bounded nonzero
`cleanup unconfirmed`; the guest service remained alive after transport restored.
Synthetic binding drift likewise refused cleanup and left the original service
alive. After restoring the test prerequisites, explicit invocation STOP/receipt
removal cleaned up both fixtures. No temporary controls remained. All disposable
sandboxes/gateways/registrations were removed; reviewed agent digest remained
`sha256:5a5353c130370361934d4826ee792e2f8038de40763f2713025dbb4ba83e0ce5`.
The source was `d8e269eff46c5c6815344dd0c0abd650b3beaad9` plus the owned-execution
diff recorded in the local evidence roots. No credentials, host session state,
policy widening or tool upgrades were used. Host SIGKILL, same-UID sabotage,
group escape and other runtime/platform combinations remain unproven/unsupported
as described in configuration; this is lifecycle assistance, not containment.

Independent-review corrections additionally verified guardian SIGKILL after
STOP and after primary exit status: a resistant group member stayed alive, no
completion receipt was emitted, and Dungeon returned bounded unconfirmed error.
The outer wrapper now requires kernel group absence rather than treating any
guardian SIGKILL as its final group kill. Combined host INT plus primary-transport
SIGKILL returned a transport error, not 130. Affected live normal-exit/lingering
child and ordinary INT/TERM gates passed again; temporary controls were removed.
The earlier live matrix is evidence for the preceding revision, not a repeated
full matrix on this correction. Synthetic tests also cover guardian faults and
primary transport status independently. Zombie-only groups may conservatively
refuse confirmation until reaped; no signal is sent to a reaped guardian's group.

### Issue #29 matched OpenCode pair (2026-10-03)

A separate disposable run passed with **host and guest OpenCode 2.0.14** on
Fedora 44, kernel 7.2.5-200.fc44.x86_64, SELinux Enforcing, OpenShell
CLI/gateway 0.0.116 and rootless Podman 5.8.4. The existing reviewed agent
image was selected only by immutable digest
`sha256:5a5353c130370361934d4826ee792e2f8038de40763f2713025dbb4ba83e0ce5`;
the dedicated gateway image was
`sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645`.
Neither image was rebuilt, and the installed host client was not replaced.
Rootless Podman `create`/`cp` extracted the image's native client into a private
`/tmp/opencode` directory. Its SHA-256, also verified inside the guest, was
`78accad0f9fa61f681c4e0342ce4960ab784d27ddf34796ce12d156f9b5c3abb`.

The exact official registry tarball for `@opencode/cli-linux-x64@2.0.14`
matched the checked-in lock's SHA-512 integrity
`sha512-YFGnck40hBmD8S785zHi1sCZMVodoyxy615SrEo+YXJj/i92I8ViETodsY6Yde0H0VJk/ZuLu8mrllOA4wbAlQ==`.
Its binary matched the extracted image and guest hashes. Registry metadata
identified the anomalyco/opencode repository and GitHub Actions trusted
publisher; signatures/provenance attestations were **not independently verified**.
The reviewed V2 source tag `v2.0.14` resolves to
`08462140ec0de1e4b17d4a353d8d5827f53cf7b0`; the native explicit-server,
session-target and foreground-password behavior was checked against that source
and the V2 CLI/API documentation.

Dungeon source was `d8e269eff46c5c6815344dd0c0abd650b3beaad9` with the accepted
Issue #28 review corrections. The tested debug binary SHA-256 was
`abccbb33b157844d4b7a941dea4a1d678dc385d7101ad3704c3cae49da154dfa`.
The runtime-source diff hash (tracked `src`/Cargo diff plus the named untracked
owned sources) was
`98e8ec9418323ea0adcf30b19cca524238ecc18fe9fabc1609f1e49da9a1bfcf`;
the complete pre-documentation diff plus named untracked-file contents hashed to
`9118c51245925cfc58d5e558cff3ebe66027625e3f3927ec8cd26e54e9112b19`.
Per-file hashes confirmed runtime/image inputs remained unchanged during this run.

The gateway used private XDG registrations, mTLS and the corrected identity-mounted
driver-data layout. Host client HOME and every XDG state directory were isolated
from real host state. The project/config were synthetic. Native guest `serve`
ran through `dungeon exec --owned`; generated passwords were consumed in memory
and supplied only to the host client through ephemeral `OPENCODE_PASSWORD`.
No password was passed in argv or written to configuration/evidence. The native
host client used explicit `--server`, with no positional guest path.

| Check | Result |
| --- | --- |
| Exact host/guest version, binary identity and native authentication | PASS; both printed `opencode v2.0.14`; unauthenticated health returned 401; native authenticated API succeeded |
| Actual native remote AI read/edit/test | PASS; guest-listed `opencode/mimo-v2.6-flash-free` completed the synthetic arithmetic fix and guest test; no account or host auth imported |
| Host-visible edit and approved host tests | PASS; arithmetic-only output was checked before host execution; test unchanged and original owner/group/modes preserved |
| Independent forward interrupt/reconnect | PASS; process-group INT removed the host listener; reconnect reached the same guest service PID |
| Owned guest-service shutdown/relaunch | PASS; PID TERM returned 143 and PID INT returned 130, each with direct guest-listener absence; clean relaunch retained the native session |
| Same bound sandbox restart and native history | PASS; identical sandbox/session IDs, guest location `/sandbox/project`, project state and exact message history survived explicit restart/relaunch |
| Same-conversation recovery | PASS; native resume recovered a conversation-only synthetic marker without re-supplying it; guest and approved host tests passed again |
| Stopped forwarding and bounded denials | PASS; stopped sandbox refused forwarding; `/var/tmp` write denied, `/tmp` allowed, unapproved example.com CONNECT returned 403, checked engine sockets absent |
| Cleanup | PASS; owned controls absent; disposable sandbox, gateway, network, private registrations, credentials, client state and project removed; host listeners absent |

The model selection was a validation-client argument, not Dungeon product policy.
The prior Issue #28 generic fault matrix is not claimed as rerun here. Interactive
TUI message submission and other versions/platforms remain unverified. Sanitized
summaries, artifact/source hashes and provenance are retained locally at
`/tmp/opencode/dungeon-matched29-V9AyIi/evidence`; no whole conversations,
passwords, raw prompt files or client databases are retained.

### Issue #30 editor prerequisite checkpoint (2026-10-03)

**VS Code Remote SSH: NOT TESTABLE.** After the matched-pair gate, neither
`code`, `code-insiders` nor `codium` was on PATH; RPM and Flatpak inventories
showed no matching application. Checked standard installation/extension
directories were absent, including Remote SSH extension roots. Neither DISPLAY
nor WAYLAND_DISPLAY was set in this session. A supported installed GUI client,
Remote SSH extension, usable interactive display and explicit installation
approval if needed are prerequisites. Nothing was installed.

Host Neovim 0.11.6 and OpenSSH 10.2p1 are available. A separately chosen bounded
interactive Neovim/SSH workflow could be useful, but was not exercised and would
not substitute for a VS Code pass. No editor/vendor authentication, host-home
sharing or policy widening was attempted.

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

Creating the reviewed version tag is the explicit native-release authorization step. npm publication remains a separate future gate. Runtime support claims remain limited to the exact validated combinations. Issue #5's read-only configuration imports remain a separate implementation and validation gate after the Issue #17 image-owned tool prerequisites. The Issue #24 live mount has the recorded Fedora/OpenShell/Podman evidence above; generated/default transfer remains copy until an explicit promotion decision.
