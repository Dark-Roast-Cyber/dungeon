# Changelog

## Unreleased (0.0.2)

- Replace the unpublished Python implementation with one Rust CLI.
- Split the Rust implementation into plain `cli`, `config`, `policy`, and `openshell` responsibility modules without introducing traits or sub-crates.
- Move `dungeon.json` to schema 2 and remove the redundant policy-path field; `policy.json` is always adjacent.
- Collapse starter profiles into one starter development policy until live validation establishes meaningful scenario differences.
- Remove the provisional in-guest `dungeon telltale` command; future Telltale integration remains independent retained collection.
- Add a thin npm/npx launcher design with OS/CPU-specific native package templates. No postinstall downloader and no duplicate runtime logic.
- Add `dungeon auth codex` for Codex device-code authentication.
- Add `dungeon auth claude` for Claude Code's vendor-native `/login` flow.
- Keep OpenShell providers as the separate brokered API credential path.
- Keep OpenCode authentication provider-specific. Grok is intentionally not added.

## 0.0.1

Initial Python engineering preview around OpenShell 0.0.116 with explicit config/upload, OpenCode/Claude/Codex execution, VS Code/SSH attachment, policy drift checks, and in-guest Telltale diagnostics.
