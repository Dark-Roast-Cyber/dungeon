# Validation and release boundary

CI checks formatting, Clippy, Rust tests, release compilation, CLI version execution, npm launcher mapping/packing, and distribution version synchronization on explicit supported runners.

## Version policy

- Rust is pinned in `rust-toolchain.toml` to 1.98.1, the current stable toolchain. The pin makes local and CI builds use the same compiler and includes the 1.98.1 miscompilation fix.
- Cargo dependency baselines track current stable crate releases. `Cargo.lock` is the authoritative exact dependency graph for builds.
- GitHub Actions are pinned to immutable commit SHAs corresponding to current stable action releases.
- CI uses explicit GA runner images: Ubuntu 26.04, macOS 26 arm64, macOS 26 Intel, and Windows Server 2025. This avoids silent OS changes from `*-latest` aliases while staying on current supported images.
- npm launcher validation covers Node 24 LTS and Node 26 Current. Odd-numbered and EOL Node lines are not supported.
- OpenShell 0.0.116 is an intentional compatibility pin because Dungeon consumes version-sensitive CLI and structured-output contracts.

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
