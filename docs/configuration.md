# Configuration

Use one private directory per environment and pass `--config` explicitly. Dungeon never discovers config from a repository.

`dungeon.json` schema 1:

```json
{
  "schema": 1,
  "name": "demo",
  "gateway": "local",
  "workspace": "default",
  "project": "/home/user/projects/demo",
  "image": "ghcr.io/nvidia/openshell-community/sandboxes/base:latest",
  "agent": "opencode",
  "providers": [],
  "policy": "policy.json"
}
```

Names are lowercase DNS labels. `project` must be an absolute project directory, not root/home, and Dungeon config must be outside it. Providers are names, never secret values. Local image build paths and unqualified image names are rejected.

## Native policy

`policy.json` is the single editable OpenShell policy input. Starter profiles are `minimal` and `development`. Both require Landlock `hard_requirement`, the non-root sandbox account, read-only system paths, and writable guest `/sandbox` and `/tmp`.

Dungeon does not widen policy automatically. Inspected REST/TLS endpoints must use `enforcement: enforce`.

Review `plan` before `create`. `runtime.json` is Dungeon-owned identity bookkeeping and must not be edited to bypass mismatch checks.
