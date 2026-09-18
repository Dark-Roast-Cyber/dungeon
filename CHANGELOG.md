# Changelog

## Unreleased (0.0.2)

- Add `dungeon auth codex` for Codex device-code authentication inside the guest.
- Add `dungeon auth claude` to enter Claude Code for its vendor-native `/login` flow.
- Dungeon coordinates vendor authentication but never parses, copies, stores, refreshes, or escrows OAuth credentials.
- OpenShell providers remain the separate path for brokered API credentials.
- OpenCode authentication remains provider-specific. Grok is intentionally not added.

## 0.0.1

Initial engineering preview: Python CLI around OpenShell 0.0.116; guided and
noninteractive configuration; minimal/development native policy starters;
explicit upload; sandbox identity and observed-policy drift checks; direct
OpenCode, Claude Code, Codex and arbitrary-command execution; optional VS Code
and SSH setup; in-guest Telltale dry-run diagnostic; offline contract tests and
packaging guidance.

Live runtime enforcement, desktop compatibility, EDR, durable external Telltale
collection, and native Windows workload execution are not certified or shipped
as completed capabilities in this preview.
