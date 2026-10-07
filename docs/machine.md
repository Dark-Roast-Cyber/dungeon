# Machine interface, schema 1

Development-preview contract for external harnesses. One Rust Runtime and the
existing policy, identity, transfer and owned-process checks remain authoritative.
This is not an SDK, daemon, remote execution protocol or orchestrator.

After [building the CLI and setting up the gateway](../README.md#quick-start), build
the [local agent image](../images/agent/README.md) before selecting it below.
Run these examples from a project directory, not your home or filesystem root.
The `--yes` flags are explicit demonstration approvals: first `up` uploads the
project and `export --yes` applies reviewed guest changes to the host.

```sh
PRIVATE="$HOME/.config/dungeon/demo"
dungeon machine --schema 1 resolve --project "$PWD"
dungeon machine --schema 1 init --yes --directory "$PRIVATE" --project "$PWD" \
  --name demo --gateway dungeon-local --image localhost/dungeon-agent:0.0.3-dev
dungeon --config "$PRIVATE/dungeon.json" machine --schema 1 up --yes
dungeon --config "$PRIVATE/dungeon.json" machine --schema 1 status
```

Machine exec requires a pipe, not terminal stdout. This Bash example drains raw
base64 JSONL for manual inspection; `cat` is not a production protocol consumer.
`pipefail` preserves a nonzero Dungeon exit instead of hiding it behind `cat`:

```bash
set -o pipefail
dungeon --config "$PRIVATE/dungeon.json" machine --schema 1 exec --owned -- \
  node -e 'process.stdout.write("hello\n")' | cat
```

A harness must drain incrementally, validate the complete JSONL and compare the
`finished` event with the CLI exit status (including workload exit 23); see
[completion rules](#execution-bytes-and-completion). Export and close remain
separate deliberate operations:

```sh
dungeon --config "$PRIVATE/dungeon.json" machine --schema 1 export --yes
dungeon --config "$PRIVATE/dungeon.json" machine --schema 1 close --preserve
```

## Selection and operations

`--config PATH` explicitly selects a configuration. Otherwise the current canonical
project resolves its deterministic private binding. Only `resolve` accepts an
explicit `--project`; combining it with `--config` is an error. Project/repository
content never supplies authority. Resolve is read-only: missing binding returns
`bound: false`, not initialization. Explicit initialization writes the selected
private directory, not a binding registry; an arbitrary directory requires later
`--config` selection. Copy is the generated default. Live transfer remains explicit
and subject to the existing gateway and project guards.

| Operation | Contract |
| --- | --- |
| `resolve` | Loaded configuration and capabilities, without runtime contact |
| `init --yes` | Existing private-config writer; explicit directory, project, name, gateway and image |
| `create --yes` | Create or verify; no implicit project upload |
| `up --yes` | First copy upload; later resume/no-op without refresh |
| `start`, `stop` | Identity/policy-checked preservation |
| `status` | Bound runtime observation, including Stopped; no selected-artifact execution |
| `close --preserve` | Stop only; no deletion fallback |
| `upload --yes` | Deliberate whole-project copy refresh, potentially destructive to guest edits |
| `export [--yes]` | Existing whole-project three-way review, locks, conflicts and recovery |
| `exec [--owned] [--cwd PATH] [--stdin] -- ARG...` | Non-TTY JSONL, described below |
| `interactive --owned -- ARG...` | Existing owned native SSH, real foreground terminal only |

There are no machine prompts, including on a terminal. Export without approval
returns structured reviewed changes and `approval_required` rather than applying.
An unchanged export needs no approval. Review entries are ordered
`{"path": STRING, "change": "ADD|MODIFY|DELETE|CONFLICT"}`; paths are JSON data,
not diagnostic text or shell expressions. All existing transfer limitations apply.

## Ordinary JSON

Except execution, successful operations produce one line:

```json
{"schema":1,"operation":"status","ok":true,"result":{"bound":true,"environment":{"config":"/private/dungeon.json","project":"/project","fingerprint":"opaque","sandbox_id":"opaque","guest_project":"/sandbox/project","execution_location":"local","project_transfer":"copy"},"phase":"ready","identity":"verified","policy":"verified","capabilities":{},"review":null}}
```

Environment paths are authoritative configuration data. Sandbox IDs and fingerprints
are opaque strings; `sandbox_id` is null before runtime inspection. Resolve does not
assert runtime identity or policy: its phase is `uncreated`, identity `configured`,
policy `unknown`, even if an unobserved runtime already exists. An unbound resolve
uses null environment/capabilities, phase `unbound`, identity/policy `unknown`.
Observed phases are `ready`, `stopped`, `error`, `transitioning` or `unknown`.

`initial_upload_pending` is a bound-state boolean, or null without runtime
inspection (including resolve/uncreated/unbound). It does not redefine backend
phase: Ready can still be pending. Pending first-copy initialization blocks new
execution, attachment and automatic start with `operation_failed`, effects `none`
before launch. Explicit start, status, stop and existing owned cleanup remain
available. Inspect and recover deliberately with Ready-only `upload --yes`;
reused up/create never retry automatically. False includes explicit create and
legacy state and does not prove an export baseline exists. A failed completion
directory sync has unknown effects and may leave false already visible; there is
no rollback guarantee.

Capabilities each contain string `state` and nullable string `reason`. States are
`supported`, `unsupported`, `unknown` or `degraded`. Supported describes the contract,
not observed prerequisite availability or containment attestation. Streaming/owned
execution and interaction remain unknown until invoked with the required Linux,
FIFO, guest Node/proc or foreground-terminal prerequisites. Unowned interruption
is degraded. Non-Linux streaming, owned execution, interactive and finite stdin
are categorically unsupported; native Windows copy upload/export are unsupported
because their private transfer locks require Unix. Live stdin, selective transfer, deletion and remote execution are
unsupported. Whole-project copy upload/export do **not** fit Sandcastle's unchanged
arbitrary-path copy and automatic patch/sync handlers; #21 remains separate work.

Failures exit 1 and produce a fixed diagnostic, never raw native/parser errors:

```json
{"schema":1,"operation":"upload","ok":false,"error":{"code":"approval_required","message":"Explicit approval is required.","effects":"none"},"review":null}
```

Codes are `invalid_request`, `approval_required`, `unbound`, `unsupported`, `busy`,
`conflict`, `identity_mismatch`, `policy_drift`, `cleanup_unconfirmed`, and
`operation_failed`. Effects are `none` only for classified refusals before a
potential mutation. Once a native lifecycle/create/transfer, staged download or
execution/interactive spawn
boundary begins, subsequent failures retain their category but use `unknown`,
including failed post-operation inspection. This is not a transaction receipt. Unclassified runtime failures
use `operation_failed`; messages cannot be used to infer native failure details.
Malformed requests, unsupported schema/flags and non-UTF-8 arguments are sanitized
before the human CLI error path. Unparsed operations use `operation: "request"`.
Explicit help/version are intentional human-output exceptions. Output failure
exits nonzero without dumping diagnostics to stderr.

## Execution bytes and completion

Machine exec supports **Linux stdout FIFOs only**. Regular files, sockets, null
devices and terminals are refused before workload launch; there is no blocking
fallback. A consumer must drain incrementally. The host reopens the same FIFO
nonblocking without changing the inherited stdout open-description flags. The
host queue is capped at 64 KiB; guest reads pause behind bounded async chunk writes.
A sink stalled for two seconds becomes a terminal output fault.

Events have schema 1, operation `exec`, and monotonically increasing `seq`:

```json
{"schema":1,"operation":"exec","seq":0,"event":"started","sandbox_id":"opaque"}
{"schema":1,"operation":"exec","seq":1,"event":"output","stream":"stdout","encoding":"base64","data":"AP8="}
{"schema":1,"operation":"exec","seq":2,"event":"finished","ok":true,"exit_code":23,"error":null}
```

Started means the primary transport was spawned, not that the executable started.
The entire started event must fit the bounded output queue before spawning;
oversized opaque identity projections refuse without launch. Postspawn descriptor
initialization faults still enter the same owned STOP/receipt cleanup loop.
Output frames preserve byte order per stream. Cross-stream observation order is
not application chronology. Every output byte is workload data, including legitimate
Node warnings, arbitrary binary, secret text and strings resembling protocol frames.
Consumers must treat decoded bytes as untrusted. Native/guardian diagnostics are
not output frames. Guest frames are strict, bounded and cannot authorize signalling.

Finished success requires parsed EOF, fully drained output, a separate verified
receipt and matching primary transport exit. `exit_code` is the workload's code;
the CLI exits with it, including nonzero 23. `ok` describes transport completion,
not workload success. Faults use `ok: false`, null exit code and the fixed error
object. Receipt success cannot override parser, cancellation, output or transport
failure. Machine INT/TERM is sticky through teardown and final publication: it
exits 1 with a failure event, never a synthetic successful exit 130/143. Human
`exec --owned` retains its existing 130/143 signal semantics. No machine signal
handler is removed before final delivery completes. The publication decision
checks observed cancellation before the first final-frame write. If a later
signal arrives after final bytes have begun writing, those bytes cannot be safely
rewritten: the CLI exits 1, may leave an incomplete or already-published success
event, and never sends a second finished event. Consumers must check both complete
JSONL and the CLI exit; a final event alone is not a completion acknowledgement.
A failed consumer may receive no final event; final delivery is bounded
best effort, never a reason to block cleanup. Interrupted/broken streaming exits 1.

`--owned` reuses the existing private guardian/control/receipt owner. Group absence
and control removal are checked independently of output draining. It supplies no
stdin. Ordinary unowned execution has no guest-cleanup guarantee on interruption
or lost transport. Neither mode strengthens containment or handles same-UID guest
sabotage, group escape or host SIGKILL.

Copy upload/export verify selected import snapshots in the shared Runtime before
transfer locks, baseline changes or native transfer. Missing/unapproved/tampered
artifacts refuse both human and machine transfers; status, stop and existing
owned cleanup remain artifact-independent.
Human and machine work dispatch also verify imports before constructing Runtime
(including its OpenShell version probe). The Runtime checks remain a recheck at
the operation boundary, not a cached preflight result.

`--stdin` is unowned only: at most **65,536 bytes**, read completely before workload
launch, then supplied through native buffered OpenShell stdin. More bytes refuse.
OpenShell's full decoded-request cap means its nominal 4 MiB CLI input cap is not
a usable payload contract. No bidirectional/live-input claim is made.
`--cwd` must be normalized absolute `/sandbox/project` or a descendant with no
empty, dot or parent components. This is lexical scope, not symlink confinement.
ARGV is data, never host-shell evaluation. Request a guest shell explicitly with
`-- /bin/sh -c '...'` when needed.

Interactive is deliberately **not JSONL**: after checks it hands the terminal to
native owned SSH, with raw application input/output/resize. Preflight/final errors
retain fixed machine diagnostics; clients must not parse a successful terminal
stream as protocol JSON. Redirected/nonterminal interaction is unsupported.
