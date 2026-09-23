# Local reviewed agent image

Issue #6's Linux image candidate. This is **not published and not Dungeon's default**. Live validation and publication readiness are separate gates.

## Inputs and ownership

- Base: `ghcr.io/nvidia/openshell-community/sandboxes/base@sha256:aeef1c63f00e2913ea002ccb3aaf925f338b5c5d70e63576f0d95c16a138044e`, unchanged from Issue #1. The amd64 member is `sha256:c2a43bb0d765774e2790b3babfb20997bb2eac7b4bf4c6d7d8661e99817bf904`.
- `package.json` owns the exact agent versions and build-only pnpm version. `pnpm-lock.yaml` locks official npm registry packages, including platform binaries, with SHA-512 integrity metadata. Read the manifest for the selected versions rather than maintaining a second version list.
- This glibc Linux image excludes OpenCode's two redundant x64 musl optional binary packages through pnpm's lock-aware `ignoredOptionalDependencies`. The glibc x64 baseline fallback stays installed for CPUs without AVX2.
- OpenCode uses the current V2 `@opencode/cli` distribution, not the legacy `opencode-ai` package. Claude uses Anthropic's explicit **stable** channel (not its faster `latest` channel). Codex uses the current non-prerelease `latest` distribution. Channel names are consulted during review only; builds never resolve a channel.
- Node 22.22.1 comes from the digest-pinned base and satisfies the installers' Node requirements (Claude requires Node >=22; Codex >=16). No extra Node distribution or distro upgrade is installed.

Official sources reviewed on 2026-09-23 UTC:

- [OpenShell base source](https://github.com/NVIDIA/OpenShell-Community/blob/main/sandboxes/base/Dockerfile)
- [OpenCode V2 installation](https://opencode.ai/v2/docs/) and [registry metadata](https://registry.npmjs.org/@opencode/cli)
- [Claude installation and integrity](https://code.claude.com/docs/en/setup#install-with-npm) and [registry metadata](https://registry.npmjs.org/@anthropic-ai/claude-code)
- [Codex installation](https://github.com/openai/codex#installing-and-running-codex) and [registry metadata](https://registry.npmjs.org/@openai/codex)
- [pnpm registry metadata](https://registry.npmjs.org/pnpm)

## Build and check

From the repository root, using rootless Podman:

```sh
podman build --pull=never -t localhost/dungeon-agent:0.0.3-dev \
  -f images/agent/Containerfile images/agent
podman run --rm --network none --entrypoint node \
  localhost/dungeon-agent:0.0.3-dev /usr/lib/node_modules/dungeon-agent/verify.cjs --path
podman history --no-trunc localhost/dungeon-agent:0.0.3-dev
```

Pull the exact `FROM` digest first if it is not cached. No registry publication is needed. `verify.cjs` compares all three CLI version outputs against the manifest, both in the package installation and through the final non-root runtime PATH during the build. The final check removes its transient guest cache, log, and helper paths (absent in the pinned base); it does not leave a Codex session or auth state in the image. Runtime links target vendor-declared binaries directly, not pnpm's location-dependent shell shims.

The pnpm bootstrap archive is SHA-256 checked. Package installation uses the frozen lock with scripts disabled; only the two reviewed native-binary setup scripts run afterward, with network disabled. A missing binary cannot trigger an unlocked fallback download. The final stage copies only the installed tooling, not pnpm's bootstrap or package store. The build context allowlist excludes all other files.

For a version update, edit the manifest, regenerate the lock with its pinned pnpm (`pnpm install --lockfile-only --ignore-scripts` in this directory), review upstream install scripts and integrity metadata, then rebuild and repeat live validation. A pnpm version update also requires reviewing and replacing its bootstrap checksum in the Containerfile.

## Containment contract

The runtime remains `sandbox:sandbox` (UID/GID 998), with `HOME=/sandbox`. Installed tools are root-owned under `/usr/lib/node_modules/dungeon-agent`; `/usr/local/bin` exposes them. This location uses existing Dungeon read-only and executable-network policy paths. Old base OpenCode/Codex/Claude entrypoints are removed, not left as alternate launch paths. Guest state stays in guest-owned paths; the image does not contain host configuration, credentials, browser state, sessions, private keys, or engine sockets.

No model, provider, telemetry override, authentication bypass, or privileged runtime requirement is added. Vendor authentication happens only inside OpenShell. Dungeon still owns coordination, OpenShell enforcement, the image installed versions, and vendors auth/model/session formats.

## Limitations

For the local OpenShell 0.0.116 Podman validation gateway, use OpenShell's
`generate-certs` mTLS/JWT bootstrap. A plaintext gateway without JWT signing
configuration can appear healthy but cannot provision an authenticated sandbox.
When the gateway itself is containerized, TLS paths used by the Podman driver
must also exist at the same host paths, and
`[openshell.drivers.podman].grpc_endpoint` must be reachable by the sandbox.
The host engine socket belongs only to that trusted gateway, never to this image
or its guest. These are validation prerequisites, not a new Dungeon setup flow.

In the tested rootless-Podman OpenShell guest, Codex's own Linux `bwrap` sandbox
cannot create another user namespace. The official CLI offers approval to run
commands outside its *inner* sandbox; approving only the reviewed synthetic
commands kept them inside OpenShell's hard Landlock and network enforcement.
The task passed but required two one-time approvals. This is a usability
limitation of nested sandboxing, not evidence that the CLI's default model is
obsolete. Do not relax OpenShell policy or host namespace settings to mask it.

Claude reached the official login-method screen, but the operator has no
eligible account. Completed authentication, edit/test and credential
persistence are [NOT TESTABLE until Issue #11](https://github.com/Dark-Roast-Cyber/dungeon/issues/11).

- Only the local Linux amd64 image is under validation; lockfile entries for other platforms do not establish support.
- This thin layer inherits the base's packages and lower-layer size. Removing stale tools from the final filesystem does not reclaim their underlying OCI layers or certify the base free of vulnerabilities. In particular, the upstream base source records Node security-update debt for 22.22.1; publication needs a fresh base/runtime security review.
- Official registry integrity pins bytes, not vendor trust or absence of vulnerabilities. This build does not independently verify registry provenance attestations or reproduce vendor native binaries from source.
- OpenCode's installer selects its CPU-specific binary at build time. The local build does not establish portability to older CPUs.
- Guest users can download their own tools into writable state; the image's root-owned install is a reproducible starting contract, not a prohibition on guest execution.
- Do not change `dungeon init` defaults until GHCR naming, signing/provenance, release tags, immutable digest handling, and CI publication permissions are separately approved.
