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

## 11. Developer-loop observations

For containment/usability work, record facts that affect the daily development loop without turning them into new security claims:

- actual guest agent versions;
- whether each agent can launch in `/sandbox/project`;
- whether vendor authentication can begin and persist across stop/start;
- whether the selected image contains the baseline development tools the workflow expects;
- whether project upload and later export semantics are clear enough to avoid accidental work loss;
- whether the host needs OpenShell-specific manual steps that Dungeon should later automate.

Image/tooling failures belong to the image contract, not to agent-specific workarounds in Dungeon.

## 12. Results table

Use one row per check. Add agent/image or developer-friction observations when relevant:

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
| agent image/tooling compatibility |  |  |  |

## 13. Defect handling

For every failure:

1. Preserve the exact Dungeon/OpenShell/image versions.
2. Reduce to the smallest reproducible command.
3. Classify it as Dungeon defect, OpenShell/backend behavior, image/tooling problem, missing policy requirement, or unsupported test.
4. Do not weaken defaults merely to make the test green.
5. Add a focused regression test for any Dungeon defect.
6. Keep unrelated feature work out of the fix.

A successful run proves only the exact tested combination. It does not automatically certify every OpenShell driver, desktop client, operating system, EDR product, or future version.


## Issue #24 live workspace validation gate

Run this matrix only against the Issue #24 pre-validation candidate after static
CI is green. Record the exact Dungeon commit, Fedora release/kernel, OpenShell
0.0.116 CLI/gateway, and rootless Podman 5.8.4 versions. Use a synthetic project
with ordinary owner-writable files and no credentials.

Use a dedicated disposable Dungeon/OpenShell gateway. Do not rewrite an existing
`dungeon-local` gateway in place. Its Podman driver candidate must include:

```toml
[openshell.drivers.podman]
enable_bind_mounts = true
userns = "keep-id:uid=998,gid=998"
```

The validation gateway is containerized while its Podman API socket reaches the
host rootless Podman service. OpenShell 0.0.116 extracts the Podman supervisor
binary under `$XDG_DATA_HOME/openshell/podman-supervisor/...` and supplies that
absolute path back to host Podman as a bind source. Therefore the disposable
gateway must use a private absolute driver-data directory that resolves to the
same path on both sides of the gateway container boundary.

For example, if the validation root is already a private disposable directory:

```sh
LIVE_DRIVER_DATA="$LIVE_ROOT/driver-data"
mkdir -m 700 "$LIVE_DRIVER_DATA"

# In the trusted gateway container invocation:
#   -v "$LIVE_DRIVER_DATA:$LIVE_DRIVER_DATA"
#   -e "XDG_DATA_HOME=$LIVE_DRIVER_DATA"
```

Do not map the gateway's driver data to a container-only path such as
`/var/openshell/.local/share`. Host Podman cannot resolve that path even when
the file exists inside the gateway container. The driver-data mount belongs
only to the trusted gateway and must never be propagated into the sandbox.

Apply the same host/container path-identity rule to any other OpenShell driver
state that is later supplied to host Podman as a bind source. This requirement
does not authorize broader host mounts and does not change the single
`/sandbox/project` live-workspace authority.

Create an explicit live config:

```sh
dungeon init \
  --directory "$CONFIG_DIR" \
  --project "$PROJECT" \
  --gateway "$LIVE_GATEWAY" \
  --project-transfer live \
  --non-interactive \
  --name issue24-live

CONFIG="$CONFIG_DIR/dungeon.json"
dungeon --config "$CONFIG" plan
```

Before contacting the runtime, verify that plan reports exactly the canonical
host project, guest target `/sandbox/project`, read-write intent, the Podman
host-bind requirement, the keep-id candidate, and unresolved validation status.
It must not expose arbitrary driver JSON or claim gateway enforcement.

Then run:

