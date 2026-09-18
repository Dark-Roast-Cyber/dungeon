# Changelog

## Unreleased (0.0.2)

- Replace the unpublished Python implementation with one Rust CLI.
- Preserve explicit configuration, OpenShell policy, runtime identity, project upload, terminal, SSH/editor, authentication, and Telltale diagnostic boundaries.
- Add a thin npm/npx launcher design with OS/CPU-specific native package templates. No postinstall downloader and no duplicate runtime logic.
- Add `dungeon auth codex` for Codex device-code authentication.
- Add `dungeon auth claude` for Claude Code's vendor-native `/login` flow.
- Keep OpenShell providers as the separate brokered API credential path.
- Keep OpenCode authentication provider-specific. Grok is intentionally not added.

## 0.0.1

Initial Python engineering preview around OpenShell 0.0.116 with explicit config/upload, OpenCode/Claude/Codex execution, VS Code/SSH attachment, policy drift checks, and in-guest Telltale diagnostics.
