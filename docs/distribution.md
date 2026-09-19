# Distribution

The Rust `dungeon` binary is the only implementation. Every distribution path must execute that binary and must not reimplement policy, authentication, configuration, or runtime behavior.

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

Intended convenience surface:

```sh
npx @agent-archaeology/dungeon
npm install -g @agent-archaeology/dungeon
```

The launcher resolves an OS/CPU-specific optional dependency and executes its bundled Rust binary with `shell: false`. There is no postinstall downloader.

Checked-in platform manifests remain `private` templates. CI downloads the real four-platform binaries it just built, runs `npm/stage-packages.mjs` into a temporary tree, removes `private` only in that staged tree, then runs `npm pack` for the four platform packages and the launcher. The resulting five tarballs are retained as CI artifacts.

This proves package assembly without contacting npm and without putting compiled binaries in the source tree.

Registry publication is intentionally not implemented yet. The provisional `@agent-archaeology` scope must first have verified ownership, provenance, and publishing credentials. When publication is approved, platform packages must publish before the launcher so optional dependencies resolve immediately.

Unsupported architectures fail explicitly rather than silently downloading or falling back to another runtime.