```sh
stat -c '%u:%g %a %n' "$PROJECT" "$PROJECT/probe.txt"
dungeon --config "$CONFIG" up
dungeon --config "$CONFIG" exec -- id
dungeon --config "$CONFIG" exec -- stat -c '%u:%g %a %n' /sandbox/project /sandbox/project/probe.txt
dungeon --config "$CONFIG" exec -- sh -lc 'printf "guest edit\n" >> /sandbox/project/probe.txt'
cat "$PROJECT/probe.txt"
printf 'host edit\n' >> "$PROJECT/probe.txt"
dungeon --config "$CONFIG" exec -- tail -n 2 /sandbox/project/probe.txt
```

Required evidence:

- guest effective UID/GID is the expected sandbox identity 998/998;
- ordinary owner-mode host checkout files are writable from the guest;
- the guest edit appears on the host immediately with the expected host
  ownership and mode, without host chmod/chown;
- the host edit appears immediately in the guest;
- the only Dungeon-requested host bind is the canonical project at
  `/sandbox/project`;
- SELinux private-label behavior permits the intended guest access without
  making normal host use fail unexpectedly.

Exercise stop/start without upload:

```sh
dungeon --config "$CONFIG" down
dungeon --config "$CONFIG" up
dungeon --config "$CONFIG" exec -- tail -n 2 /sandbox/project/probe.txt
```

Verify the same sandbox is reused, the project is still live, and no project
upload occurs.

Repeat the existing hard-containment probes from this runbook. At minimum prove:

- a protected path such as `/etc/shadow` remains denied by the hard Landlock
  policy;
- an unapproved network destination remains denied;
- no Podman/Docker/container-engine socket is available in the guest;
- no unintended host home, credential, SSH, agent database, or broader parent
  path becomes visible.

Verify transfer-mode refusal:

```sh
dungeon --config "$CONFIG" upload --yes
dungeon --config "$CONFIG" export --yes
```

Both commands must refuse locally before contacting OpenShell.

Finally create a separate default/copy config and rerun the existing copy-mode
lifecycle/upload/export regression. Copy mode must remain unchanged.

Record each check as `PASS | FAIL | DEGRADED | NOT TESTABLE`. A successful
static build, plan output, gateway health check, or OpenShell policy readback is
not sufficient evidence for this gate. Do not make live the generated/default
mode until every required ownership, SELinux, policy-denial, visibility, and
copy-regression check above has acceptable live evidence.

## Remote-first service attachment gate

Use a dedicated synthetic live project on the validated local gateway layout
above. Freeze source, image digest, host/client and guest/server versions.
Do not alter `dungeon-local`, import host configuration/credentials, expose an
external listener, or weaken policy to make attachment pass.

1. Run `up`, then a foreground guest service bound to `127.0.0.1` using `exec`.
   For the reviewed OpenCode image: `exec -- opencode serve --hostname
   127.0.0.1 --port 4096`. Keep its generated password out of saved evidence.
2. Run `forward --port 4096 --local-port 14096`. Verify only host loopback is
   listening; no container port publication or SSH-config change is required.
3. Check that unauthenticated requests fail and an authenticated health request
   identifies the guest server. Use vendor-native authentication, not a Dungeon
   credential store or an authentication-disable flag.
4. With a supported native client, select `/sandbox/project` as a **remote**
   location. Confirm that this does not require the directory to exist on the
   host. Record API connectivity separately if native attachment fails.
5. Through the remote service, edit a synthetic file and check it immediately
   from the host editor/filesystem. Edit it on the host and read it remotely.
   Verify owner/group/mode and the single project-bind boundary remain intact.
6. Run a bounded remote AI edit/test only when attachment/authentication work.
   Repeat the policy-denial and no-engine-socket checks. Do not count direct
   `exec` or server filesystem API writes as an AI-task pass.
7. Interrupt forwarding and verify its host listener disappears. Stop the
   server separately, and clean up only this run's resources.

Record `PASS | FAIL | DEGRADED | NOT TESTABLE` per check. VS Code Remote and
vendor SSH/desktop products each need their own record, not a generic SSH or
HTTP pass carried over as support.
