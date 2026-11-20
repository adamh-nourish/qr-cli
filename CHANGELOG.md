# Changelog

All notable changes to `qr-cli` are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Until 1.0.0, minor versions may change the command-line interface.

To release, move the entries under **Unreleased** into a new version
section, set its date, bump `version` in `Cargo.toml`, merge, then push the
matching `vX.Y.Z` tag. The release workflow refuses a tag that has no
section here.

## [Unreleased]

## [0.1.0] - Unreleased

First release (draft: the date is set when `v0.1.0` is tagged).

### Added

- `qr TEXT` writes a QR code to the terminal, as text or ASCII, or to a file
  as PNG, JPEG, WebP, SVG, PDF or EPS. The format comes from the `-o`
  extension or `-f`; files are written atomically.
- Error correction, minimum version, data mode and mask options, with exact
  capacity errors when the data does not fit.
- Styling: module shapes (`square`, `dots`, `rounded`), foreground and
  background colours, transparency, inversion, and contrast warnings.
- `--verify` reads the written code back and fails with exit code 6 if it
  does not decode to the input. Verification runs automatically for styled
  codes and logos.
- Logo overlay (`--logo`), with error correction forced to H and a bounded
  image loader.
- Payload subcommands: `url`, `wifi` (with `--password-stdin`), `vcard`,
  `mecard`, `email`, `sms`, `tel`, `geo` and `event`.
- `qr decode` reads the codes in PNG, JPEG and WebP images, with `--json` and
  `--raw` output.
- `qr batch` writes one code per CSV row or text line into a directory, in
  parallel (`--jobs`), with an NDJSON report (`--json`) and `--fail-fast`.
- `qr completions SHELL` (bash, zsh, fish, PowerShell, elvish) and `qr man`.
- Release archives for Linux (x86_64 and aarch64, static musl), macOS
  (Apple silicon and Intel) and Windows (x64 and Arm64, static CRT), with
  SHA-256 checksums, shell completions and the man page, plus shell and
  PowerShell installers. Binaries are built with `cargo auditable`.

### Security

- No network access; input sizes, image dimensions, batch rows and output
  paths are bounded (see `SECURITY.md`).

[Unreleased]: https://github.com/adamh-nourish/qr-cli/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/adamh-nourish/qr-cli/releases/tag/v0.1.0
