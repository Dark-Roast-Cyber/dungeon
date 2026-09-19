# Configuration

Use one private directory per environment and pass `--config` explicitly. Dungeon never discovers config from a repository.

`dungeon.json` schema 2:

```json
{
  "schema": 2,
  "name": "demo",
  "gateway": "local",
  "workspace": "default",
  "project": "/home/user/projects/demo",
  "image": "ghcr.io/nvidia/openshell-community/sandboxes/base:latest",
  "agent": "opencode",
  "providers": []
}
```

Schema 2 deliberately removes the old `policy` field. `policy.json` is always the adjacent policy file, so making its path configurable created an invalid state without adding capability. This pre-1.0 preview does not carry schema-1 compatibility code.

Names are lowercase DNS labels. `project` must be an absolute project directory, not root/home, and Dungeon config must be outside it. Providers are names, never secret values. Local image build paths and unqualified image names are rejected.

## Native policy

`policy.json` is the single editable OpenShell policy input. `dungeon init` generates one starter development policy. Additional named profiles are deferred until live validation establishes real scenario-specific semantics.

The starter policy requires Landlock `hard_requirement`, the non-root sandbox account, read-only system paths, writable guest `/sandbox` and `/tmp`, model endpoints, common development package endpoints, GitHub, and VS Code endpoints.

Dungeon does not widen policy automatically. Inspected REST/TLS endpoints must use `enforcement: enforce`.

Review `plan` before `create`. `runtime.json` is Dungeon-owned identity bookkeeping and must not be edited to bypass mismatch checks.
