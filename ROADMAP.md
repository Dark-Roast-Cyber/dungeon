# Roadmap

## 0.0.2: native foundation

Converge on one Rust binary, one config schema, one starter policy, one OpenShell subprocess boundary, native release artifacts, and a thin npm/npx launcher. Keep the implementation small enough to validate directly before adding frameworks or compatibility layers.

Code-side distribution packaging is complete when CI produces the four native archives and five staged npm tarballs without publishing. The remaining 0.0.2 product gate is live OpenShell validation and any focused defects it exposes.

## Next: prove the execution boundary

Validate a pinned OpenShell image/gateway, OpenCode, Claude Code, Codex, vendor login, terminals, stop/resume, policy denials, persistence, VS Code, and desktop SSH. Replace intended-support statements only with reproducible evidence.

## Next: observation and adoption

Add independent Telltale collection with durable delivery, source health, correlation, and teardown retention.

The declarative setup/import design is now bounded: reviewed OCI images remain the binary/toolchain authority; host imports are reviewed read-only snapshots; writable state is separate; requested network/credential capabilities never widen policy automatically; and no host execution hooks or plugin ABI are introduced. Implementation remains deferred until the execution boundary is live-validated so it does not expand the first validation surface.

## Later

Managed OpenCode serve/attach lifecycle, reviewed export/session migration, organization policy integration, validated EDR deployments, npm registry publication after scope/provenance setup, and another backend only after a concrete need is established.
