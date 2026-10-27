# Distribution

The Rust `dungeon` binary is the only implementation. Every distribution path must execute that binary and must not reimplement policy, authentication, configuration, or runtime behavior.

## Current availability

Rust 0.0.2 on main is source-only; follow the [source build](../README.md#1-build-the-cli).
The optional [local install helper](../scripts/install-local.sh) builds that
same binary with locked Cargo dependencies and installs it to a user bin
directory. It neither publishes a release nor installs/starts OpenShell.
CI builds native archives and npm tarballs, but no Rust 0.0.2 release or npm
package is published. The latest published release, v0.0.1, contains the older
Python wheel/source archive and does not implement this Rust manual. The
[reviewed agent image](../images/agent/README.md) also requires a local build.

## Clean local uninstall

The Rust CLI's `uninstall` supports standalone plain-container installations and
the Linux/rootless-Podman OpenShell setup with the user-local install helper.
It is not a package-manager uninstaller.

For plain containers, it inventories the recorded native services, rejects
unrecorded Dungeon containers, stops/removes recorded containers, verifies absence,
and removes private configuration and the selected CLI. Projects, original imports,
cached images and unrelated containers remain. Mixed installations require
destroying plain containers before OpenShell teardown.
An empty, private Dungeon state directory is removed by standalone plain cleanup;
nonempty or unknown state still requires inspection and is not silently deleted.
An interrupted create with a recorded intent is included in native inventory and
can be removed when exactly one container matches. If the result is absent or
ambiguous, uninstall retains the intent rather than guessing that the service has
finished processing it.

`uninstall --binary-only` needs no engine or OpenShell and removes only the selected
CLI. Configuration/resources remain; cleanup is reported unverified. Ordinary
uninstall takes this same path when Dungeon config/state are absent.

The following detailed teardown sequence applies to OpenShell installations:

1. Close all Dungeon clients and other lifecycle/transfer commands. Export needed
   copy-mode edits before removal; uninstall does not export or back up anything.
2. Run the installed binary's `uninstall --dry-run`. Review the exact container
   IDs and paths. Keep OpenShell 0.0.116, Podman, its API socket and the pinned
   gateway image available until teardown finishes. Start a stopped gateway
   explicitly if needed; uninstall never starts or repairs it.
3. Run `uninstall` and type `uninstall dungeon`, or use `uninstall --yes` for a
   disposable test environment. Custom installs need `--bin-dir ABSOLUTE_PATH`
   on both invocations.
4. Verify the selected binary, Dungeon roots, registration, recorded container
   IDs and `dungeon-local` network are absent using the acceptance gate below.

Removal order: identity-checked stop/destroy for every valid private local binding;
all-workspace inventory and native container/workspace-volume/secret absence
checks; exact recognized gateway container removal; non-forced empty-network
removal; OpenShell-owned registration removal; private Dungeon config/state;
binary last. OpenShell 0.0.116 leaves client mTLS files behind after removing
metadata; uninstall explicitly removes only that reviewed gateway's private
registration/TLS directory (including OpenShell's `last_sandbox` selection file)
after confirming metadata absence.
A different installed binary is not executed or removed. Dry-run
does not create a lock or initialize Podman storage. Confirmation is mandatory.
OpenShell deletion can be asynchronous: uninstall waits up to 30 seconds of
rechecking for recorded sandbox/resource absence before proceeding. New unknown
resources or failed inventory queries still cause refusal, never a forced prune.

The setup lock prevents another fresh setup during removal; it is **not** a
maintenance lease against every Dungeon/OpenShell/Podman client. Quiesce them
first. Removal is not transactional: a failure can leave some sandboxes already
deleted. Inspect the reported partial state before retrying; no broad prune,
orphan adoption, secret copying, dependency uninstall or secure-erasure claim.
An `uninstall-incomplete.json` marker in the Dungeon state root retains the
reviewed container/sandbox IDs before destruction. A failed removal blocks
automatic retry, so retiring runtime bindings cannot hide residual resources.
Inspect those IDs and remaining resources before manually clearing the marker.

| Removed | Preserved / outside scope |
| --- | --- |
| `${XDG_CONFIG_HOME:-$HOME/.config}/dungeon` | Host projects and original imported configuration |
| `${XDG_STATE_HOME:-$HOME/.local/state}/dungeon` | Cached OCI images and the shared Podman store/socket |
| `openshell/gateways/dungeon-local` metadata via OpenShell, then its private client TLS directory | Other gateways, containers, volumes and credentials |
| Recognized local gateway, recorded sandbox containers and their native guest state | OpenShell, Podman, toolchains, source checkout/builds and shell PATH settings |
| Selected user-local `dungeon` binary | Package-manager installs (use that manager), arbitrary custom `init --directory` trees |

Invalid bindings, unknown network containers and unbound/foreign gateway
sandboxes block removal **before destruction**, including resources in other
workspaces. External-gateway bindings must be destroyed explicitly first.
Custom config directories are not searched or erased; destroy their sandboxes
with `--config PATH` first and remove the private directory separately after
inspection. Unknown/unsafe files or incomplete legacy setup can also require
manual recovery. Preserve adjacent export-recovery material until inspected.

### Disposable native acceptance gate

In a dedicated disposable Linux VM/user, prepare Rust, Git, OpenShell 0.0.116,
rootless Podman with its API socket, and the two documented images. Begin with no
Dungeon state and no containers. From this checkout:

```sh
DUNGEON_UNINSTALL_TEST=disposable-user sh tests/uninstall-live.sh
```

For a VM without a Rust toolchain, build the locked release on a compatible host
and supply `DUNGEON_UNINSTALL_TEST_BINARY=/absolute/path/to/dungeon`. The test
installs that prebuilt native binary instead of invoking the source installer;
record its hash with the results. This validates native install/removal, not a
build inside the VM.

This installs the actual release binary in a custom user-local bin directory,
creates a fresh gateway, a Ready live-project sandbox and a Stopped copy sandbox,
and exercises dry-run then approved uninstall through the installed binary.
Independent native checks assert every recorded Dungeon container ID is gone,
network/registration/config/state/binary are absent, and no workspace volumes or
secrets were left behind. A separate sentinel container's ID/state/start time,
another active gateway registration, images, prerequisite CLIs and host project hashes must
survive. Evidence and projects remain in the printed test directory. Failures
retain remaining resources for inspection rather than pruning them.
The gate also reinstalls to the default `~/.local/bin/dungeon` location and
removes that binary-only installation after the gateway has already been removed.

Synthetic Rust coverage is not a substitute for this native gate. Do not run it
against your normal account or existing gateway. It is opt-in, not part of CI.

## Native releases

CI packages the same native targets that it compiles:

- Linux x64: `dungeon-VERSION-linux-x64.tar.gz`
- macOS arm64: `dungeon-VERSION-darwin-arm64.tar.gz`
- macOS x64: `dungeon-VERSION-darwin-x64.tar.gz`
- Windows x64: `dungeon-VERSION-win32-x64.zip`

Each archive contains only the native binary, `LICENSE`, and `README.md`.

A tag matching `vMAJOR.MINOR.PATCH` triggers `.github/workflows/release.yml`. The workflow verifies the tag exactly matches the package version in `Cargo.toml`, runs locked quality/tests once, rebuilds all four native targets, creates the archives, generates `SHA256SUMS`, and publishes a GitHub prerelease.

Creating a tag is therefore the native-release authorization boundary. Ordinary pushes and pull requests never create a GitHub release.

The release workflow intentionally rebuilds rather than promoting an earlier CI binary so every published asset is tied directly to the tagged source tree and locked dependency graph.

## npm / npx

Planned commands, not currently usable (the package is unpublished):

```sh
npx @agent-archaeology/dungeon
npm install -g @agent-archaeology/dungeon
```

The launcher resolves an OS/CPU-specific optional dependency and executes its bundled Rust binary with `shell: false`. There is no postinstall downloader.

Checked-in platform manifests remain `private` templates. CI downloads the real four-platform binaries it just built, runs `npm/stage-packages.mjs` into a temporary tree, removes `private` only in that staged tree, then runs `npm pack` for the four platform packages and the launcher. The resulting five tarballs are retained as CI artifacts.

This proves package assembly without contacting npm and without putting compiled binaries in the source tree.

Registry publication is intentionally not implemented yet. The provisional `@agent-archaeology` scope must first have verified ownership, provenance, and publishing credentials. When publication is approved, platform packages must publish before the launcher so optional dependencies resolve immediately.

Unsupported architectures fail explicitly rather than silently downloading or falling back to another runtime.
