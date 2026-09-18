# Configuration

Use one private directory per environment. Pass `--config` explicitly before
the command; Dungeon never discovers configuration from a repository. Files
are created without overwrite. The directory is requested as mode 0700 and
files as 0600 on POSIX; Windows ACLs require separate host administration.

`dungeon.json` schema 1 has exactly these fields:

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

`workspace` is OpenShell's administrative scope, not the project path.
`project` must be an absolute directory, not the filesystem root or home. It
must exist for initialization/upload; lifecycle access still works after the
host copy is removed. Configuration must be outside it. Names are lowercase DNS
labels. Providers are names of preconfigured credential providers, not values.
Local Dockerfiles/build contexts and unqualified image names are rejected.

`agent` selects the default for `run`; it does not prohibit `exec` or another
agent. This is execution containment, not a command-name allowlist.
The image must provide the `sandbox` user/group, `/sandbox`, `/bin/bash`, and
the selected tools, and satisfy OpenShell's compatible-image requirements.
A custom digest is preferable to the mutable convenience default.

## Native policy

`policy.json` uses JSON syntax accepted by OpenShell's YAML parser. It is the
single editable policy input, not generated afresh on every launch. The wizard
writes one of two starting points:

| Starter | Allowed endpoint groups |
| --- | --- |
| `minimal` | Common model API/authentication and OpenCode metadata hosts. |
| `development` | Minimal plus selected GitHub, package-registry, and VS Code hosts. |

Both request mandatory Landlock, the non-root sandbox account, read-only
system paths, and writable guest `/sandbox` and `/tmp`. Endpoint lists are
starting points, not an exhaustive compatibility catalog. For example, a
provider's extra authentication hosts or an extension-specific CDN can require
an explicit reviewed addition before creation. No wildcard host approval or
automatic policy widening occurs.

The starter endpoints use host/port rules, not inspected HTTP method/path
restrictions. They permit use of allowed services, including possible data
upload. When adding inspected REST/TLS rules, Dungeon requires
`enforcement: enforce`, not audit-only mode. Backend schema validation remains
OpenShell's responsibility. There is no guaranteed offline profile in 0.0.1.

Review the complete `plan` output before `create`. It reports exact creation
arguments, policy, configuration fingerprint, and unverified status without
contacting a backend. The actual create operation snapshots that same in-memory
policy into a temporary private file to avoid a policy-file reread race.

`runtime.json` is runtime-owned identity bookkeeping, not user configuration.
Do not edit it to bypass mismatches. Stop and investigate using the original
configuration or the explicitly scoped upstream CLI. Changing a provider's
credentials or a mutable image is not detected by the configuration digest.
