# Security boundary

Dungeon 0.0.x is an engineering preview and has not undergone an independent security audit. Issue #1 performed live synthetic validation of one OpenShell 0.0.116 + rootless Podman 5.8.4 combination; that evidence does not generalize to other drivers, platforms, images, desktop clients, or EDR products.

Trust the host administrator, Dungeon binary/configuration, OpenShell CLI/gateway/driver, selected image, and explicitly exposed credential/tool services. Treat the coding agent, project content, downloaded code, and agent-authored session records as untrusted.

OpenShell owns filesystem/process/network enforcement. Dungeon requires Landlock `hard_requirement` in accepted policy and refuses inspected audit-only endpoints, but policy readback is not kernel attestation.

Dungeon clears the inherited environment before invoking OpenShell and restores only an explicit allowlist. It does not automatically mount/copy host homes, engine sockets, native agent databases, private keys, or provider secrets.

## Credentials

OpenShell providers can grant authority even when raw secrets are brokered. `dungeon auth` starts vendor-owned login inside the guest. Resulting OAuth credentials live in guest-controlled state. Dungeon does not inspect, copy, refresh, escrow, or synchronize them.

## Remaining risks

- Allowed endpoints can receive data.
- First `up` uploads the configured project once. Resume and automatic `run` startup do not upload. Explicit `upload --yes` is a deliberate refresh/recovery operation that overwrites guest project contents and may destroy guest-side edits; upload is not a secret scanner or immutable snapshot.
- `down` stops the sandbox without deleting its guest state. There is no project export yet, so guest changes are not automatically returned to the authoritative host project.
- Guest/session durability is backend-specific.
- Mutable images/provider content can change without changing Dungeon's fingerprint.
- The [local agent image candidate](images/agent/README.md) pins its base and agent distributions but inherits upstream base layers, including Node 22.22.1 with reported security-update debt. Local testing is not an image audit or publication approval.
- Inside the tested OpenShell Podman guest, Codex's inner `bwrap` sandbox could not start. Explicit one-time approval ran the synthetic task outside Codex's sandbox, still within OpenShell's hard policy; do not mistake this for two working layers of containment.
- Host EDR does not imply guest-process visibility.
- Telltale integration is intentionally deferred until Telltale 0.7.0/Event4 stabilizes the downstream contract.
- npm scope ownership, platform package provenance, and publishing credentials must be validated before npm publication.
