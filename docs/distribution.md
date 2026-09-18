# Distribution

The Rust `dungeon` binary is the only implementation. Every distribution path must execute that binary and must not reimplement policy, authentication, configuration, or runtime behavior.

## Native releases

Release builds should produce immutable archives for Linux x64, macOS arm64/x64, and Windows x64 with checksums. Direct installers/package managers can consume those artifacts later.

## npm / npx

Intended convenience surface:

```sh
npx @agent-archaeology/dungeon
npm install -g @agent-archaeology/dungeon
```

The launcher resolves an OS/CPU-specific optional dependency and executes its bundled Rust binary with `shell: false`. There is no postinstall downloader.

Platform package directories are deliberately `private` source templates because the repository does not contain compiled binaries. Publication tooling must stage and verify the native binary before removing `private` and publishing platform packages, then publish the launcher.

The `@agent-archaeology` npm scope is provisional until ownership, provenance, and publishing credentials are explicitly validated. Ordinary CI does not publish npm packages.

Unsupported architectures fail explicitly rather than silently downloading or falling back to another runtime.
