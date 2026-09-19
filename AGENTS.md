# Engineering guidance

Read README, SECURITY, docs/architecture.md, and docs/validation.md before changing runtime behavior.

Keep one Rust implementation, one policy owner, and one OpenShell subprocess boundary. The current module split is responsibility-based, not an abstraction framework. npm is distribution only and must never reimplement Dungeon behavior. Do not add a backend trait, daemon, plugin ABI, compatibility layer, or credential copier without an independently useful contract.

Treat project configuration, paths, logs, and tool output as untrusted. Never evaluate host shell snippets from them. No automatic host-home sharing, engine sockets, native auth/session database synchronization, or policy widening.

Prefer deletion to compatibility layering. Update focused tests and authoritative docs with behavior changes. Use synthetic fixtures only.

Validate with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and npm launcher tests. Mock/unit tests do not establish live runtime, desktop, EDR, or platform containment claims. Do not tag or publish without explicit authorization.
