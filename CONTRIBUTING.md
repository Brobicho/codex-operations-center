# Contributing

Contributions are welcome through focused pull requests.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo run -- doctor
cargo run -- --graphics unicode
```

Use `CODEX_OPS_HOME` and `CODEX_HOME` when testing installation logic so development does not alter your normal Codex configuration:

```bash
test_root="$(mktemp -d)"
CODEX_HOME="$test_root/codex" \
CODEX_OPS_HOME="$test_root/data" \
cargo run -- integrate
```

## Product principles

- Display only observed or explicitly derived Codex information.
- Never label an invented score as agent performance, progress, or capability.
- Preserve unrelated user configuration during installation and removal.
- Keep keyboard access available for every mouse action.
- Maintain a usable fallback when pixel graphics are unavailable.

