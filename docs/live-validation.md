# Live validation runbook

This runbook is the execution checklist for Issue #1. It is intentionally operational rather than architectural: the goal is to validate the current 0.0.2 boundary, record exact evidence, and surface defects without redesigning Dungeon during the test.

Use synthetic project content and dedicated test credentials. Do not commit raw credentials, personal paths, transcripts, browser state, or unsanitized runtime logs.

## 1. Freeze the test inputs

Record before creating an environment:

```text
Dungeon commit:
Dungeon version:
OpenShell CLI version:
OpenShell gateway version:
OpenShell driver/runtime:
Host OS/kernel:
Container/VM runtime version:
Reviewed image reference:
Resolved image digest:
OpenCode version:
Claude Code version:
Codex version:
VS Code version:
```

Use the exact PR/main commit being evaluated. Prefer an immutable image digest over a mutable tag.

Do not update Dungeon, OpenShell, the image, or agent binaries midway through one validation record. If an input changes, start a new record.

## 2. Create a synthetic project

Use a small disposable repository outside the Dungeon configuration directory. Include:

- one text file the agent may edit;
- one simple test/build command;
- no real secrets;
- no real SSH keys;
- no personal configuration;
- no production credentials.

Record the initial tree hash or commit so later changes are attributable.

## 3. Initialize and inspect without starting a runtime

```sh
dungeon init --directory "$CONFIG_DIR" --project "$PROJECT"
CONFIG="$CONFIG_DIR/dungeon.json"

dungeon --config "$CONFIG" plan
```

Verify before continuing:

- config is outside the project;
- schema is 2;
- `policy.json` is adjacent to `dungeon.json`;
- the selected image/gateway/workspace are correct;
- no project upload is embedded in create;
- no unexpected provider or environment injection appears;
- policy uses Landlock `hard_requirement`;
- inspected network endpoints are enforcing rather than audit-only.

Result: `PASS | FAIL | DEGRADED | NOT TESTABLE`.

## 4. Create and bind the environment

```sh
dungeon --config "$CONFIG" create
dungeon --config "$CONFIG" status
```

Record the backend sandbox ID and compare it with host-side `runtime.json`.

Verify:

- sandbox is Ready;
- Dungeon label/fingerprint is present;
- returned policy source is sandbox-scoped;
- returned effective policy passes Dungeon validation;
- runtime binding records the expected sandbox ID/config fingerprint/policy digest;
- rerunning `create` does not silently create or adopt another sandbox.

Do not interpret policy readback as proof of kernel enforcement.

## 5. Upload behavior

Review the synthetic project first, then:

```sh
dungeon --config "$CONFIG" upload --yes
dungeon --config "$CONFIG" exec -- pwd
dungeon --config "$CONFIG" exec -- ls -la
```

Expected guest project root: `/sandbox/project`.

Verify:

- project was copied rather than bind-mounted;
- host project edits after upload do not appear automatically in the guest;
- guest edits do not appear automatically on the host;
- outside-project symlinks are rejected by host preflight;
- special files are rejected;
- a normal project upload completes without broad host-home transfer.

Record partial-failure or overwrite behavior if encountered rather than assuming semantics.

## 6. Basic process boundary

Run:

```sh
dungeon --config "$CONFIG" exec -- id
dungeon --config "$CONFIG" exec -- uname -a
dungeon --config "$CONFIG" shell
```

From the shell, verify the effective user/group and inspect the workspace without changing policy.

Then perform bounded denial probes:

```sh
cat /etc/shadow
touch /etc/dungeon-should-not-exist
sudo -n true
curl --connect-timeout 5 https://1.1.1.1/
```

The exact tool availability may vary. A missing probe tool is `NOT TESTABLE`, not a pass.

Expected security meaning:

- protected system reads/writes do not become available;
- privilege increase does not succeed;
- raw/unapproved network access does not bypass policy.

Record command, exit status, and a short sanitized result. Do not paste large raw logs.

## 7. Allowed network behavior

Exercise only endpoints that the reviewed starter policy intentionally allows and that are needed by the selected agent/toolchain.

Verify:

- required allowed endpoints work;
- an unapproved destination fails;
- failure is distinguishable from DNS/tool absence when possible;
- allowed destinations are not treated as proof that credential use is safe.

