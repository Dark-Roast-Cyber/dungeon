# Architecture

Dungeon owns setup and lifecycle coordination. OpenShell owns workload
isolation and policy enforcement. Agent interfaces own their conversations.
Telltale owns interpretation of supported session sources. None substitutes
for the others.

```text
Host CLI / guided setup
  -> explicit configuration + native policy
  -> pinned OpenShell adapter
  -> managed environment: agents, tools, project, session state
       ^ terminal / optional SSH editor or desktop connection
       -> optional Telltale diagnostic
```

## Implemented in 0.0.1

`config.py` validates explicit input and generates starter policy documents.
`runtime.py` is the only subprocess boundary. It constructs argument vectors,
selects an explicit gateway/workspace, limits inherited host environment, and
checks the pinned CLI version. `cli.py` coordinates user commands. No backend
interface hierarchy, plugin ABI, web service, or separate policy language is
introduced for a single adapter.

Initialization does not execute project code or contact a runtime. Creation
uses a private temporary policy copy, no automatic credential-provider creation,
and an explicit configuration label. It then records the returned sandbox ID
and a digest of the observed policy in host-side `runtime.json`. Reuse verifies
the ID, name, administrative workspace, configuration label, and policy digest.
An unchanged label alone is not evidence of unchanged policy.

The observed digest detects later policy changes. It is not attestation that
the kernel enforced those rules, nor proof that a mutable image or provider
configuration stayed unchanged. Creation rejects a global-policy override;
organization-wide global-policy adoption needs a separate validated contract.
The host, gateway, and their administrators remain trusted.

Configuration fields bind creation inputs. A changed configuration requires
restoring the original or creating a differently named environment. Policy
updates are not hot-reloaded by Dungeon. Stop checks identity but permits
backend policy drift, so a changed policy does not prevent stopping the known
instance. Failed create/readback can leave a sandbox without a local binding;
there is deliberately no automatic adoption or destructive cleanup.

## Storage ownership

| Data | Owner and lifetime |
| --- | --- |
| Host project | User-owned; never mounted by Dungeon. Upload is explicit. |
| Guest project | Backend-owned private copy; no automatic export. |
| Agent sessions | Guest agent-owned; persist according to driver lifecycle. |
| Config and binding | Host-side private environment directory. |
| Credentials | Explicit OpenShell providers or user-approved guest login. |
| Diagnostic output | Foreground output only; not a retained audit trail. |

No host dotfiles, editor caches, agent databases, skills, private SSH keys,
engine sockets, or credential directories are copied automatically. Tool
composition currently uses a reviewed compatible image. Declarative features
and selective configuration imports are future work, not hidden setup hooks.

## Telltale boundary

The optional diagnostic uses the standalone Telltale CLI. Dungeon does not
import Rust internals, add client parsers, manufacture execution facts from
transcripts, or change Telltale's event contracts. A compatible image supplies
the binary. A missing binary fails visibly through the remote exit code.

The next integration slice should move collection outside agent write authority,
with explicit source roots, environment/session correlation, redaction, retained
delivery, and degraded-health reporting. Agent-authored source data remains
untrusted even when collected externally. Backend enforcement logs complement
transcripts; they do not replace them or imply a complete conversation.

No perpetual old/new model bridge or duplicated detector is justified. Follow
[Telltale's own architecture](https://github.com/Dark-Roast-Cyber/telltale/blob/main/docs/architecture.md)
and [roadmap](https://github.com/Dark-Roast-Cyber/telltale/blob/main/ROADMAP.md)
when selecting the future collector contract.
