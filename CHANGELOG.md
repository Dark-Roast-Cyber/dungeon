# Changelog

## Unreleased lifecycle (Issue #8)

- Add `up`, `run`, and `down` lifecycle flow: first `up` creates and uploads once, resume avoids upload, `run` starts a stopped bound sandbox without upload, and `down` stops while preserving guest state. Keep overwriting upload as explicit `upload --yes`; project export is not included.

## Unreleased local setup (Issue #7)

- Add `dungeon setup` for Linux/rootless Podman/OpenShell 0.0.116: bootstrap and register one local mTLS gateway, check health and driver, refuse conflicting state, and show the reviewed local-image build/next init command. No image publication, policy change, installer, or MicroVM fallback.

## Unreleased image candidate (0.0.3)

- Add one local Linux agent image with a digest-pinned OpenShell base, exact official OpenCode V2/Claude Code/Codex packages, a pnpm integrity lock, and build-time CLI version checks. No image publication, default-image change, policy expansion, or host-state import.

## Unreleased (0.0.2)

- Replace the unpublished Python implementation with one Rust CLI.
- Split the Rust implementation into plain `cli`, `config`, `policy`, and `openshell` responsibility modules without introducing traits or sub-crates.
- Move `dungeon.json` to schema 2 and remove the redundant policy-path field; `policy.json` is always adjacent.
- Collapse starter profiles into one starter development policy until live validation establishes meaningful scenario differences.
- Remove the provisional in-guest `dungeon telltale` command; Telltale integration is deferred until Telltale 0.7.0/Event4 provides a stable downstream contract.
- Add native release archives for Linux x64, macOS arm64/x64, and Windows x64 with SHA-256 checksums.
- Add a tag-gated GitHub prerelease workflow that verifies the tag against `Cargo.toml`, rebuilds from locked dependencies, and never runs on ordinary branch pushes.
- Add a thin npm/npx launcher design with OS/CPU-specific native package templates. CI stages real native binaries into temporary packages and packs all five tarballs without publishing. No postinstall downloader and no duplicate runtime logic.
- Add `dungeon auth codex` for Codex device-code authentication.
- Add `dungeon auth claude` for Claude Code's vendor-native `/login` flow.
- Keep OpenShell providers as the separate brokered API credential path.
- Keep OpenCode authentication provider-specific. Grok is intentionally not added.
- Bound the future declarative setup model: reviewed images own binaries/toolchains, imports are read-only snapshots, writable state is separate, and setup requests cannot silently widen policy or execute host hooks.
- Preserve the successful synthetic OpenCode SQLite snapshot/download experiment as deferred research rather than binding Dungeon to Telltale's moving pre-0.7 contracts.
- Reorient near-term work around a reviewed agent image, one-command local setup, simplified lifecycle, safe project export, and then reviewed editor/tool configuration imports.

## 0.0.1

Initial Python engineering preview around OpenShell 0.0.116 with explicit config/upload, OpenCode/Claude/Codex execution, VS Code/SSH attachment, policy drift checks, and in-guest Telltale diagnostics.
