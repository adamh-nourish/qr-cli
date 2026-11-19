# Security policy

## Supported versions

`qr` is pre-1.0. Only the latest release (and `main`) receives security fixes.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/adamh-nourish/qr-cli/security/advisories/new)
("Report a vulnerability" on the Security tab). Do not open a public issue.

Include the `qr --version` output, your platform, the exact command line and
any input files needed to reproduce the problem. We aim to acknowledge reports
within 7 days and to agree a disclosure date with you once a fix is ready.

## Scope

In scope:

- Crashes, panics, hangs or unbounded memory use caused by input text, files,
  images or command-line arguments.
- Output that is unsafe to open: script or external references in SVG, escape
  sequences that reach a terminal unescaped, or files written outside the
  requested location (for example, through batch name templates).
- Any network access by the binary. `qr` is designed to work fully offline.
- Supply-chain issues in the build or release pipeline.

Out of scope:

- The content a user chooses to encode (for example, a phishing URL).
- Attacks that require an attacker who already runs code as the same user.

## Hardening in place

- `#![forbid(unsafe_code)]` in every crate; `unwrap`, `expect` and `panic!`
  are denied by Clippy outside tests, and release builds use `panic = "abort"`.
- `cargo deny` checks RustSec advisories, licences, banned crates (network and
  OpenSSL stacks) and sources (crates.io only) on every pull request and daily.
- All GitHub Actions are pinned to full commit SHAs, workflows run with
  read-only tokens, and workflows are linted with `actionlint` and `zizmor`.
