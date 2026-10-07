# Changelog

## Unreleased preview additions (0.0.2)

- Simplify `init` with current-project, generated-name and private-binding defaults; one scope confirmation or `--yes`, with explicit customization flags. Add offline `config`/`config --path`, make `plan` non-initializing, improve command help, and open `shell` in the guest project. Validate initial configuration before writing files; no image/policy default change.
- Serialize first-copy creation/upload and block new work while durable initial upload is pending; recovery remains explicit `upload --yes`, without automatic retry or rollback guarantees.

- Add schema-1 machine lifecycle JSON and workload-byte JSONL on the same Runtime, with explicit whole-project transfer approval; no Sandcastle adapter or arbitrary-path synchronization.
- Add read-only `doctor` diagnostics and opt-in Linux owned foreground/interactive execution. Ownership assists cleanup, not stronger containment.
- Add explicit experimental live project transfer and fresh `setup --enable-live`; copy remains the default and copy-gateway migration is refused. Validation is limited to the recorded Linux/OpenShell/Podman combinations.
- Add approved private Neovim snapshots with read-only mounted contents. Writable guest ancestors still allow pathname replacement; this is not trusted-execution control.
- Serialize copy transfers with private native locks and retain interrupted export recovery for manual inspection; no automatic rollback or power-loss guarantee.

## Unreleased safe export (Issue #9)

- Add Ready-only reviewed export for OpenShell 0.0.116: stage the guest tree, summarize changes deterministically, detect three-way conflicts, and require typed `yes` or `--yes` before applying. Preserve host-only files; reject special entries and unsupported metadata. Track the uploaded baseline in private config state, and retain private adjacent backups if apply fails. No live sync or host mount.

## Unreleased lifecycle (Issue #8)

- Add `up`, `run`, and `down` lifecycle flow: first `up` creates and uploads once, resume avoids upload, `run` starts a stopped bound sandbox without upload, and `down` stops while preserving guest state. Keep overwriting upload as explicit `upload --yes`; reviewed project export is a separate Issue #9 operation.

## Unreleased local setup (Issue #7)

- Add `dungeon setup` for Linux/rootless Podman/OpenShell 0.0.116: bootstrap and register one local mTLS gateway, check health and driver, refuse conflicting state, and show the reviewed local-image build/next init command. No image publication, policy change, installer, or MicroVM fallback.

## Local image candidate (0.0.3-dev, not the CLI version)

- Add one local Linux agent image with a digest-pinned OpenShell base, exact official OpenCode V2/Claude Code/Codex packages, a pnpm integrity lock, and build-time CLI version checks. No image publication, default-image change, policy expansion, or host-state import.
- Add pinned, hash-verified Neovim 0.12.5 and Rust 1.98.1 (`rustc`, `cargo`, `rust-std`) to the image. Synthetic editor, compiler, and offline Cargo workloads passed under the unchanged OpenShell hard policy on the tested rootless-Podman gateway. Writable state uses HOME-derived `/sandbox` locations; no runtime installers or explicit guest environment propagation were added. Approved configuration snapshots are now implemented separately, with the contents-only limit above.

## Unreleased (0.0.2)

- Replace the Python implementation published as v0.0.1 with one Rust CLI; Rust 0.0.2 remains source-only.
- Split the Rust implementation into plain `cli`, `config`, `policy`, and `openshell` responsibility modules without introducing traits or sub-crates.
- Move `dungeon.json` to schema 2 and remove the redundant policy-path field; `policy.json` is always adjacent.
- Collapse starter profiles into one starter development policy until live validation establishes meaningful scenario differences.
- Remove the provisional in-guest `dungeon telltale` command; Telltale integration is deferred until Telltale 0.7.0/Event4 provides a stable downstream contract.
- Add CI native archive packaging for Linux x64, macOS arm64/x64, and Windows x64, and a release workflow generating SHA-256 checksums. No Rust release assets are published yet.
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
