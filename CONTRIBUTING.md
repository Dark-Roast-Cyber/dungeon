# Contributing

Dungeon is a Rust binary. Validate changes with:

Install Node 24 on a trusted host PATH for the owned-execution Rust fixture
and Node tests. The fixture translates the fixed guest `/usr/bin/node` path to
that host test executable; it does not require Node at `/usr/bin/node` on the
host. This is a test-only prerequisite, not a host Dungeon runtime dependency.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
node --test npm/launcher/test/platform.test.mjs
node --test tests/owned.test.cjs
```

Keep changes independently reviewable. Include a concrete contract or failure mode when proposing abstractions, backend support, or policy changes. Never commit real credentials or personal session stores.

The npm tree is a launcher plus private platform-package templates. It is not a second implementation.
