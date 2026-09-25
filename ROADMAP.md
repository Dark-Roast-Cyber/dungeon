# Roadmap

## Product direction

Dungeon is a low-friction environment manager and adoption layer for isolated agent development.

The intended experience is that teams can adopt supported OpenShell isolation, optionally add Telltale later, and keep using familiar agent CLIs, shells, editors, SSH, remote attachments, and external harnesses. Dungeon should mostly disappear once the environment is healthy.

Dungeon owns environment setup, configuration, lifecycle, scope, reviewed capability requests, project transfer, identity checks, and integration surfaces. OpenShell owns isolation and policy enforcement. Agent tools and harnesses own orchestration.

Dungeon should not become a prompt engine, issue runner, parallel-agent scheduler, branch orchestrator, or replacement for Sandcastle/OpenCode-style harnesses.

## 0.0.2: validated native foundation

Dungeon has converged on one Rust binary, one config schema, one starter policy, one OpenShell subprocess boundary, native release artifacts, and a thin npm/npx launcher.

Issue #1 validated the primary Linux path with OpenShell 0.0.116 and rootless Podman 5.8.4 using an immutable community-image digest. The run exercised runtime identity, project isolation, policy denials, allowed network, PTY/exit behavior, stop/start persistence, config mismatch refusal, policy drift refusal, SSH, and vendor authentication boundaries. The resulting Dungeon defects were fixed on PR #4. Issue #1 is complete.

The recorded Podman combination is validated. MicroVM, desktop-client containment, EDR visibility, and broader platform claims remain separate work.

## 0.0.3: low-friction contained development

The current milestone remains focused on making the contained development loop comfortable without weakening the security boundary.

Completed or substantially implemented:

1. **Reviewed agent image, Issue #6.** The local image candidate pins official OpenCode, Claude Code, and Codex packages on the validated immutable base. Publication/provenance and default-image selection remain separate approval boundaries. Claude account-dependent validation remains tracked by Issue #11.
2. **One-command local setup, Issue #7.** `dungeon setup` provisions and verifies the local Linux/rootless-Podman gateway after explicit prerequisite installation and immutable gateway-image pull.
3. **Daily lifecycle, Issue #8.** `up`, `run`, and `down` preserve guest state and do not silently re-upload on resume.
4. **Safe project export, Issue #9.** Reviewed staged export is implemented on the validated OpenShell 0.0.116/rootless-Podman path.
5. **Reviewed tool/editor imports, Issue #5.** Continue with explicit read-only snapshots and guest-private writable state. Issue #17 supplies the first image-owned Neovim/Rust prerequisites.

Keep Issue #16 as bounded follow-up hardening for export durability, concurrency, and broader-platform semantics.

## Next: low-friction environment management

After the current 0.0.3 loop is stable, prioritize these slices in dependency order.

### 1. Project adoption without repeated configuration ceremony, Issue #18

Allow a developer to enter a project and resolve one private Dungeon environment without repeatedly passing `--config PATH`.

The explicit config path remains the deterministic automation/recovery surface.

Project-local content must not become security authority. Convenience must resolve to the existing lifecycle, identity, policy, upload, and export semantics rather than creating a second path.

### 2. Capability-oriented configuration, Issue #19

Make environment configuration understandable in terms of project scope, reviewed tools, read-only imports, guest-private state, network requests, provider requirements, and bounded guest environment values.

Guided CLI commands and declarative configuration must converge on one authoritative model.

Do not add arbitrary host hooks, runtime install scripts, raw secrets, automatic policy widening, or a second policy language.

Issues #5 and #17 are the first concrete implementation path and should be extended rather than duplicated.

### 3. Better OpenShell adoption and diagnostics, Issue #22

Improve prerequisite detection, setup diagnostics, repeatability, and remediation guidance for supported OpenShell paths.

Dungeon may automate safe, explicit, non-privileged setup steps where justified, but it must not silently install privileged dependencies, overwrite incompatible state, or fall back to weaker containment.

OpenShell remains visible as the isolation/enforcement engine.

### 4. Stable machine interface for external harnesses, Issue #20

Expose the smallest versioned machine-readable surface required for external harnesses to use Dungeon as an isolated environment provider.

Prefer the native CLI with stable JSON/JSONL before adding a daemon, SDK, or plugin ABI.

The interface should cover lifecycle, status, command execution, streaming, PTY behavior, approved transfer, safe return of changes, identity, and capability/degraded-state reporting.

It should not contain prompts, model selection, issue queues, iteration loops, parallel-agent scheduling, branch strategy, or review orchestration.

### 5. Sandcastle integration, Issue #21

Use Sandcastle as the first concrete harness consumer.

Target functional compatibility with Sandcastle's isolated-provider seam while keeping Dungeon's stronger transfer and policy contracts. Prefer contributing the adapter upstream to Sandcastle if accepted.

Do not port Sandcastle into Dungeon, add Node as a Dungeon runtime dependency, introduce writable host bind mounts, or bypass Dungeon's reviewed project-return boundary.

## Native-feeling attachment

Remote/editor attachment is a first-class product goal.

Continue validating and improving:

- terminal and shell workflows;
- agent CLIs;
- VS Code Remote;
- SSH attachment;
- managed OpenCode serve/attach where it materially reduces friction;
- Claude Desktop and ChatGPT Desktop connection paths when their exact client/platform contracts can be validated.

The goal is that the harness or client runs inside isolation while the user's normal interaction model remains familiar.

## Platform validation

After the contained Linux developer loop is stable:

- validate VS Code Remote workflows;
- validate WSL2/Windows management;
- validate macOS;
- revisit OpenShell MicroVM when its Landlock/runtime behavior is compatible with Dungeon's hard requirement;
- validate Claude Desktop and ChatGPT Desktop connection paths separately from generic SSH;
- validate EDR visibility/control separately from Dungeon/OpenShell isolation.

Do not generalize support from successful build targets or generic SSH behavior.

## Optional Telltale integration

Telltale integration is intentionally not active runtime work while Telltale completes its 0.7 semantic/runtime convergence.

Issue #2 remains closed as deferred. Revisit integration only after Telltale 0.7.0 ships with Event4 as the intended external telemetry contract and the relevant privacy, durability, identity, source-health, and embedding/acquisition boundaries are stable enough for a downstream dependency.

The long-term direction is optional composition:

```text
agent / editor / external harness
  -> Dungeon environment management
  -> OpenShell isolation

optional:
  -> Telltale detection / response integration
```

Dungeon must remain useful without Telltale, and Telltale must not become an isolation dependency.

## Later

Later work may include:

- organization policy ceilings;
- mature remote/desktop workflows;
- published reviewed images with provenance;
- npm registry publication after scope/provenance setup;
- another isolation backend only after a concrete validated need exists.

Avoid generic backend frameworks, plugin systems, or orchestration features without a demonstrated requirement.
