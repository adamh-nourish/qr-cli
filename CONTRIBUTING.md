# Contributing to qr-cli

The design is recorded in [`docs/design/DECISIONS.md`](docs/design/DECISIONS.md).
Read the section relevant to your change before starting.

## Layout

| Path | Purpose |
|---|---|
| `crates/qr-core` | Pure library: matrix, encoding, payloads, renderers, limits. No I/O. |
| `crates/qr-cli` | The `qr` binary: argument parsing, files, streams, exit codes. |

## Toolchain

`rust-toolchain.toml` pins the development toolchain (stable 1.99) for rustup
users. The minimum supported Rust version is **1.85** (`rust-version` in
`Cargo.toml`); do not use language or library features newer than that.
Cargo's MSRV-aware resolver keeps `Cargo.lock` on 1.85-compatible versions;
commit `Cargo.lock` with every dependency change.

Extra tools used by the gates:

```sh
cargo install --locked cargo-nextest cargo-llvm-cov cargo-deny
# optional: hyperfine (startup gate), actionlint and zizmor (workflow lint)
```

## Local gates

CI runs all of these on every pull request and they must pass:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
cargo test --doc --workspace --locked
cargo llvm-cov nextest --workspace --locked --fail-under-lines 90
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --workspace --locked
cargo deny check
actionlint && zizmor --min-severity medium .   # when .github/ changes
```

With rustup, also check the MSRV: `cargo +1.85 check --workspace --all-targets --locked`.

### Coverage with Homebrew Rust (no rustup)

Homebrew's `rust` formula does not ship `llvm-tools`, so `cargo llvm-cov`
cannot find `llvm-cov`/`llvm-profdata` on its own. Point it at Homebrew's LLVM
(`brew install llvm`); the LLVM major version must be at least the one rustc
was built with (`rustc -vV`):

```sh
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
cargo llvm-cov nextest --workspace --locked --summary-only
```

## Code rules

- No `unsafe` (forbidden), and no `unwrap`, `expect` or `panic!` outside tests.
  Return typed errors (`thiserror`) and map them to the documented exit codes.
- Document every public item. Keep modules small and cohesive.
- Every change brings its own unit and integration tests; the workspace line
  coverage gate is 90%.
- Size arithmetic uses checked operations.

## Pull requests

- Titles follow [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat(cli): ...`, `fix(core): ...`, `ci: ...`); CI enforces this because
  PRs are squash-merged and the title becomes the commit message.
- Keep each PR under about 400 changed lines where possible.
- New GitHub Actions must be pinned to a full commit SHA with a version comment.

## Releases

Releases are built by [dist](https://opensource.axo.dev/cargo-dist/)
(`dist-workspace.toml`) in `.github/workflows/dist-build.yml`. To check
packaging locally (`brew install cargo-dist cargo-auditable`):

```sh
dist plan
cargo build --locked -p qr-cli --bin qr && scripts/dist-extras.sh target/debug/qr
dist build --artifacts=all --target "$(rustc -vV | sed -n 's/^host: //p')"
scripts/verify-archive.sh target/distrib "$(rustc -vV | sed -n 's/^host: //p')"
```

To release: move the **Unreleased** entries in `CHANGELOG.md` into a dated
version section, bump `version` in `Cargo.toml`, merge, then push the tag
`vX.Y.Z`. `release.yml` builds and smoke tests every target before it
creates the GitHub Release.

## Licence

By contributing you agree that your contributions are dual-licensed under
MIT OR Apache-2.0, as described in `LICENSE-MIT` and `LICENSE-APACHE`.
