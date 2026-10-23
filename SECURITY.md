# Security boundary

## Reporting

Do not disclose a suspected vulnerability in a public issue, pull request, or discussion. If the repository Security tab shows **Report a vulnerability**, use that private form. Otherwise send the report privately to `public@christiant.io`, the maintainer contact on the [Dark Roast Cyber profile](https://github.com/Dark-Roast-Cyber). Include a minimal synthetic reproduction, the affected commit or tag, and the impact. Do not send credentials, tokens, session stores, or live project data. No response deadline is promised. Fixes are evaluated for current `main` and the latest published release, when one exists.

Dungeon 0.0.x is an engineering preview and has not undergone an independent security audit. Issue #1 performed live synthetic validation of one OpenShell 0.0.116 + rootless Podman 5.8.4 combination; that evidence does not generalize to other drivers, platforms, images, desktop clients, or EDR products.

Trust the host administrator, Dungeon binary/configuration, OpenShell CLI/gateway/driver, selected image, and explicitly exposed credential/tool services. Treat the coding agent, project content, downloaded code, and agent-authored session records as untrusted.

OpenShell owns filesystem/process/network enforcement. Dungeon requires Landlock `hard_requirement` in accepted policy and refuses inspected audit-only endpoints, but policy readback is not kernel attestation.

Dungeon clears the inherited environment before invoking OpenShell and restores only an explicit allowlist. It does not automatically mount/copy host homes, engine sockets, native agent databases or private keys. Reviewed OpenCode configuration access can expose provider secrets as described below.

`forward` exposes an explicitly selected guest-loopback service on host
`127.0.0.1` through OpenShell's authenticated transport, after checking bound
identity and policy. It is not an authentication layer for that service. Keep
vendor authentication enabled, trust the connecting client, and stop the
foreground forward when finished. Other local users/processes can reach host
loopback; an agent API can execute commands and write the live project. Dungeon
does not store service passwords or publish listeners on external interfaces.

`exec --owned` is opt-in Linux foreground lifecycle assistance, not stronger
containment. A live guest guardian signals only its own process group; command
output and user-supplied PIDs never authorize signalling. Cleanup is bounded and
refuses changed identity/policy. Host SIGKILL, transport loss, same-UID guest
sabotage and process-group escape can leave work running; unconfirmed cleanup
is an error.
External/OOM guardian death is not proof of cleanup: kernel group absence is
required, with conservative refusal for unreaped zombies or recycled group IDs.
Guardian faults terminate the non-TTY wrapper even if surviving work retains
application output pipes; no cleanup receipt is fabricated, and work may remain.
Failed primary transport remains an error even alongside a pending host signal.
Forced group KILL can affect application durability but does not
delete project/session state. Temporary guest control files are not a persistent
PID registry or credential store. See [configuration](docs/configuration.md#owned-foreground-execution).

`ssh --owned` adds opt-in interactive group ownership, not stronger containment.
It accepts only the exact pinned OpenShell SSH configuration template before
using a private host file; no host identities, agent, forwarding, multiplexing
or local commands are enabled. OpenShell's proxy path uses native none-auth SSH
behind mTLS and disables host-key checking; this is not ordinary SSH host-key
validation. The foreground guest guardian retains the native terminal; a silent
invocation-local observer records kernel group absence separately from output.
Failed/stopped/continued SSH and guardian faults remain errors even after cleanup.
Physical terminal loss triggers cleanup but prevents confirmed terminal restoration.
Group escape, same-UID sabotage, host SIGKILL and unavailable cleanup transport
retain the non-TTY ownership limits. Saved state is not unsaved-buffer recovery.

Project selection is launch context, not security authority. The low-friction
launcher canonicalizes the selected/current directory and derives a private
Dungeon-owned config location from that path. It never loads policy, transfer
mode, raw mount data, or credentials from repository files. Renamed/moved
projects are not silently rebound.

Human `init` requires one scope confirmation or explicit `--yes` before writing
private configuration. It never starts a sandbox or uploads files. Implicit
first-use launch binding still requires a terminal. `plan` and `config` only
inspect existing configuration and cannot create authority. Initialization
validates config and resolved destination scope before creating files; this is
not a lock against concurrent host path replacement.

## Local setup

Fresh `setup` enables trusted local gateway driver bind capability by default;
`setup --copy-only` retains a gateway without bind capability. Setup grants no project mount authority,
changes no policy and converts no private project configuration. The driver maps
the developer to sandbox UID/GID 998; gateway/certificate generation retains
plain `keep-id`. Exact copy gateways refuse migration; exact live gateways are
verified without downgrade. Unknown/partial/legacy state is refused, not repaired.
The fresh CLI path has one disposable Fedora CoreOS/Podman native record, not
general platform attestation. Failed/interrupted setup may
leave partial new resources; no destructive rollback, blind retry or automatic
adoption is promised. Dry-run uses bounded inventory only and refuses missing,
empty or custom engine storage instead of initializing it.

## Credentials

Machine schema 1 emits fixed diagnostics, not native stderr, policy/provider JSON
or parser error text. Requested workload bytes are intentionally lossless and
unredacted, including secrets and terminal control sequences; consumers must treat
them as untrusted. Lexically checked guest cwd is not symlink confinement. FIFO/socket
backpressure/cancellation and independent owned receipts assist lifecycle only;
they do not add containment or guarantee final event delivery. See the
[machine contract](docs/machine.md).

OpenShell providers can grant authority even when raw secrets are brokered. `dungeon auth` starts vendor-owned login inside the guest. Resulting OAuth credentials live in guest-controlled state. Dungeon does not inspect, copy, refresh, escrow, or synchronize them.

New human OpenCode bindings propose read-only access to selected native config
files, plugins/themes and legacy `auth.json` in their initialization review.
Existing bindings and machine initialization do not gain access automatically.
Read-only prevents guest writes to mounted contents, **not reading, using or
exfiltrating credentials**. Plugins/configuration execute inside the guest with
its authority. Neither mode scans arbitrary content for secrets.

The source-only first slice derives individual file mounts, never a host home,
config parent or data-directory mount. Live sources are not relabeled; SELinux
denial is not permission to disable enforcement. Copy mode captures private
files after explicit `reload` approval and may relabel only those private copies.
Excluded and known credential/native-state paths, including all `.env` names,
are skipped before content reads. Auth exclusion conservatively omits whole
OpenCode JSON config files that could mix credentials with ordinary settings;
it cannot sanitize secrets embedded in arbitrary plugin code or other content.

Nonempty original-source imports conservatively refuse enforcing SELinux and
unknown/unreadable enforcement state before runtime contact; custom labels are
not an exception. No automatic copy fallback or host relabeling occurs. Empty,
copy and disabled selections do not require that check. Status/stop/owned cleanup
remain import-independent; image-owned native parents stay guest-private/writable.

OpenCode 2.1.2 stores current credentials in its native database. That database
is never mounted, copied, queried or synchronized by this feature. Legacy
`auth.json` visibility is not current-account sharing or ongoing refresh: the
fresh native fixture returned an empty credential list despite a readable file.
Do not infer authentication from a file mount. Read-only auth may
prevent login refresh, and snapshots may become stale. Live file replacement
may leave an older inode mounted. `reload` only reselects unbound configuration;
it refuses any runtime binding and does not restart services, rotate guest
tokens or write back to the host. Writable guest ancestors still allow pathname
replacement; this is not trusted-execution control. Native enforcement,
guest-state writability and authentication remain separate validation gates.

## Remaining risks

- Fresh copy `up` publishes pending initialization atomically with runtime identity and holds admission/transfer locks through upload and baseline completion. Pending blocks new work, not status/stop/explicit start or existing owned cleanup. Failure/interruption requires deliberate `upload --yes` recovery; no automatic retry. A failed completion directory sync can leave completed state visible and reports persistence unconfirmed, not rollback. This is not power-loss durability or a lock against external guest/host writers.

- Allowed endpoints can receive data.
- In copy mode, first `up` uploads the configured project once. Resume and automatic `run` startup do not upload. Explicit `upload --yes` is a deliberate refresh/recovery operation that overwrites guest project contents and may destroy guest-side edits; upload is not a secret scanner or immutable snapshot.
- In copy mode, `down` stops the sandbox without deleting its guest state. Export is explicit, only available while the bound sandbox is Ready under OpenShell 0.0.116, and stages a downloaded guest tree for review.
- A newly initialized project defaults to an explicitly reviewed live read-write mount of the canonical project at `/sandbox/project`; guest writes affect host files immediately. It does not expose arbitrary mount authority, host home, engine sockets, credential stores, or additional writable host paths. Live upload/export are refused. Existing configs without a transfer field remain copy; explicit `--project-transfer copy` keeps reviewed upload/export. The OpenShell/rootless-Podman/Fedora ownership, SELinux, policy-denial, and host-visibility gate has one recorded result, not general containment validation.
- Upload may filter ignored files using gitignore rules and leaves unrelated existing guest entries in place; export downloads the complete guest tree. Dungeon records a common upload baseline only for pre-upload host entries that exactly match post-upload guest entries (type, content hash, and Unix executable bit). Differing or absent paths are not asserted as successfully transferred common state. Export compares this baseline, downloaded guest tree, and current host tree. Host-only files are preserved; differing changes on both sides of a baseline path are conflicts and are refused. Old configurations without a baseline are refused rather than guessed: manually download/recover the guest project with OpenShell, inspect it, then deliberately re-upload with `upload --yes` to establish a new baseline.
- Establishing an export-ready baseline now requires a symlink-free host and guest project. Earlier upload preflight permitted internal symlinks; this stricter first-slice contract refuses them rather than silently baselining unsupported link semantics. Quiesce the agent before exporting: OpenShell's directory download is not a point-in-time snapshot of concurrent guest writes.
- Copy upload/export use nonwaiting native file locks for the environment and exact canonical host project, including different config paths under the same HOME/XDG configuration authority. Locks cover baseline reads/invalidation, transfer, review, apply and baseline replacement. Private lock state is never adopted, chmod-repaired or unlinked. This is Unix-host serialization, not crash/power-loss atomicity; native Windows copy transfer refuses without equivalent private-file protections. Nested overlapping projects, different HOME/XDG authorities, project moves, external host writers and concurrent guest writes are outside this lock contract. Quiesce those writers explicitly.
- Prepared export recovery is retained before the first host mutation, including on panic or process interruption. Matching unfinished recovery blocks both upload and export. Unknown, legacy or unsafe `dungeon-recovery-*` candidates conservatively block every project sharing that parent until manually inspected; there is no automatic migration, rollback or cleanup. Recovery scanning is bounded to 4096 parent entries and 1 MiB per marker, with no-follow private regular-file checks. After all host changes complete, baseline replacement can occur before success is reported: interrupted replacement or a failed directory sync requires inspection of both host and baseline, not an assumption that the baseline is unchanged.
- Export requires interactive typed `yes` or explicit `--yes` after a deterministic change summary. It rejects special entries (including symlinks), and applies no metadata beyond regular file contents, directories, and the Unix executable bit. Other permissions, ownership, timestamps, ACLs, and extended attributes are not preserved. If applying changes fails, backups of affected host files are kept privately adjacent to the project for manual recovery; recovery is not automatic or transactional.
- Guest/session durability is backend-specific.
- Mutable images/provider content can change without changing Dungeon's fingerprint.
- Optional Neovim imports capture explicitly selected ordinary user-owned files into a private reviewed snapshot and request a read-only bind on the exact recognized local bind-enabled UID998 gateway. The original source is never mounted or relabeled. Filesystem/name checks are not a secret scanner; inspect all input yourself. Lua may execute inside the guest, never on the host. Read-only enforcement protects **mounted contents only**: the guest can rename writable `/sandbox/.config`, create replacement `/sandbox/.config/nvim`, and select different configuration for later execution. This is not trusted-execution control or an immutable pathname guarantee. No plugins are installed or network policy widened. Source drift does not refresh the snapshot. Snapshot alteration blocks new work but does not block identity-checked status/stop or owned-command cleanup.
- The [local agent image candidate](images/agent/README.md) pins its base and agent distributions but inherits upstream base layers, including Node 22.22.1 with reported security-update debt. Local testing is not an image audit or publication approval.
- Inside the tested OpenShell Podman guest, Codex's inner `bwrap` sandbox could not start. Explicit one-time approval ran the synthetic task outside Codex's sandbox, still within OpenShell's hard policy; do not mistake this for two working layers of containment.
- Host EDR does not imply guest-process visibility.
- Telltale integration is intentionally deferred until Telltale 0.7.0/Event4 stabilizes the downstream contract.
- npm scope ownership, platform package provenance, and publishing credentials must be validated before npm publication.
