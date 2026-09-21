# Validation and release boundary

CI checks formatting, Clippy, locked Rust tests, release compilation, CLI version execution, npm launcher behavior, distribution version synchronization, native archive creation, and full npm package assembly on explicit supported runners.

## Version policy

- Rust is pinned in `rust-toolchain.toml` to 1.98.1.
- `Cargo.lock` is committed and is the authoritative exact dependency graph for the application.
- GitHub Actions are pinned to immutable commit SHAs corresponding to stable action releases.
- CI uses explicit GA runner images: Ubuntu 26.04, macOS 26 arm64, macOS 26 Intel, and Windows Server 2025.
- npm launcher validation targets Node 24 LTS.
- OpenShell 0.0.116 is an intentional compatibility pin because Dungeon consumes version-sensitive CLI and structured-output contracts.

Offline tests cover strict duplicate-key JSON, safe names, schema rejection, starter-policy requirements, create arguments, auth command scope, removed provisional commands, native launcher selection, npm staging, and native archive production. These do not prove successful OAuth exchange, OpenShell enforcement, guest isolation, endpoint-security visibility, or desktop compatibility.

## Native release gate

A release tag must be exactly `vMAJOR.MINOR.PATCH` and match the package version in `Cargo.toml`. The release workflow rebuilds from the tagged source, runs locked quality/tests, executes each native binary, emits four archives, generates `SHA256SUMS`, and creates a GitHub prerelease.

No release is created by ordinary CI, pull requests, or branch pushes.

## npm package gate

Ordinary CI stages the real native binaries into temporary platform packages and creates five npm tarballs: four platform packages plus the launcher. Checked-in platform templates remain private and unchanged.

This validates packaging only. npm registry publication remains blocked until the package scope, provenance, and publishing credentials are explicitly established.

For the first end-to-end execution, use [live-validation.md](live-validation.md) and record the exact tested runtime combination.

## Live gates

| Gate | Evidence |
| --- | --- |
| Runtime | exact CLI/gateway/driver/kernel/image and create/start/stop |
| Enforcement | denied path/egress/raw-network/privilege attempts |
| CLI agents | auth, fixture edit/test, PTY, interrupt, exit, resume |
| Persistence | project/session survival and deletion behavior |
| Upload | destination, symlinks, partial failure, overwrite |
| Remote | VS Code and each desktop path with credential boundary |
| Telltale | independent source discovery/failure/durability when implemented |
| EDR | supported sensor observes/controls attributed guest process |

## Release

Creating the reviewed version tag is the explicit native-release authorization step. npm publication remains a separate future gate. Live OpenShell validation remains required before broadening runtime support claims.
