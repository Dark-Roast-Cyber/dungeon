# Security controls and tradeoffs

Dungeon is an **engineering preview, not independently audited isolation**.
Trust the host administrator, engine, Dungeon, selected image and (when used)
OpenShell. Treat repositories, agent actions, imports and tool output as untrusted.
This guide explains the current controls, not a general security profile.

## Current implementation and defaults

| Runtime | SELinux selection | Filesystem/network policy | Host authority |
| --- | --- | --- | --- |
| Plain rootless Podman, Linux | New bindings default to `disabled`; explicit `private` retains MCS separation | Exact project/import binds; ordinary bridge egress, **no Landlock/OpenShell policy** | UID998 maps to the invoking developer through keep-id |
| OpenShell 0.0.116 + rootless Podman | Private bind labeling; `disabled` refused | Required hard Landlock plus OpenShell process/network policy | Trusted supervisor configures enforcement before the unprivileged workload |
| Standard rootful Docker CE, Linux | Existing private relabel request unchanged; `disabled` refused | Exact project/import binds; ordinary bridge egress, **no OpenShell policy** | Guest uses invoking UID/GID; daemon access is host-root-equivalent |

New human and explicit machine-container bindings use this developer-first
default. Without a saved/explicit Docker choice, offline initialization pins the
new binding to Podman rather than approving disabled labeling and later falling
back to Docker. Setup can still discover/select Docker; explicit
`--selinux-isolation private --engine auto` keeps automatic engine discovery.
Historical omitted fields still mean private and retain their
serialization/fingerprints. Reading or launching them never applies new defaults.
Acceptance uses controlled private-versus-disabled native behavior on an enforcing
host; unavailable AVC logs are a recorded diagnostic limitation, not a prerequisite.
OpenShell 0.1.x remains separately gated; this option does not enable it.

## Why each control exists

| Control | Protection and reason | Limits |
| --- | --- | --- |
| Rootless Podman and keep-id | Container root is not host root; guest project writes keep developer ownership without recursively changing host ownership | The developer's own files and reachable services remain valuable targets. User namespaces do not eliminate kernel bugs |
| Narrow bind scope and guest-private storage | Only the reviewed project is a default writable host bind. Login, sessions, caches and tools' writable state stay in the guest | An agent can destroy/exfiltrate project contents. Guest-only state survives stop/start but is lost on destroy |
| Read-only imports | Reviewed config leaves/private snapshots can be consumed without writing their mounted contents | **Read-only is not secret isolation.** Config/plugins can execute and credentials can be read, used or exfiltrated. Writable guest parents can redirect pathnames |
| SELinux private MCS labels | Add a MAC boundary between container processes and differently labeled host/container objects | Labels are separate from Unix permissions and mount scope. Relabeling mutates host inodes; later moves and competing relabels can break access |
| seccomp | Limits available kernel operations. Plain containers retain the engine's configured default profile; Dungeon does not supply a custom one | Engine configuration matters. OpenShell's pinned driver uses container-level `seccomp=unconfined` so its trusted supervisor can install its own workload filters; do not claim the engine's default profile also applies there |
| Capabilities and no-new-privileges | Plain containers explicitly drop `NET_RAW`, request no-new-privileges and use a non-root workload user; setuid/file capabilities cannot grant new privilege | Not a drop-all capability policy. OpenShell's trusted supervisor needs extra namespace-scoped capabilities during setup and drops workload privilege itself |
| OpenShell hard Landlock and network policy | Independently narrow workload filesystem/process access and approved destinations, including paths otherwise visible in the container | Not present in plain containers. Allowed endpoints can receive data. Policy readback is not kernel attestation or proof every denial path works |
| Mount/PID/network/IPC/UTS namespaces | Separate process views, filesystems and networking from ordinary host operation | Containers share the host kernel. They are not VMs, resource-exhaustion protection or a guarantee against escape |
| Excluding host home, native auth/session databases and engine sockets | Avoid granting broad host/daemon authority or importing unrelated credentials | The trusted OpenShell gateway needs its engine socket; the **workload** must not receive it. Selecting a project that contains secrets exposes those secrets |

These layers are independent. Changing SELinux separation does not turn on
rootlessness, harden networking, grant new mounts or make a plain container an
OpenShell sandbox.

## SELinux: file labels versus process labels

- **Private `:Z`:** relabels bind contents with a container-private MCS category.
  OpenShell's `selinux_label: "private"` requests this bind behavior. Separate live
  environments on the same host directory can relabel each other's files: use
  separate checkouts, not automatic label repair.
- **Shared `:z`:** uses a shared container file label, reducing cross-container
  separation. It still relabels files and does **not** fix arbitrary later moves.
- **`--security-opt label=disable`:** disables SELinux process label separation
  for the one plain Podman workload. Dungeon omits `:Z` on both the project and
  reviewed private snapshots. Read-only snapshots stay read-only; original import
  sources are never relabeled. The existing conservative original-source import
  gate is unchanged: use reviewed copies on enforcing hosts.

A same-filesystem rename preserves the inode and its label. A file moved from
outside an already-private workspace can appear in listings but fail on open.
Copying, host creation and editor atomic replacement have different label
inheritance behavior; test them independently.

