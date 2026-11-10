# kVNC への貢献

English: [CONTRIBUTING.md](CONTRIBUTING.md)

変更は小さく保ち、loopback-only の secure default を維持してください。documented threat model と test なしに remote input injection、non-loopback listen、unauthenticated network exposure を追加してはいけません。

Pull Request 前に次を実行します。

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

`Cargo.lock` を変更した場合は、pinned generator で third-party notice を再生成し、
生成差分を Pull Request に含めます。

```sh
cargo install --locked --version 0.9.3 --features cli cargo-about
scripts/generate-third-party-licenses.sh
git diff --check
```

user-facing document を変更するときは、両言語版を更新します。protocol、listener、clipboard、background operation の変更では、必ず [検証ガイド](docs/verification.ja.md) に従ってください。

少なくとも `--source demo` と debug log を使って検証します。RFB handshake を変更した場合は RFB `003.008` と `003.003` の両方を検証し、listener を変更した場合は別の trusted host から指定した bind address を検証します。desktop capture の変更は、変更した各 OS の interactive desktop session で smoke test を実施してください。unauthenticated non-loopback の例は、private-network boundary を説明せずに追加してはいけません。
