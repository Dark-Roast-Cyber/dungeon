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

**Reviewed tool/editor imports, Issue #5 (plan-only slice landed).** The authoritative private config now accepts an explicit Neovim directory declaration; `plan` inventories it with file hashes plus the derived image-owned `nvim` requirement and guest-private state paths. Runtime application stays blocked: OpenShell 0.0.116 uploads as the sandbox user into writable `/sandbox`, which cannot enforce a read-only imported subtree, and Dungeon will not ship a weaker copy labeled as read-only.

**Live workspace mount, Issue #24.** The single schema-2 model has closed copy/live transfer intent. Live derives exactly one read-write OpenShell Podman bind request from the canonical configured project to `/sandbox/project`, while raw driver config and arbitrary mounts remain unavailable. Existing/default init remains copy, upload/export stays copy-only, and the current gateway is not auto-migrated. A narrowly scoped OpenShell 0.0.116/rootless-Podman/Fedora gate is recorded in [validation](docs/validation.md); live does not become the normal default without an explicit promotion decision.

Keep Issue #16 as bounded follow-up hardening for export durability, concurrency, and broader-platform semantics.

## Next: low-friction environment management

After the current 0.0.3 loop is stable, prioritize these slices in dependency order.

**Remote-first use with live local edits.** Keep the user's
editor/client and configuration on the host; run agent tools and state inside
isolation. A full native OpenCode remote AI edit/test and same-session restart
resume passed; interactive TUI message submission and managed serve/forward/
attach lifecycle remain open. See [validation](docs/validation.md) for the
recorded gates. `forward` is foreground and loopback-only; it does not own or
stop an independently started guest server. Validate VS Code Remote and vendor
remote paths individually. Automate lifecycle only after its ownership gates.
Guest configuration imports are optional follow-up, not a prerequisite for the
remote-first loop. Reviewed custom images remain the installed-tool mechanism.
Telltale stays optional and separate from isolation.

### 1. Project adoption without repeated configuration ceremony, Issue #18

A first project-binding slice is implemented on top of the existing lifecycle:
`dungeon [PROJECT]` uses the supplied canonical directory or current directory,
derives one private Dungeon config location, and launches the configured default
agent after ensuring the environment is up. Normal subcommands can resolve the
current project's same private binding without repeated `--config PATH`.

The binding is deterministic rather than registry-based, repository content is
never configuration authority, first creation is interactive and reviewed, and
explicit `--config` remains the automation/recovery surface. Follow-up
ergonomics should stay on this same binding and lifecycle rather than adding a
second project/config model.

### 2. Capability-oriented configuration, Issue #19

Make environment configuration understandable in terms of project scope, reviewed tools, read-only imports, guest-private state, network requests, provider requirements, and bounded guest environment values.

Guided CLI commands and declarative configuration must converge on one authoritative model.

Do not add arbitrary host hooks, runtime install scripts, raw secrets, automatic policy widening, or a second policy language.

Issue #5 is the first configuration-import slice; Issue #17 supplies its reviewed image-owned tool prerequisites and should not be reimplemented as a runtime installer.

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

Do not port Sandcastle into Dungeon, add Node as a Dungeon runtime dependency, add arbitrary writable host binds, or bypass Dungeon's reviewed copy-mode project-return boundary. Where live project access is explicitly selected, it is limited to the canonical configured project; prefer an external integration-side adapter and do not add other writable binds or a host-execution fallback.

## Native-feeling attachment

Remote/editor attachment is a first-class product goal.

Continue validating and improving:

- terminal and shell workflows;
- agent CLIs;
- VS Code Remote;
- SSH attachment;
- managed OpenCode serve/attach where it materially reduces friction;
- Claude Desktop and ChatGPT Desktop connection paths when their exact client/platform contracts can be validated.

The goal is that agent execution runs inside isolation while the user's client
and editor can remain on the host. No host editor-config import should be
required for that workflow. Keep tools installed in reviewed images; imports
remain an optional, explicitly reviewed guest setup path.

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
