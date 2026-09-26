# Local reviewed agent image

Issue #6's Linux image candidate, extended by Issue #17 with reviewed image-owned Neovim and Rust capabilities. The [Issue #17 local Linux amd64 validation](../../docs/validation.md#issue-17-local-image-tooling-2026-09-26) passed under the unchanged OpenShell hard policy. This is **not published and not Dungeon's default**; publication readiness is a separate gate.

## Inputs and ownership

- Base: `ghcr.io/nvidia/openshell-community/sandboxes/base@sha256:aeef1c63f00e2913ea002ccb3aaf925f338b5c5d70e63576f0d95c16a138044e`, unchanged from Issue #1. The amd64 member is `sha256:c2a43bb0d765774e2790b3babfb20997bb2eac7b4bf4c6d7d8661e99817bf904`.
- `package.json` owns the exact agent versions, build-only pnpm version, Neovim/Rust versions, Rust distribution coordinates, and reviewed SHA-256 artifact hashes. `pnpm-lock.yaml` locks official npm registry packages, including platform binaries, with SHA-512 integrity metadata. Read the manifest for selected versions and hashes rather than maintaining a second version list.
- Neovim comes from the official Linux x86_64 release archive. Rust uses the official `rustc`, `cargo`, and `rust-std` standalone component archives for the bounded x86_64 GNU/Linux toolchain, without rustup or runtime toolchain resolution.
- This glibc Linux image excludes OpenCode's two redundant x64 musl optional binary packages through pnpm's lock-aware `ignoredOptionalDependencies`. The glibc x64 baseline fallback stays installed for CPUs without AVX2.
- OpenCode uses the reviewed pinned V2 `@opencode/cli` distribution, not the legacy `opencode-ai` package. Claude's pinned version was selected from Anthropic's **stable** channel (not its faster `latest` channel); Codex's pinned version was selected from the non-prerelease `latest` distribution. Channel names are consulted during review only; builds never resolve a channel.
- Node 22.22.1 comes from the digest-pinned base and satisfies the installers' Node requirements (Claude requires Node >=22; Codex >=16). No extra Node distribution or distro upgrade is installed.

Agent/base sources reviewed on 2026-09-23 UTC:

- [OpenShell base source](https://github.com/NVIDIA/OpenShell-Community/blob/main/sandboxes/base/Dockerfile)
- [OpenCode V2 installation](https://opencode.ai/v2/docs/) and [registry metadata](https://registry.npmjs.org/@opencode/cli)
- [Claude installation and integrity](https://code.claude.com/docs/en/setup#install-with-npm) and [registry metadata](https://registry.npmjs.org/@anthropic-ai/claude-code)
- [Codex installation](https://github.com/openai/codex#installing-and-running-codex) and [registry metadata](https://registry.npmjs.org/@openai/codex)
- [pnpm registry metadata](https://registry.npmjs.org/pnpm)

Issue #17 sources reviewed on 2026-09-25 UTC:

- [Neovim v0.12.5 release](https://github.com/neovim/neovim/releases/tag/v0.12.5)
- [Rust 1.98.1 release](https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/) and official distribution host at `https://static.rust-lang.org/dist/2026-09-03/`

## Build and check

From the repository root, using rootless Podman:

```sh
podman build --pull=never -t localhost/dungeon-agent:0.0.3-dev \
  -f images/agent/Containerfile images/agent
podman run --rm --network none --entrypoint node \
  localhost/dungeon-agent:0.0.3-dev /usr/lib/node_modules/dungeon-agent/verify.cjs --path
podman history --no-trunc localhost/dungeon-agent:0.0.3-dev
```

Pull the exact `FROM` digest first if it is not cached. No registry publication is needed. `verify.cjs` compares all three agent CLI outputs against the manifest, then, from the final non-root runtime PATH, checks Neovim, rustc, and Cargo versions, exact image-owned paths, root ownership, non-writability, and `HOME=/sandbox`. The final check removes its transient guest cache, Cargo state, logs, and helper paths; it does not leave a Codex session or auth state in the image. Runtime agent links target vendor-declared binaries directly, not pnpm's location-dependent shell shims.

The pnpm bootstrap archive and all Neovim/Rust archives are SHA-256 checked before extraction or local installer execution. Package installation uses the frozen lock with scripts disabled; only the two reviewed npm vendor native-binary setup scripts run afterward, with network disabled. Rust component `install.sh` files are executed only after their containing official archives pass the reviewed hash check. No remote installer is piped to a shell, no rustup toolchain manager is added, and no mutable Rust or Neovim release channel is resolved. A missing binary cannot trigger an unlocked fallback download. The final stage copies only the installed agent packages and reviewed image-tool filesystem, not pnpm's bootstrap, package store, or downloaded archives. The build context allowlist excludes all other files.

For an agent version update, edit the manifest, regenerate the lock with its pinned pnpm (`pnpm install --lockfile-only --ignore-scripts` in this directory), review upstream install scripts and integrity metadata, then rebuild and repeat live validation. For a Neovim or Rust update, replace only the manifest version/distribution coordinates and reviewed artifact hashes, then repeat the same image and containment validation. A pnpm version update also requires reviewing and replacing its bootstrap checksum in the Containerfile.

## Containment contract

The runtime remains `sandbox:sandbox` (UID/GID 998), with `HOME=/sandbox`. Agent packages are root-owned under `/usr/lib/node_modules/dungeon-agent`, with `/usr/local/bin` exposing their reviewed entrypoints. Neovim installs under `/usr/local/bin/nvim`, `/usr/local/lib/nvim`, and `/usr/local/share/nvim`; Rust installs `/usr/local/bin/rustc`, `/usr/local/bin/cargo`, and `/usr/local/lib/rustlib`. These locations are root-owned and non-writable to `sandbox`, and already fall under Dungeon's existing read-only `/usr` policy and executable-network paths. No policy expansion is required. Old base OpenCode/Codex/Claude entrypoints are removed, not left as alternate launch paths.

With `HOME=/sandbox`, Neovim uses its standard HOME-derived data, state, and cache paths (`/sandbox/.local/share/nvim`, `/sandbox/.local/state/nvim`, `/sandbox/.cache/nvim`), and Cargo uses `/sandbox/.cargo`; project build output stays under `/sandbox/project` or another guest-writable `/sandbox` path. OpenShell 0.0.116 sanitizes child-process environments, so OCI image `ENV` is not a guest command environment contract. No XDG or Cargo overrides are needed for these tested workloads; bounded guest environment configuration belongs to Issue #19. Issue #17 does not import a real Neovim config or Cargo credentials. The image does not contain host configuration, credentials, browser state, sessions, private keys, registry auth, project files, or engine sockets.

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