Disabling separation removes an extra MAC boundary that could limit damage if
other layers fail, including cross-container access after a mount/namespace
failure. It is not equivalent to private MCS hardening. Host SELinux enforcement,
system policy, `containers.conf`, the OpenShell gateway and unrelated containers
are **not changed**.

The pinned OpenShell driver has no supported per-workload process-label toggle.
Removing `selinux_label` changes only bind relabeling, not process separation.
Parity needs an upstream supported driver control and a native gate retaining
hard Landlock/network denials; Dungeon never patches gateway labels or silently
switches runtimes to simulate it.
This remains an outstanding requirement in
[#49](https://github.com/Dark-Roast-Cyber/dungeon/issues/49), not a claim that
OpenShell moved-file usability is fixed or an intended permanent runtime difference.

## Choose for the workload

| Workload | Practical choice | Residual risk |
| --- | --- | --- |
| Routine local coding in a reviewed checkout | Rootless Podman's new-binding disabled default; review the security reduction before approval | Writable project, unrestricted plain-container egress and shared kernel |
| Inspecting an untrusted repository | Prefer private separation, separate checkout, no credential imports; use supported OpenShell with reviewed policy when filesystem/network restrictions are needed | Private labeling does not stop permitted project damage or exfiltration to allowed services |
| Deliberately malicious code or credible container-escape risk | Use a separate disposable VM/host with no valuable credentials; do not rely on Dungeon alone | Kernel/engine/supervisor vulnerabilities and host resource exhaustion remain outside a container-only promise |

## Select and verify

For a **new** plain Podman binding:

```sh
dungeon init --runtime container --engine podman  # disabled is the new-binding default
# Or retain the additional MCS boundary:
dungeon init --runtime container --engine podman --selinux-isolation private
```

`config --edit --selinux-isolation disabled` also reviews an **uncreated** Podman
binding. Any runtime marker, stopped or running, refuses editing. Initialize a
separate environment/checkout to change an existing choice; never hand-edit a
bound config. Deliberate stop/destroy loses guest-only login, sessions and files;
export copy-mode work first. Destroying retains configuration, not permission to
silently rebuild it with a different mode.
See [adopting the new default for an existing environment](configuration.md#adopt-relaxed-labeling-for-an-existing-environment)
for explicit replacement and rollback options.

`config`, first-use review and `plan` show the request. `status` and machine
schema-1 `environment.selinux_isolation` distinguish `not-inspected` from
`native-configuration`. Plain-Podman status/doctor check native security
options and labels; this is configuration verification, **not kernel attestation**.
OpenShell and Docker report the request without claiming native label verification.

For the exact bound container ID from status, inspect deliberately:

```sh
getenforce
podman container inspect CONTAINER_ID --format '{{json .HostConfig.SecurityOpt}}'
podman container inspect CONTAINER_ID --format '{{.ProcessLabel}} | {{.MountLabel}}'
```

Disabled mode requires native `label=disable` and an empty process label.
Podman may retain a nonempty `MountLabel` for container storage even with process
separation disabled; this is not evidence that host binds were relabeled.
Private mode must not report `label=disable`; on a SELinux-enabled Podman service,
process/mount labels must have matching private MCS categories.

## Troubleshoot without weakening the host

1. Confirm runtime, chosen mode and `getenforce`; a permission error alone is not
   proof of SELinux or Landlock.
2. Compare `ls -Zd PROJECT` and `ls -Z PROJECT/FILE`, including an accessible
   control and the moved inode. Check `stat`, `id`, `namei -l` and `getfacl` for
   UID mapping, parent traversal, modes and ACLs.
3. When available through authorized access, compare the matching AVC
   (`ausearch -m AVC -ts recent`) and its source/target contexts. Record missing
   access as an evidence limitation; do not bypass access controls. Controlled
   private/disabled comparisons can establish the moved-file behavior without
   proving a particular audit event.
4. On OpenShell, separately check policy and its denied-operation diagnostics.
   Do not attribute DAC/ACL, Landlock, EDR or application failures to SELinux
   without evidence.
5. Keep existing environments intact. Do not run `setenforce`, recursive
   `chcon`/`restorecon`, ownership repair, a relabel watcher or global engine
   changes as a Dungeon workaround.

## Primary references and evidence

- [Podman run: labels, mounts, capabilities and security options](https://docs.podman.io/en/stable/markdown/podman-run.1.html)
- [Podman create: per-container security options](https://docs.podman.io/en/stable/markdown/podman-create.1.html)
- [Rootless Podman](https://github.com/containers/podman/blob/main/docs/tutorials/rootless_tutorial.md)
- [Docker daemon attack surface](https://docs.docker.com/engine/security/#docker-daemon-attack-surface)
- [Linux Landlock](https://docs.kernel.org/userspace-api/landlock.html)
- [Pinned OpenShell driver contract](https://github.com/NVIDIA/OpenShell/blob/v0.0.116/crates/openshell-driver-podman/src/container.rs)
  (`PodmanSandboxDriverConfig`, `ContainerSpec`, `build_container_spec_for_image`;
  reviewed tag commit `d1155aa70042d3e2ee49dbfa15346b108b7c1d92`)
- [Security boundary](../SECURITY.md), [configuration](configuration.md#selinux-separation),
  [recorded validation](validation.md) and [native runbook](live-validation.md)