If a required vendor endpoint is missing from the starter policy, record it as a defect/request. Do not widen policy silently during the same validation record.

## 8. Agent execution

For each supported agent, use the same synthetic project.

### OpenCode

```sh
dungeon --config "$CONFIG" run opencode
```

Validate:

- interactive TTY rendering;
- resize behavior;
- a synthetic edit;
- a project test/build;
- normal exit;
- Ctrl-C behavior;
- session state after stop/start.

Record the actual OpenCode state/database path and SQLite journal mode if observable. This is required input for Issue #2.

### Claude Code

```sh
dungeon --config "$CONFIG" auth claude
dungeon --config "$CONFIG" run claude
```

Use the vendor browser flow from inside the guest. Verify Dungeon does not print, parse, or persist the credential itself.

### Codex

```sh
dungeon --config "$CONFIG" auth codex
dungeon --config "$CONFIG" run codex
```

Verify the device URL/code flow works from the remote/headless environment and that Dungeon remains only a coordinator.

For both authenticated agents, record where vendor-owned guest auth state is stored at a directory level only. Do not record token values.

## 9. Stop, start, and identity safety

```sh
dungeon --config "$CONFIG" stop
dungeon --config "$CONFIG" start
dungeon --config "$CONFIG" status
```

Verify:

- guest project/session state behaves as the backend documents;
- Dungeon continues to bind the same backend ID;
- execution refuses if local config fingerprint no longer matches;
- execution refuses if the sandbox is replaced with another instance.

For backend-policy drift, use a direct OpenShell mutation only if the installed version exposes a safe supported way to do so. If it does not, record `NOT TESTABLE`; do not invent an unsupported mutation path.

## 10. VS Code and SSH

```sh
dungeon --config "$CONFIG" code
dungeon --config "$CONFIG" ssh-config
```

Validate VS Code separately from generic SSH:

- project path is the intended guest path;
- extensions/tools that execute remotely actually execute in the guest;
- local GUI/extensions remain outside the workload boundary;
- no unexpected credential/config directories are mounted or copied.

For Claude Desktop or ChatGPT remote-project/SSH testing, record exact product version, platform, connection mode, and credential behavior. Generic SSH success is not evidence that a desktop feature is contained.

## 11. Prepare the Telltale acquisition facts

Do not implement collection during this run. Capture only the facts Issue #2 needs:

- actual OpenCode database path;
- whether WAL/journal side files are present;
- SQLite journal mode if safely queryable;
- whether OpenShell can produce/download a consistent file or directory snapshot;
- what remains available after `stop`;
- what remains available before/after environment deletion if deletion is tested separately;
- whether a host-side private acquisition root can receive data without becoming guest-writable.

The target Telltale rooted layout for the first OpenCode slice is:

```text
<acquisition-root>/.local/share/opencode/opencode.db
```

Do not copy only a live SQLite main database and call it consistent.

## 12. Results table

Use one row per check:

| Check | Result | Evidence | Follow-up |
| --- | --- | --- | --- |
| create/status identity | PASS/FAIL/DEGRADED/NOT TESTABLE | short sanitized observation | issue/none |
| project upload isolation |  |  |  |
| protected filesystem read |  |  |  |
| protected filesystem write |  |  |  |
| privilege increase |  |  |  |
| raw/unapproved network |  |  |  |
| OpenCode edit/test |  |  |  |
| Claude auth/run |  |  |  |
| Codex auth/run |  |  |  |
| stop/start persistence |  |  |  |
| config mismatch refusal |  |  |  |
| sandbox replacement refusal |  |  |  |
| backend policy drift |  |  |  |
| VS Code remote |  |  |  |
| SSH config |  |  |  |
| OpenCode DB acquisition facts |  |  |  |

## 13. Defect handling

For every failure:

1. Preserve the exact Dungeon/OpenShell/image versions.
2. Reduce to the smallest reproducible command.
3. Classify it as Dungeon defect, OpenShell/backend behavior, image/tooling problem, missing policy requirement, or unsupported test.
4. Do not weaken defaults merely to make the test green.
5. Add a focused regression test for any Dungeon defect.
6. Keep unrelated feature work out of the fix.

A successful run proves only the exact tested combination. It does not automatically certify every OpenShell driver, desktop client, operating system, EDR product, or future version.
