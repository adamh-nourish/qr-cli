# qr-cli

A fast, secure, portable QR code generator CLI (`qr`). Work in progress; see `docs/design/DECISIONS.md`.

## Install

Prebuilt binaries for Linux (x86_64 and aarch64, static musl), macOS (Apple
silicon and Intel) and Windows (x64 and Arm64) are attached to each
[GitHub Release](https://github.com/adamh-nourish/qr-cli/releases), with a
SHA-256 checksum per archive. Each archive also holds shell completions
(`completions/`) and the man page (`man/qr.1`).

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/adamh-nourish/qr-cli/releases/latest/download/qr-cli-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/adamh-nourish/qr-cli/releases/latest/download/qr-cli-installer.ps1 | iex"
```

The installers verify the archive's checksum and put `qr` in
`~/.cargo/bin` (set `QR_CLI_INSTALL_DIR` to choose another prefix, and
`QR_CLI_NO_MODIFY_PATH=1` to leave your shell profile alone).

From source, with Rust 1.85 or later:

```sh
cargo install --locked --git https://github.com/adamh-nourish/qr-cli --tag v0.1.0 qr-cli
```

The binaries are not code-signed yet, so a browser download may get a
Gatekeeper (macOS) or SmartScreen (Windows) warning; the installers and
`cargo install` are not affected. Release binaries are built with
[`cargo auditable`](https://github.com/rust-secure-code/cargo-auditable), so
`cargo audit bin qr` can check them for known vulnerabilities.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the local quality gates and [SECURITY.md](SECURITY.md) for reporting vulnerabilities.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

JPEG output uses the `jpeg-encoder` crate, which is based in part on the work of the Independent JPEG Group.
