# Security Proposal: qr-cli (Rust)

## 1. Threat model

**Assets:** the user's filesystem (output paths), the user's terminal, downstream consumers of generated artefacts (browsers rendering SVG/HTML, print pipelines), and the release supply chain (binaries users install).

**Trust boundaries / untrusted inputs:**
- Payload data: argv, stdin, `--input-file`, batch files (CSV/JSONL)
- Structured payload fields (WiFi SSID/password, vCard, MeCard, geo, email, SMS)
- Logo/overlay images (PNG/JPEG/WebP/GIF/SVG)
- Config files (`$XDG_CONFIG_HOME/qr/config.toml`, project-local `.qr.toml`) and env vars
- Output path templates in batch mode
- Decode mode, if offered (`qr decode img.png`): untrusted image plus untrusted decoded text

**Adversaries:** a malicious batch or CSV file, a crafted image (decompression bomb or parser bug), a hostile repo containing a `.qr.toml`, a compromised dependency or Action, and a tampered release artefact.

**Out of scope:** a local attacker who already has the user's privileges, and the semantic safety of what the QR encodes, since phishing URLs are the user's responsibility. We could still offer an optional `--warn-on` lint.

## 2. Concrete requirements

### 2.1 Input size limits
- Payload cap: 7,089 numeric, 4,296 alphanumeric or 2,953 bytes (V40-L), enforced *before* encoding. Exceeding it gives a clear error with the actual and max sizes, never a panic.
- stdin and input files: read through `Read::take(limit+1)`, with a default cap of 64 KiB per payload. Batch files default to 50 MiB / 100k rows, overridable with `--max-batch-rows`.
- Render bounds: `--scale`/`--size` capped (for example, at most 16,384 px per side and 256 MP total) to prevent a memory DoS from tiny inputs. Margin, module size and DPI are all range-validated through clap `value_parser!` ranges.
- All size arithmetic uses `checked_mul` and `checked_add`, and `clippy::arithmetic_side_effects` is warned on in render code.

### 2.2 Output path handling
- **No overwrite by default.** Create the file with `OpenOptions::create_new(true)`, which is atomic and avoids the TOCTOU risk of an `exists()` check. `--force` opts in.
- With `--force`, write to a temp file in the same directory (`tempfile::NamedTempFile::new_in`) and then `persist()`. That is an atomic rename that replaces a symlink rather than following it.
- Symlinks: in the `create_new` path, refuse a final component that is a symlink, which is `O_NOFOLLOW` semantics on Unix. Parent symlinks are allowed, because a user-chosen directory is trusted.
- Batch output templates (`{name}.png` from CSV fields): sanitise field values by stripping `/ \ : NUL`, control chars and leading `.`/`-`, rejecting Windows reserved names (CON, NUL, COM1…) and capping length at 200 bytes. Resolve the result and require that it stays under `--out-dir`, with no `..` escape. Never interpret a CSV value as a path.
- `-` means stdout. If stdout is a TTY and the format is binary, refuse unless `--force`.
- File mode is 0644 via the umask. Never chmod +x.

