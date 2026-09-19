# Security boundary

Dungeon 0.0.x is an engineering preview. It has not undergone an independent audit or live containment validation.

Trust the host administrator, Dungeon binary/configuration, OpenShell CLI/gateway/driver, selected image, and explicitly exposed credential/tool services. Treat the coding agent, project content, downloaded code, and agent-authored session records as untrusted.

OpenShell owns filesystem/process/network enforcement. Dungeon requires Landlock `hard_requirement` in accepted policy and refuses inspected audit-only endpoints, but policy readback is not kernel attestation.

Dungeon clears the inherited environment before invoking OpenShell and restores only an explicit allowlist. It does not automatically mount/copy host homes, engine sockets, native agent databases, private keys, or provider secrets.

## Credentials

OpenShell providers can grant authority even when raw secrets are brokered. `dungeon auth` starts vendor-owned login inside the guest. Resulting OAuth credentials live in guest-controlled state. Dungeon does not inspect, copy, refresh, escrow, or synchronize them.

## Remaining risks

- Allowed endpoints can receive data.
- Project upload is explicit but is not a secret scanner or immutable snapshot.
- Guest/session durability is backend-specific.
- Mutable images/provider content can change without changing Dungeon's fingerprint.
- Host EDR does not imply guest-process visibility.
- Independent Telltale evidence collection is not implemented yet.
- npm scope ownership, platform package provenance, and publishing credentials must be validated before npm publication.
