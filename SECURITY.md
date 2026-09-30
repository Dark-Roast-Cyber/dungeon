# Security boundary

Dungeon 0.0.x is an engineering preview and has not undergone an independent security audit. Issue #1 performed live synthetic validation of one OpenShell 0.0.116 + rootless Podman 5.8.4 combination; that evidence does not generalize to other drivers, platforms, images, desktop clients, or EDR products.

Trust the host administrator, Dungeon binary/configuration, OpenShell CLI/gateway/driver, selected image, and explicitly exposed credential/tool services. Treat the coding agent, project content, downloaded code, and agent-authored session records as untrusted.

OpenShell owns filesystem/process/network enforcement. Dungeon requires Landlock `hard_requirement` in accepted policy and refuses inspected audit-only endpoints, but policy readback is not kernel attestation.

Dungeon clears the inherited environment before invoking OpenShell and restores only an explicit allowlist. It does not automatically mount/copy host homes, engine sockets, native agent databases, private keys, or provider secrets.

Project selection is launch context, not security authority. The low-friction
launcher canonicalizes the selected/current directory and derives a private
Dungeon-owned config location from that path. It never loads policy, transfer
mode, raw mount data, or credentials from repository files. Renamed/moved
projects are not silently rebound.

## Credentials

OpenShell providers can grant authority even when raw secrets are brokered. `dungeon auth` starts vendor-owned login inside the guest. Resulting OAuth credentials live in guest-controlled state. Dungeon does not inspect, copy, refresh, escrow, or synchronize them.

## Remaining risks

- Allowed endpoints can receive data.
- In copy mode, first `up` uploads the configured project once. Resume and automatic `run` startup do not upload. Explicit `upload --yes` is a deliberate refresh/recovery operation that overwrites guest project contents and may destroy guest-side edits; upload is not a secret scanner or immutable snapshot.
- In copy mode, `down` stops the sandbox without deleting its guest state. Export is explicit, only available while the bound sandbox is Ready under OpenShell 0.0.116, and stages a downloaded guest tree for review.
- The explicit live-mode candidate intentionally exposes the canonical configured project read-write at `/sandbox/project`. It does not expose arbitrary mount authority, host home, engine sockets, credential stores, or additional writable host paths. Live upload/export are refused. This path is not supported/default until the OpenShell/rootless-Podman/Fedora ownership, SELinux, policy-denial, and host-visibility validation gate passes.
- Upload may filter ignored files using gitignore rules and leaves unrelated existing guest entries in place; export downloads the complete guest tree. Dungeon records a common upload baseline only for pre-upload host entries that exactly match post-upload guest entries (type, content hash, and Unix executable bit). Differing or absent paths are not asserted as successfully transferred common state. Export compares this baseline, downloaded guest tree, and current host tree. Host-only files are preserved; differing changes on both sides of a baseline path are conflicts and are refused. Old configurations without a baseline are refused rather than guessed: manually download/recover the guest project with OpenShell, inspect it, then deliberately re-upload with `upload --yes` to establish a new baseline.
- Establishing an export-ready baseline now requires a symlink-free host and guest project. Earlier upload preflight permitted internal symlinks; this stricter first-slice contract refuses them rather than silently baselining unsupported link semantics. Quiesce the agent before exporting: OpenShell's directory download is not a point-in-time snapshot of concurrent guest writes.
- Do not run concurrent upload/export commands against one environment. Dungeon rechecks host contents and sandbox identity but does not serialize separate Dungeon processes or guarantee crash/power-loss atomicity. Failed/interrupted apply requires operator inspection of the private recovery directory.
- Export requires interactive typed `yes` or explicit `--yes` after a deterministic change summary. It rejects special entries (including symlinks), and applies no metadata beyond regular file contents, directories, and the Unix executable bit. Other permissions, ownership, timestamps, ACLs, and extended attributes are not preserved. If applying changes fails, backups of affected host files are kept privately adjacent to the project for manual recovery; recovery is not automatic or transactional.
- Guest/session durability is backend-specific.
- Mutable images/provider content can change without changing Dungeon's fingerprint.
- An optional private Neovim `imports` declaration is plan-only. Runtime operations refuse it: uploading into writable `/sandbox` as the sandbox user does not provide guest read-only enforcement. Path/name checks are not a secret scanner; inspect any proposed host input before enabling a future import implementation.
- The [local agent image candidate](images/agent/README.md) pins its base and agent distributions but inherits upstream base layers, including Node 22.22.1 with reported security-update debt. Local testing is not an image audit or publication approval.
- Inside the tested OpenShell Podman guest, Codex's inner `bwrap` sandbox could not start. Explicit one-time approval ran the synthetic task outside Codex's sandbox, still within OpenShell's hard policy; do not mistake this for two working layers of containment.
- Host EDR does not imply guest-process visibility.
- Telltale integration is intentionally deferred until Telltale 0.7.0/Event4 stabilizes the downstream contract.
- npm scope ownership, platform package provenance, and publishing credentials must be validated before npm publication.
