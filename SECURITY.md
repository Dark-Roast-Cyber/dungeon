# Security boundary

Dungeon 0.0.1 is a host-side engineering preview. It has not undergone an
independent audit or live containment validation. Do not use it as a mandatory
enterprise enforcement product or for hostile workloads requiring proven
isolation. Report suspected vulnerabilities privately through the repository's
GitHub security reporting channel when enabled; do not post secrets publicly.

## Trust and enforcement

Trust the host administrator, Dungeon installation/configuration, OpenShell
CLI/gateway/driver, selected image, and any explicitly exposed credential or
external tool service. The coding agent, project content, downloaded code, and
agent-authored session records are untrusted. Host administrative control is
not restricted by this launcher.

OpenShell enforces filesystem/process/network policy. A Docker-backed gateway
is not a per-workload microVM. Selecting an image or seeing a successful plan
does not prove which kernel boundary or security controls are active.
Mandatory Landlock and blocking inspected policy modes are requested and checked
in returned configuration. A policy response is not kernel attestation.

No host home, engine socket, native agent database, private keys, editor config,
or environment secrets are automatically mounted or copied. Management keeps a
small host environment allowlist, including standard proxy settings and the
host SSH agent for gateway access. That is not permission to forward the agent
socket into a workload. Project-owned executable paths and shell evaluation
are excluded at Dungeon's subprocess boundary.

## Remaining risks

- Allowed endpoints can receive data. Host/port filtering is not exfiltration
  prevention or authorization for every operation at an allowed service.
- Providers grant authority even when raw credentials remain brokered. Guest
  OAuth login stores vendor credentials in guest-controlled state; compromise of
  that environment can expose or misuse them. Dungeon does not inspect, copy,
  refresh, or escrow those credentials. Some desktop integrations may also
  expose real tokens in the guest.
- Project upload is explicit but is not a secret scanner or immutable snapshot.
  Preflight rejects outward symlinks and special files. Concurrent host changes,
  upstream transfer behavior, and ignored-file semantics still need validation.
- Downloaded output can be malicious when later executed on the host. Dungeon
  deliberately has no automatic export or host postprocessing hooks.
- A stopped/deleted VM, guest, or container has driver-specific durability.
  Preserve valuable work before deletion; this release provides no recovery.
- Mutable images and provider content can change without changing config labels.
  Fingerprints and recorded IDs detect some drift, not malicious administrators.
- Telltale inside the agent's environment can be stopped, altered, or given
  altered transcripts. The diagnostic is not independently trustworthy evidence.
- Host EDR does not imply guest-process visibility. CrowdStrike/Elastic support
  requires an explicitly supported deployment and end-to-end verification.

Creation failures do not trigger automatic deletion. Inspect the exact named
sandbox through the scoped backend CLI. Stop remains available for a bound
identity after policy drift; executing further commands requires resolving it.
