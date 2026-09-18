# npm distribution

Dungeon's npm surface is a launcher for the Rust binary, not a second implementation.

Platform package directories are private source templates because this repository does not contain compiled binaries. Release tooling must stage the matching binary into `bin/`, remove `private`, verify the binary version, and publish platform packages before the launcher.

The `@agent-archaeology` scope is provisional until ownership and publishing credentials are validated. Do not publish under an unverified scope.
