# Contributing

Dungeon is a Rust binary. Validate changes with:

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
