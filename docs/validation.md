# Validation and release boundary

CI must check formatting, Clippy, Rust tests, release compilation, CLI version execution, npm launcher mapping/packing, and distribution version synchronization on appropriate runners.

Offline tests cover strict duplicate-key JSON, safe names, starter-policy requirements, create arguments, auth command scope, and native launcher selection. These do not prove successful OAuth exchange, OpenShell enforcement, guest isolation, endpoint-security visibility, or desktop compatibility.

## Live gates

| Gate | Evidence |
| --- | --- |
| Runtime | exact CLI/gateway/driver/kernel/image and create/start/stop |
| Enforcement | denied path/egress/raw-network/privilege attempts |
| CLI agents | auth, fixture edit/test, PTY, interrupt, exit, resume |
| Persistence | project/session survival and deletion behavior |
| Upload | destination, ignore rules, symlinks, partial failure, overwrite |
| Remote | VS Code and each desktop path with credential boundary |
| Telltale | source discovery/failure behavior; external durability separately |
| EDR | supported sensor observes/controls attributed guest process |

## Release

Ordinary commits validate without publishing. A release requires explicit authorization and an exact reviewed version. npm publication has a separate gate for scope ownership, platform artifacts, provenance, and credentials.
