# npm distribution

Dungeon's npm surface is a launcher for the Rust binary, not a second implementation.

Platform package directories are private source templates because compiled binaries do not belong in the source tree. `stage-packages.mjs` copies already-built native binaries into a temporary package tree, verifies version agreement, removes `private` only from the staged manifests, and keeps the checked-in templates unchanged.

CI performs this staging against the real four-platform binaries and runs `npm pack` for all platform packages plus the launcher. That validates package assembly without contacting the npm registry.

The `@agent-archaeology` scope is provisional until ownership, provenance, and publishing credentials are validated. Do not publish under an unverified scope.
