# Contributing to kVNC

Japanese: [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md).

Keep changes small and preserve the loopback-only secure default. Do not add remote input injection, non-loopback listening, or unauthenticated network exposure without a documented threat model and tests.

Before opening a pull request, run:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

Update both language variants when changing user-facing documentation. Follow the [verification guide](docs/verification.en.md) for every protocol, listener, clipboard, or background-operation change.

At minimum, test `--source demo` with debug logging. For RFB handshake changes, verify both RFB `003.008` and `003.003`; for listener changes, verify the exact bound address from a separate trusted host; and for desktop-capture changes, perform a smoke test in an interactive desktop session on every OS you change. Never add an unauthenticated non-loopback example without explaining its private-network boundary.
