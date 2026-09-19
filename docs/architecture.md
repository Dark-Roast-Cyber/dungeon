# Architecture

Dungeon owns setup and lifecycle coordination. OpenShell owns workload isolation and policy enforcement. Agent interfaces own conversations and credential stores. Telltale integration is intentionally deferred until evidence can be collected independently of the workload.

```text
Host CLI
  -> explicit configuration + native OpenShell policy
  -> pinned OpenShell adapter
  -> managed environment: agents, tools, project, session state
       ^ terminal / optional SSH editor or desktop connection
```

## One implementation

`dungeon` is one Rust binary with four plain responsibility modules:

- `cli`: command parsing and dispatch.
- `config`: config/state parsing, validation, strict JSON, and fingerprints.
- `policy`: the starter policy and policy safety validation.
- `openshell`: the single OpenShell subprocess boundary and lifecycle operations.

There is no backend hierarchy, daemon, plugin ABI, embedded JS runtime, second policy language, or framework layer.

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

No host dotfiles, editor caches, agent databases, private SSH keys, engine sockets, or credential directories are copied automatically.

## Telltale boundary

There is no provisional in-guest Telltale command. The accepted future integration is independent collection with retained evidence and explicit source health. Dungeon should not add a weaker compatibility path before that work is validated.
