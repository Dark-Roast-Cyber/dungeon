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

Host Neovim 0.11.6 and OpenSSH 10.2p1 were available. That checkpoint did not
exercise an editor. The separate native SSH/guest Neovim run below does not
substitute for a VS Code pass.

### Issue #30 native SSH terminal editor (2026-10-03)

**Partial gate: interactive guest Neovim PASS; disconnect cleanup DEGRADED.**
Dungeon source was `3965d7d34e6e0ee28483fc0e68ebc708ef880c48`, with unchanged
runtime/image inputs. The tested debug binary SHA-256 was
`58e1d2065d632601045daba35877b809fcd2aa4cdafe9f8744b93a0124a5ae0e`.
The run used Fedora 44, kernel 7.2.5-200.fc44.x86_64, SELinux Enforcing,
OpenShell CLI/gateway 0.0.116, rootless Podman 5.8.4, host OpenSSH 10.2p1 and
image-owned guest Neovim 0.12.5. The unchanged immutable agent and gateway
digests were respectively
`sha256:5a5353c130370361934d4826ee792e2f8038de40763f2713025dbb4ba83e0ce5` and
`sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645`.

Pinned upstream [SSH configuration/proxy source](https://github.com/NVIDIA/OpenShell/blob/v0.0.116/crates/openshell-cli/src/ssh.rs)
and [supervisor SSH source](https://github.com/NVIDIA/OpenShell/blob/v0.0.116/crates/openshell-supervisor-process/src/ssh.rs)
plus exact installed help established native remote-command/PTY support. The
emitted `dungeon ssh-config` block matched the reviewed fixed native ProxyCommand
using the installed canonical OpenShell path, test gateway/name and workspace.
Only that explicitly trusted SSH mechanism executed on the host; no arbitrary
output or project snippet was evaluated. Native `ssh -F` used a private 0600
file, isolated HOME/XDG registration, `User sandbox`, no identity files/agent,
no public-key/password/keyboard-interactive auth, no forwarding/multiplexing and
no local commands. OpenShell's mTLS-backed session proxy supplies native SSH
none-auth access; no host SSH credentials were needed. Upstream disables host-key
checking/known-host storage for this proxy path; this is not ordinary SSH host-key
validation. No `~/.ssh` modification occurred.

The synthetic live project used its own disposable gateway with keep-id and
identity-mounted driver data. A foreground `uv run` Python harness allocated a
real controlling PTY (`pty.fork`) to native host OpenSSH, which launched guest
`nvim -u NONE -i NONE` in `/sandbox/project`. This was not headless Neovim or a
host Neovim UI/config import.

| Check | Result |
| --- | --- |
| Interactive rendering/input/edit/save | PASS; terminal screen rendered the fixture and accepted an arithmetic substitution and `:w` |
| Guest context and test | PASS; `/sandbox/project`, `HOME=/sandbox`, UID 998 and terminal input confirmed; `:!node test.js` printed `tests passed` |
| Host-visible edit and approved host test | PASS; exact arithmetic-only fix inspected before host execution; unchanged test SHA-256 `702e699d68d016175147d366080cdff34768f1564505b4eb0f306a06622216ba`; owner/group/modes preserved |
| Editor state | PASS; synthetic state under `/sandbox/.local/state/nvim`, owner/group 998:998, mode 0700, persisted after restart; data/cache directories were absent under this minimal invocation |
| SSH disconnect lifetime | DEGRADED; closing the host PTY returned SSH exit 255 but two guest Neovim processes remained sleeping; disconnect is not editor shutdown |
| Reconnect and same sandbox stop/start | PASS; saved edit reopened through SSH, same sandbox ID reused after stop/start, guest test passed again; no unsaved-buffer recovery claim |
| Bounded denials | PASS; SSH `/var/tmp` write denied, `/tmp` write allowed, unapproved example.com CONNECT returned 403, checked engine sockets absent; no credential reads |
| Cleanup | PASS; stopping only the dedicated test sandbox removed surviving editor processes; final guest editor inventory empty; disposable sandbox/gateway/network/private registrations/certificates/database/HOME/project/config removed, gateway port absent |

Existing container identities/names/states matched before/after. No tools were
installed/upgraded, images rebuilt, host configuration/credentials shared, policy
widened or live default promoted. Sanitized gates, binary/upstream/harness/source
hashes and cleanup comparisons are retained at
`/tmp/opencode/dungeon-ssh30-73pUNm/evidence`; no raw terminal/auth output or
editor/session databases are retained.
VS Code, host editor UI, vendor remote/desktop clients, other platforms and full
Issue #30 acceptance remain unproven.

### Owned interactive SSH implementation

The opt-in `ssh --owned -- COMMAND ARG...` candidate was exercised against
`df349cfac7f46544d6e853e646eb73a8d24caad2` plus the recorded implementation diff,
on Fedora 44/kernel 7.2.5-200.fc44.x86_64, SELinux Enforcing, OpenShell CLI/gateway
0.0.116, rootless Podman 5.8.4, host OpenSSH 10.2p1 and guest Neovim 0.12.5.
The unchanged agent/gateway image digests were respectively
`sha256:5a5353c130370361934d4826ee792e2f8038de40763f2713025dbb4ba83e0ce5` and
`sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645`.
Dedicated synthetic gateway/project/HOME/XDG resources used the reviewed keep-id
and identity-mounted driver-data layout. No credentials or real host editor state
were imported; no images/tools/policy/defaults were changed.

| Gate | Result |
| --- | --- |
| Native Neovim render/input/Ctrl-C and guest context | PASS; Ctrl-C canceled editor input without forcing guardian shutdown; `/sandbox/project`, HOME `/sandbox`, TTY confirmed |
| Native raw host terminal/foreground group and resize | PASS; child SSH was foreground, ICANON/ECHO disabled, 100x40 reached Neovim; original group/termios restored on intact-terminal exits |
| Edit/save/guest and approved host test | PASS; arithmetic-only fix; unchanged test SHA-256 `702e699d68d016175147d366080cdff34768f1564505b4eb0f306a06622216ba`; host UID/GID/modes preserved |
| Saved file/private state and clean relaunch | PASS; editor saved file/state retained, normal exit 0; no unsaved recovery claim |
| Normal exit 23 with TERM-resistant descendant | PASS; exact application status, kernel group absence and control removal before sandbox deletion |
| Repeated HUP/INT/TERM and startup INT/TERM | PASS; confirmed host policy returns 129/130/143, intact terminal restored |
| SSH PID SIGKILL and STOP/CONT | PASS; confirmed invocation cleanup remains a transport error, not overridden by receipt |
| Physical host PTY master closure | PASS bounded error; cleanup controls absent; terminal restoration impossible and reported as error |
| Concurrent invocations/unrelated service | PASS; stopping one preserves the other and existing non-TTY owned sentinel |
| Argument transport and missing executable | PASS; empty/quoted/newline/flag args remain data; acknowledged spawn failure returns 127 |
| Non-TTY and changed binding | PASS refusal; binding drift leaves controls/work for exact invocation recovery after restoring prerequisites |
| Live narrowed-policy drift | PASS refusal; original policy restored before exact invocation STOP/receipt removal; no policy widening |
| Paused gateway | PASS bounded unconfirmed error; known fixture STOP/receipt removal succeeded after gateway recovery |
| Guardian/observer death | PASS nonzero refusal, never normal success; known synthetic application identity checked before manual recovery; group absence confirmed separately |

The initial physical-PTY test exposed missing native hangup observation; the host
now polls only terminal error/hangup state, without reading input/proxying bytes.
The corrected main matrix passed. An initial death fixture could exit on native
SIGHUP and become a zombie before manual evidence collection; corrected fixtures
catch HUP/TERM and recovery checks distinguish zombies. The affected death gates
passed separately. Sanitized matrices, tested-source/binary hashes and exact
revision boundaries are retained at
`/tmp/opencode/dungeon-owned-ssh-live-D9CMMw/evidence`. The earlier scratch proof
is mechanism evidence, not substituted for these implementation runs.

A maturity regression with a deliberately withheld status acknowledgement exposed
that timeout shutdown must also mark the observer receipt as a fault. That
correction and its bounded synthetic test passed; the final main/fault/policy
matrices ran with identical runtime inputs and debug binary SHA-256
`9458a53f89db64ee5717ef97fb0df0b8118c3ab8a841cfec4c5bee08284ace8a`.
One earlier final-input normal-exit attempt returned nonzero before the receipt
was captured; its cause was not established. The next main run and three focused
normal/resistant-group confirmations passed. Conservative group/zombie refusal
remains a documented limit, not a reason to reinterpret that failed attempt as a
pass. The default parallel Rust run also encountered an unrelated diagnostics
fixture result (`unknown` instead of `incompatible`); its isolated rerun and the
full serial suite passed without changes to diagnostics at that checkpoint.

Independent review then required acknowledgement ordering, final tail-signal
handling and portable host Node fixtures. The corrected guardian now versions
acknowledgements: late HUP/TERM faults supersede a pending normal completion.
The host decides status after transport teardown/terminal restoration, with
handlers still active; unacknowledged late signals error. Host tests resolve Node
from sanitized project-excluded PATH; the production guest path stays `/usr/bin/node`.
Delayed-ack HUP/TERM and injected teardown-tail signal regressions passed.

On the same recorded runtime combination, the corrected-tree focused live run
passed late guardian HUP/TERM after normal finish with observer acknowledgement
delayed. Receipts retained code 23 **and** fault=true; Dungeon returned error 1,
restored the terminal and removed controls. Three normal23/resistant-group runs
also passed with sanitized fixed stderr categories, receipt-presence/status flags,
live/zombie group counts and host SSH states sampled during cleanup. The original
unclassified normal failure did not recur; its cause remains unknown, not inferred
from these successful observations. Evidence is at
`/tmp/opencode/dungeon-owned-ssh-review-Qt9vpY/evidence`; tested debug SHA-256 is
`4c78a398607a7ff6488fb242c037826766ba03d86596879a4ab736654d7c44f2`.
The broader matrix above remains evidence for its earlier exact revision, not
a claimed full rerun after these corrections. Tail-signal injection is a unit
regression, not a live timing claim.

The default parallel suite reproduced a diagnostics `unknown` result in another
fixture. A bounded fresh-script/parallel-fork reproduction captured spawn errno
26 (`ETXTBSY`): inherited writable descriptors can temporarily keep the script
text busy until exec closes CLOEXEC descriptors. Diagnostics now retries only
that spawn error within the original query budget, subtracting startup time from
collection; no other error is retried. The targeted regression and default
parallel `cargo test --locked --quiet` passed (75 unit + 2 integration tests).
Formatting, Clippy, release build, all seven owned Node tests, three npm launcher
tests and the npm staging test passed. No test skip, serialized CI or widened
deadline was introduced. Exact historical assertion failures lack errno capture
and are not individually retroactively classified.

All disposable sandboxes/gateways/networks/private registrations/certificates/
databases/HOME/project/configuration were removed; preexisting container
ID/name/state inventories matched and dedicated port 18696 was absent. Invocation
controls/group absence were checked before final sandbox deletion for successful
gates; whole-sandbox deletion is not invocation confirmation. Other platforms,
group escapes/job-control shells, same-UID sabotage, host SIGKILL, VS Code and
vendor desktop clients remain unsupported/unproven. Generic SSH's earlier
disconnect degradation is unchanged.

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
