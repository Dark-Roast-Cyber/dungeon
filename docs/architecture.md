# Architecture

Dungeon owns setup and lifecycle coordination. OpenShell owns workload isolation and policy enforcement. Agent interfaces own conversations and credential stores. Telltale owns interpretation of supported session sources.

```text
Host CLI / guided setup
  -> explicit configuration + native OpenShell policy
  -> pinned OpenShell adapter
  -> managed environment: agents, tools, project, session state
       ^ terminal / optional SSH editor or desktop connection
       -> optional Telltale diagnostic
```

## One implementation

`dungeon` is one Rust binary. Configuration/policy validation, CLI behavior, and the OpenShell subprocess boundary live in that implementation. There is no backend hierarchy, daemon, plugin ABI, embedded JS runtime, or second policy language.

The npm tree is distribution only: a tiny Node launcher selects a platform package and executes its Rust binary. It must never parse Dungeon config, apply policy, coordinate auth, or contact OpenShell.

## Runtime identity

Initialization does not execute project code or contact a runtime. Creation snapshots the loaded policy, disables automatic provider creation, and labels the sandbox with a configuration fingerprint. Dungeon records sandbox ID and observed-policy digest in host-side `runtime.json`.

Reuse verifies identity and policy drift. These checks are not kernel/image attestation and do not prove provider content stayed unchanged.

## Storage ownership

| Data | Owner |
| --- | --- |
| Host project | User-owned; never mounted by Dungeon |
| Guest project | Backend-owned explicit copy |
| Agent sessions | Guest agent-owned |
| Config/runtime binding | Private host directory |
| Credentials | OpenShell providers or vendor-owned guest login |
| Telltale diagnostic | Foreground only |

No host dotfiles, editor caches, agent databases, private SSH keys, engine sockets, or credential directories are copied automatically.

## Telltale boundary

The optional diagnostic uses the standalone Telltale CLI. Dungeon does not add parsers, change Telltale event contracts, or manufacture execution facts. Independent external collection remains a separate accepted integration slice.
