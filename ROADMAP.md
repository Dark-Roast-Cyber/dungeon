# Roadmap

## 0.0.2: validated native foundation

Dungeon has converged on one Rust binary, one config schema, one starter policy, one OpenShell subprocess boundary, native release artifacts, and a thin npm/npx launcher.

Issue #1 validated the primary Linux path with OpenShell 0.0.116 and rootless Podman 5.8.4 using an immutable community-image digest. The run exercised runtime identity, project isolation, policy denials, allowed network, PTY/exit behavior, stop/start persistence, config mismatch refusal, policy drift refusal, SSH, and vendor authentication boundaries. The resulting Dungeon defects are fixed on PR #4.

The recorded Podman combination is validated. MicroVM, desktop-client containment, EDR visibility, and broader platform claims remain separate work.

## 0.0.3: low-friction contained development

The next milestone is developer usability, not telemetry integration.

The target experience is that a developer can create an isolated environment, authenticate an agent, work normally, stop/resume it, and safely get changes back without learning OpenShell internals or weakening containment.

Work in dependency order:

1. **Reviewed agent image — Issue #6.** The [local image candidate](images/agent/README.md) pins official OpenCode, Claude Code, and Codex packages on the validated immutable base. OpenCode edited/tested the synthetic fixture; Codex edited/tested with explicit approvals because its inner sandbox cannot create a user namespace under OpenShell. Claude launched to vendor login, with account-dependent checks delegated to [Issue #11](https://github.com/Dark-Roast-Cyber/dungeon/issues/11). Publication/provenance and default-image selection require separate approval. No image/plugin ecosystem or dynamic agent updater.
2. **One-command local setup — Issue #7.** `dungeon setup` provisions and verifies the local Linux/rootless-Podman gateway after explicit prerequisite installation and an immutable gateway-image pull. It does not select an unpublished agent image or attest to containment on untested hosts.
3. **Daily lifecycle — Issue #8.** Implemented the `up` / `run` / `down` flow: first `up` creates and uploads once, resume does not upload, and explicit `upload --yes` is reserved for deliberate refresh/recovery. Guest state survives `down`. Safe project export remains Issue #9.
4. **Safe project export — Issue #9.** Stage guest changes on the host, show the proposed change set, detect conflicts, and require approval before mutating the authoritative host project. Do not replace this with a writable bind mount.
5. **Reviewed tool/editor imports — Issue #5.** After the core loop is comfortable, add explicit read-only configuration snapshots and separate guest-private writable state without host execution hooks.

## Platform validation

After the contained Linux developer loop is stable:

- validate VS Code Remote workflows;
- validate WSL2/Windows management;
- validate macOS;
- revisit OpenShell MicroVM when its Landlock/runtime behavior is compatible with Dungeon's hard requirement;
- validate Claude Desktop and ChatGPT Desktop connection paths separately from generic SSH;
- validate EDR visibility/control separately from Dungeon/OpenShell isolation.

## Deferred Telltale integration

Telltale integration is intentionally **not active roadmap work** while Telltale completes its 0.7 semantic/runtime convergence.

Issue #2 is closed as deferred. Revisit integration only after Telltale 0.7.0 ships with Event4 as the intended external telemetry contract and the relevant privacy, durability, identity, source-health, and embedding/acquisition boundaries are stable enough for a downstream dependency.

Issue #1's synthetic OpenCode SQLite snapshot result is preserved as research, not as an architecture commitment. Dungeon must not add Event 3 compatibility, transitional Telltale models, collection state, or a collector framework in the meantime.

## Later

Later work may include managed OpenCode serve/attach lifecycle, organization policy integration, mature remote/desktop workflows, npm registry publication after scope/provenance setup, and another isolation backend only after a concrete validated need exists.