### 2.3 Logo and image decoding
- Use `image` with **only** the needed format features (png, jpeg, webp, gif). Disable everything else, including tiff, exr, hdr, dds and avif-native/C codecs.
- Always decode through `ImageReader::with_guessed_format()` plus `Limits { max_image_width: 4096, max_image_height: 4096, max_alloc: 64 MiB }`. Check file size before reading (at most 10 MiB).
- SVG logos (`resvg`/`usvg`): no external resource loading (`resources_dir = None`, no `href` to file:// or http), a font DB with no system scan unless needed, a cap on node count and nesting depth, and a 5 MiB file-size cap. Rasterise into a bounded pixmap.
- Never embed raw logo SVG into SVG output. Embed it either as a rasterised PNG data URI or as a re-serialised usvg tree.
- Validate logo coverage at 30% or less of the symbol area, otherwise reject (a scannability and safety guard).

### 2.4 SVG, HTML and EPS output injection
- Generate SVG with a typed writer, never string concat of user data. All user-controlled strings, such as `--title`, `--desc`, colours and the optional embedded payload text, are XML-escaped (`& < > " '`). Strip chars that are invalid in XML 1.0.
- Colours are parsed into a `Color` type (hex, rgb() or named) and re-serialised, so arbitrary strings never reach `fill=`.
- No `<script>`, no event attributes, no `<foreignObject>`, no external `href`. Add an output-level test that parses every generated SVG and asserts these are absent.
- For an HTML output format, escape the same way. For EPS/PDF, escape `( ) \` in PostScript strings.

### 2.5 Terminal escape injection
- Any echo of user data (errors, `--verbose`, `decode` output, batch progress) goes through a `sanitize_for_terminal()` that replaces C0/C1 controls, ESC, DEL and bidi overrides (U+202A–U+202E, U+2066–U+2069) with visible `\u{..}` escapes.
- Decoded QR contents are untrusted. Print them raw only with `--raw` and only when stdout is not a TTY.
- Terminal rendering (ANSI/Unicode half-blocks) emits only fixed escape sequences we generate. Honour `NO_COLOR`.

### 2.6 Config trust
- A config file supplies **defaults only**: style, colours, size, format. It can never supply output paths with `--force`, hooks, commands, or plugin or URL loading.
- No auto-loading of a CWD `.qr.toml` (a hostile-repo risk). The user config lives in the XDG/AppData dir only, or is passed explicitly with `--config`.
- On Unix, warn if the config is world-writable. Parse with `serde` plus `deny_unknown_fields` and a size cap of 1 MiB.
- Precedence is CLI over env (`QR_*`) over config over built-in defaults, and `qr config show --resolved` prints the result.
- No network access anywhere in the binary, and none in CI (verified with `cargo deny` bans on `reqwest`, `hyper` and similar).

### 2.7 Unsafe policy
- Put `#![forbid(unsafe_code)]` in every crate in the workspace.
- Use `cargo geiger` as an informational report on dependencies.
- Prefer pure-Rust crates (no C image codecs, no OpenSSL).
- Panics are treated as bugs. Set `panic = "abort"` in release and use `clippy::unwrap_used` and `expect_used` = deny outside tests.

### 2.8 Fuzzing (`cargo-fuzz`, nightly job plus a 60 s smoke run on each PR)
1. `fuzz_encode`: arbitrary bytes, ECC level and version into the encoder. Assert no panic and that the round-trip decodes (via `rqrr`) equal the input.
2. `fuzz_payload_builders`: arbitrary structured WiFi/vCard/geo fields. Assert spec-valid escaping (for example, WiFi `\;,:"`).
3. `fuzz_svg_writer`: arbitrary title and colour strings, with output parsed by `roxmltree`. Assert well-formed XML with no script or event attrs.
4. `fuzz_logo_decode`: arbitrary bytes into the bounded image loader. Assert no panic or OOM above the limit.
5. `fuzz_config_parse`: arbitrary TOML.
6. `fuzz_batch_path_sanitize`: assert the result stays within the out-dir and is never a reserved name.
7. `fuzz_decode` (if decode ships).

Property tests (`proptest`) mirror items 1, 3 and 6 in the normal test suite, so they count toward coverage.

## 3. Supply chain

- **Commit `Cargo.lock`**, use `--locked` everywhere, and pin `rust-toolchain.toml` to a specific stable.
- **cargo-deny** (`deny.toml`):
  - advisories set to deny
  - licenses on an allowlist (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0)
  - sources limited to crates.io only, with no git deps
  - bans for duplicate major versions (warn) and the network crates listed above
- **cargo-audit**: runs on PRs and on a daily schedule, so new RUSTSEC advisories open issues.
- **cargo-vet** or `cargo-crev`: optional, but we recommend importing the Mozilla, Google and Bytecode Alliance audits.
- **GitHub Actions:**
  - every `uses:` pinned to a full 40-char commit SHA with a `# vX.Y.Z` comment
  - top-level `permissions: {}` (or `contents: read`), with per-job elevation only for release (`contents: write`, `id-token: write`, `attestations: write`)
  - `persist-credentials: false` on checkout
  - no `pull_request_target`, and no untrusted `${{ github.event.* }}` interpolated into `run:` (pass it via env)
  - `step-security/harden-runner` in audit mode
  - `zizmor` and `actionlint` to lint workflows
- **Dependabot:** weekly for `cargo` and `github-actions` (keeping the SHA pins fresh), with grouped minor/patch updates.
- **Releases:**
  - built by `cargo-dist`, or a custom matrix covering linux-gnu/musl x86_64+aarch64, macOS x86_64+aarch64 and windows-msvc x86_64+aarch64
  - reproducible flags (`--locked`, `-C strip`, `SOURCE_DATE_EPOCH`)
  - `SHA256SUMS` published
  - **SLSA build provenance** via `actions/attest-build-provenance`, verifiable with `gh attestation verify`
  - **keyless Sigstore/cosign** signatures for checksums
  - **CycloneDX SBOM** (`cargo-cyclonedx`) attached and attested
  - macOS notarisation and Windows Authenticode are follow-ups that depend on certs, and the delivery engineer owns them
- **Repo settings:** branch protection on `main` (required checks, linear history, no force-push), secret scanning with push protection, private vulnerability reporting enabled, and a `SECURITY.md`. Tag protection on `v*`.

## 4. Blocking CI checks

| Check | Command |
|---|---|
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` (plus pedantic subset, `unwrap_used`/`expect_used`/`panic` deny in non-test) |
| Unsafe | `#![forbid(unsafe_code)]` presence enforced by clippy/grep check |
| Tests | `cargo nextest run --locked` on ubuntu, macos, windows |
| Coverage | `cargo llvm-cov --fail-under-lines 90` (security-critical modules: path sanitiser, SVG writer, limits ≥95%) |
| Supply chain | `cargo deny check` (advisories, bans, licenses, sources) |
| Audit | `cargo audit --deny warnings` |
| Workflow lint | `actionlint` + `zizmor --min-severity medium` |
| Pinning | check all `uses:` are SHA-pinned (zizmor `unpinned-uses`) |
| Fuzz smoke | each fuzz target runs 60 s without crash (PR); nightly 30 min non-blocking but auto-files issue |
| MSRV | `cargo +<msrv> check --locked` |
| Docs | `cargo doc --no-deps` with `-D warnings` (rustdoc broken links) |
| Secrets | GitHub push protection (repo setting) |

Non-blocking but reported: `cargo geiger`, `cargo udeps`, `cargo semver-checks` (once v1.0), and the binary-size delta.

## 5. Security acceptance tests (must exist before v0.1)
- An existing output without `--force` fails with no change, and its contents are untouched.
- When the output is a symlink to `/tmp/victim`, `--force` replaces the link and the victim is unchanged.
- Batch row `name=../../etc/x` writes inside the out-dir or fails.
- A 1x1 PNG header that claims 100k×100k is rejected before allocation.
- An SVG logo with `<image href="file:///etc/passwd">` is not loaded.
- `--title '"><script>'` produces escaped SVG that parses with no script element.
- A payload containing `\x1b]0;pwned\x07` shows up escaped in error output.
- A CWD `.qr.toml` is ignored.
