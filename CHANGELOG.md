# Changelog

## 0.3.0

First Rust engineering preview, replacing the Python implementation in v0.0.1.
The supported runtime path is Linux. Native builds for macOS and Windows do not
establish runtime support or containment certification. Dungeon is not audited.

### Environments and daily use

- One Rust binary manages private schema-2 project bindings, scope review,
  configuration editing, setup, diagnostics and lifecycle. Project files cannot
  authorize their own policy or mounts.
- Run OpenCode, Claude Code, Codex or image-provided tools in plain rootless Podman
  or standard Docker CE containers. OpenShell 0.0.116 remains optional for hard
  Landlock/network policy, copy/export, SSH, forwarding and host OpenCode attachment.
- New human bindings default to reviewed live project access. Explicit/legacy
  copy mode retains one-time upload, reviewed three-way export, transfer locks and
  retained recovery material. Stop/resume preserves guest files, login and sessions.
- New Linux Podman bindings default to disabled per-workload SELinux separation
  without host relabeling. Select `--selinux-isolation private` for the extra
  MCS boundary. Host SELinux stays unchanged; historical configs/fingerprints stay
  private. Docker and OpenShell refuse disabled mode. See
  [security controls](https://github.com/Dark-Roast-Cyber/dungeon/blob/v0.3.0/docs/security-controls.md).
- Optional reviewed OpenCode/Neovim imports remain read-only; native auth/session
  databases are excluded. Guest-native login is separate from named providers.
  Replacing a guest to change its image or SELinux mode loses guest-only state.

### Automation, hardening and distribution

- Schema-1 lifecycle JSON and OpenShell workload-byte JSONL reuse the same runtime.
  Runtime progress stays on stderr; workload streams and exit status are preserved.
  Opt-in owned execution assists cleanup, not stronger containment.
- Identity/policy checks refuse drift; uninstall refuses foreign bindings and
  removes only reviewed resources. Inspected network protocols require explicit
  `enforcement=enforce`; unknown or mistyped selectors refuse.
- The optional unpublished image recipe pins OpenCode 2.2.7, patched Node22.23.4,
  Claude Code, Codex, Neovim and Rust. Build/select it explicitly; it is not the
  mutable upstream default and is not upgraded during launch.
- Tag-gated native archives include SHA-256 checksums and reviewed release notes.
  Rust, lockfile, npm manifests and launcher dependency pins must agree before
  publication. npm tarball assembly is tested; registry publication is not enabled.

### Known limitations

- OpenShell 0.1.x remains gated pending the full port and native qualification.
  The SELinux toggle does not enable it or relax OpenShell's hard policy.
- Imported newer OpenCode configuration can fail in the older CLI supplied by
  upstream `base:latest` ([#47](https://github.com/Dark-Roast-Cyber/dungeon/issues/47)).
  The image tag does not imply the latest agent. Prefer imports disabled until
  the selected image/config combination is validated.
- Historical and OpenShell `cli.json` imports are read-only and can block UI
  preference writes ([#48](https://github.com/Dark-Roast-Cyber/dungeon/issues/48)).
  New plain-container selections copy it once into guest-owned writable state;
  native OpenCode UI acceptance remains outstanding. The host source is never
  made writable.
- The current Fedora/Podman disabled/private SELinux and full-container gates pass;
  older exit137 observations remain undiagnosed, not generally fixed by this release.
  Evidence is combination-specific. Desktop, EDR, remote-host and broader-platform
  claims remain unvalidated; Sandcastle and Telltale are not integrated.

## 0.0.1

Initial Python engineering preview around OpenShell 0.0.116 with explicit config/upload, OpenCode/Claude/Codex execution, VS Code/SSH attachment, policy drift checks, and in-guest Telltale diagnostics.
