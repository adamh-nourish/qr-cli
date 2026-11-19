# Final Decision Record: `qr-cli` (devil's advocate)

## 0. Problems found across the proposals

| # | Conflict or claim | Ruling |
|---|---|---|
| 1 | **Overwrite default.** UX overwrites unless `--no-clobber`; Security and DA refuse unless `--force`. | **Single `-o` overwrites, atomically (temp file plus rename).** The user typed the path, and that is how `cp`, `qrencode` and ImageMagick behave. `--no-clobber` is available. **Batch never overwrites** without `--force`, because batch is where the path risk actually is. |
| 2 | **resvg.** Delivery wants it; Performance and DA reject it. | **No resvg in v1.** One module grid feeds our own raster renderer. SVG logos are deferred, so the usvg attack surface is gone. |
| 3 | **"7,089-byte cap."** UX's figure is wrong: 7,089 is the *numeric digit* limit. The byte limit is 2,953 (V40-L). | Read at most 7,090 bytes, then report the exact capacity per mode and EC level from the encoder. Security's 64 KiB stdin cap is dropped as redundant. |
| 4 | **Logo size.** Width 0.3 (UX), 30% of area (Security) and 20% of area (DA) are three different limits. | **`--logo-scale` is a fraction of the width, max 0.3** (about 9% of the area). EC is forced to H and `--verify` runs automatically. |
| 5 | **Config auto-load** (UX) vs "no config scan at startup" (Performance) vs "defer" (DA). | **Defer the config file and the `QR_*` env vars to v1.x.** This removes a precedence matrix, a fuzz target and a trust question. `NO_COLOR` and `CLICOLOR_FORCE` are honoured. |
| 6 | **Performance hard wall-time budgets** (8 ms, 12 ms, and so on) as CI failures. Performance itself admits runners vary by 10–30%. | Only deterministic metrics are gated (§5). The numbers are unmeasured targets, as Performance says. |
| 7 | **Fuzz 60 s × 7 targets on every PR** (Security). That needs nightly Rust and about 7 CI-minutes per OS. | **Fuzzing runs nightly and does not block. proptest runs on every PR and blocks.** |
| 8 | **Duplicate tooling:** cargo-audit plus cargo-deny advisories; harden-runner; cargo-vet; cosign plus SLSA plus SBOM. | Use **cargo-deny only** (it already reads RustSec). Drop harden-runner (a third-party action that only reports in audit mode) and cargo-vet. Attestations wait until the repo is public, because a private repo needs GHEC for them. |
| 9 | **release-plz** needs a GitHub App or a PAT secret that the user has to create. | **Defer release-plz.** In v1, a manual `v*` tag triggers cargo-dist. PR titles follow conventional commits so release-plz can be added later. |
| 10 | **Coverage per module ≥95%** (Security). `cargo llvm-cov` has no per-module gate. | A single **90% line gate** across the workspace. Integration tests run the real binary through `assert_cmd`, so `main.rs` does not need excluding. |
| 11 | **Cargo feature flags per codec** (Performance). Every flag multiplies the build matrix. | **No optional features in v1.** All dependencies are pure Rust, and the binary-size gate is what keeps bloat out. |
| 12 | **JPEG output.** DA says cut it (lossy); UX keeps it. | **Keep it** at quality 92 with a minimum scale of 4. The user asked for comprehensive output, the artefacts are negligible at that scale, and `--verify` covers the rest. |
| 13 | **`--mask`.** UX assumes the encoder supports it; that has not been checked against `qrcode`. | It ships only if PR 2's bake-off confirms the encoder supports it. Otherwise it is deferred. **PR 2: confirmed** for `fast_qr` (forced masks are honoured and the chosen mask is reported), so `--mask 0..7` ships in PR 3. |

## 1. Language and crates

**Rust**, edition 2024. `rust-toolchain.toml` pins the stable toolchain installed locally (1.99). MSRV is `rust-version = "1.85"`.

**Workspace layout:**
- `crates/qr-core`: pure library, no I/O. Contains the matrix, encoding, payloads, renderers and limits.
- `crates/qr-cli`: binary `qr`. Contains clap, I/O and exit codes.

**Crates:**
- **CLI:** `clap` 4.5 (derive), `clap_complete` 4.5, `clap_mangen` 0.2, `anstream`/`anstyle`, `terminal_size` 0.4.
- **Encoding and decoding:** `fast_qr` 0.14 (`default-features = false`) is the production encoder, behind a `QrEncoder` trait. `qrcode` 0.14 is a **dev-dependency only**: the differential oracle, the capacity-table cross-check and the `qrcode_reference` bench group. Decoding uses `rqrr` 0.9.
  - **Original rule:** swap to `fast_qr` only if it is more than 3× faster at v20+ **and** passes the same round-trip vectors. PR 2 met both: automatic-mask encode at EC M is about 12× faster (`benches/encode.rs`, M4 Pro: v5 47 µs vs 439 µs, v20 294 µs vs 3.72 ms, v40 1.01 ms vs 12.9 ms), all output round-trips through `rqrr`, and with a forced mask the matrices are bit-identical to `qrcode`'s.
  - **The rule did not cover symbol size or mask choice. The swap trades both for encode speed:**
    - **No mixed-mode segmentation.** `plan()` picks one mode for the whole input, while `qrcode` segments optimally. At EC M (`tests/differential.rs` pins these): `tel:+441234567890` v2 vs v1; `SMSTO:+447700900123:Hello there` v3 vs v2; a 43-byte URL ending in a 16-digit id v4 vs v3; `order-` + 200 digits v10 vs v6; `ref:` + 1,000 digits v26 vs v16. WiFi, `geo:`, an upper-case URL and a short vCard came out the same. The capacity ceiling drops for mixed data as well: `ref:` + 5,000 digits is `DataTooLong` at L, but fits `qrcode` at v40.
    - **Automatic mask is not the ISO minimum.** `fast_qr` scores masks before drawing format information and resets its run (N1) penalty at function modules. On 400 seeded random byte inputs (0–600 bytes, EC M, same version and mode) its matrix matched `qrcode`'s in 273/400 (68%); review measured 278/400 and, under an independent ISO penalty, `fast_qr` chose the minimum in 197/300 vs `qrcode`'s 295/300. Every mask decodes, so this is a small scan-robustness cost, not a correctness bug.
  - **Is the speed needed?** One-shot encode is under 1 ms with either crate at typical sizes, and `qrcode` meets the §5 batch target up to about v10. `fast_qr` only changes the outcome for v20+ batches, while segmentation affects every tel/sms/URL-with-id code.
  - **Status: owner sign-off on this trade is pending on PR 2.** If the owner prefers `qrcode`, the swap back is a second ~40-line backend behind the same trait, plus segment-aware sizing in `plan()`/`capacity`. Adding segmentation in front of `fast_qr` is the other way to close the size gap.
- **Raster:** `png` 0.17 (indexed or 1-bit output) and `jpeg-encoder` 0.6. `image` 0.25 with `default-features=false` and only `png`, `jpeg` and `webp` is used to *read* logos and decode inputs and to encode lossless WebP.
  - **PR 6:** `png` is 0.18, the version `image` 0.25 already depends on, so there is one copy. `jpeg-encoder` stays on 0.6 because 0.7 needs Rust 1.87 (above the 1.85 MSRV); its licence adds IJG, allowed in `deny.toml` for that crate only and credited in the README. In this PR `image` enables only `webp`; PR 9 adds the `png` and `jpeg` decoders for the logo loader. PNG uses zlib's default level with unfiltered 1-bit rows, not `Fast`: a 290 px code is 424 bytes instead of 1,574 for about 0.1 ms more work. The release binary grew by 132 KB (932,144 to 1,064,528 bytes, aarch64-apple-darwin).
- **Batch, files and parsing:** `rayon` 1.10 (batch path only), `csv` 1.3, `tempfile` 3, `serde`/`serde_json` 1, `thiserror` 2.
- **Dev:** `proptest` 1, `assert_cmd` 2, `predicates` 3, `insta` 1, `roxmltree` 0.20, `divan` (pick one; no criterion).

## 2. Binary and v1 CLI surface

The binary is `qr` and the crate is `qr-cli`. The name may collide with the `qr` script installed by Python's `qrcode` package; the README documents this.

```
qr [OPTIONS] [TEXT]                      # generate (default); `-` = stdin; `qr -- wifi` for literal subcommand names
  -i, --input <FILE>      -o, --output <PATH|->     -f, --format <terminal|text|ascii|png|svg|jpeg|webp|pdf|eps>
  --ec <L|M|Q|H>  --qr-version <1..40>  --mode <auto|numeric|alnum|byte>  [--mask <0..7> if encoder supports]
  --margin <N=4>  --scale <N=10> | --size <PX>   --dpi <N>
  --fg <COLOR> --bg <COLOR> --transparent --invert --shape <square|rounded|dots>
  --logo <PNG|JPEG|WEBP> --logo-scale <0.05..0.3>
  --verify / --no-verify  --data-uri  --no-clobber  --keep-newline
  --json  -q/--quiet  -v/--verbose  --color <auto|always|never>
qr url|wifi|vcard|mecard|email|sms|tel|geo|event <ARGS> [render OPTIONS] [--print-payload]
   (wifi: --ssid --password|--password-stdin --auth wpa|wep|nopass --hidden)
qr decode <IMAGE>... [--json] [--raw]
qr batch <FILE> --out-dir <DIR> [--lines] [--name <TEMPLATE>] [-f FMT] [--jobs N] [--fail-fast] [--force] [--json]
qr completions <bash|zsh|fish|powershell|elvish>
qr man
```

**Output resolution** follows UX: `-f` first, then the `-o` extension, then terminal output on a TTY, otherwise UTF-8 text. Binary output to a TTY is refused.

**Exit codes:**

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Internal error |
| 2 | Usage error |
| 3 | Data too long |
| 4 | I/O error |
| 5 | No code found (decode) |
| 6 | Verification failed |

**Errors** follow UX's style: lowercase, include the actual values, give one hint, and never print a stack trace.

## 3. v1 features vs deferred

**v1:**
- **Input:** argument, stdin and file input, with exact capacity errors.
- **Outputs:** terminal, text, ascii, PNG, SVG, JPEG, WebP (lossless), PDF and EPS. PDF and EPS are written by hand as vector output.
- **Output handling:** `--data-uri`, `--json`, completions and the man page.
- **Encoding options:** EC level, version, mode, margin, scale, size and DPI.
- **Styling:** colours (hex and CSS names), transparent background, invert, and three module shapes. Warnings for contrast below 3:1 and for an inverted foreground.
- **Logo:** PNG, JPEG and WebP logo images. Raster outputs and SVG get the logo, embedded as a PNG data URI. A logo on PDF or EPS is a usage error in v1.
- **`--verify`:** decodes the rendered raster with rqrr. It runs automatically for shapes, logos and non-default colours.
- **Payload helpers:** all nine.
- **Subcommands:** `decode` and `batch`.

**Deferred to v1.x:**
- **Configuration:** config file, `QR_*` env vars, `qr config`.
- **Output and terminal:** clipboard, Sixel and Kitty images, logos on PDF and EPS, SVG logos.
- **Styling:** presets, frames and captions, gradients, custom eyes.
- **Encoding and payloads:** `--split` (structured append), Micro QR and rMQR, EPC/SEPA, crypto URIs, `--optimize` (oxipng).
- **Release and distribution:** release-plz, crates.io, Homebrew, Scoop, winget, GHCR, attestations, SBOM, code signing.

**Cut:**
- Fetching inputs or logos by URL.
- Server, GUI or webcam modes.
- Dynamic or short-link QR codes.
- An npm wrapper.

## 4. Mandatory security requirements

1. `#![forbid(unsafe_code)]` in both crates. In `[workspace.lints]`, `unwrap_used`, `expect_used` and `panic` are denied outside tests. Release builds use `panic="abort"`.
2. **Input and render limits:**
   - Input is read through `take()`.
   - `--scale`, `--size` and `--dpi` are range-checked by clap value parsers.
   - Renders are capped at 16,384 px per side and 256 MP in total.
   - All size arithmetic uses checked operations.
3. **Image loading:** every image goes through one bounded loader:
   - the file is at most 10 MiB;
   - `image::Limits` sets a maximum of 8,192×8,192 and 128 MiB allocated;
   - the header is checked before decoding.
4. **SVG output:** generated by a typed writer.
   - Colours are parsed into a `Color` type and written back out, never passed through as strings.
   - All text is XML-escaped and XML-invalid characters are stripped.
   - No script, event attributes, foreignObject or external href.
   - A `roxmltree` test asserts all of this on every generated SVG.
   - PDF and EPS strings are escaped.
5. **Terminal escapes:** `sanitize_for_terminal()` is applied to every echo of user or decoded data. It escapes C0/C1 controls, ESC, DEL and bidi overrides. Raw decoded output is printed only with `--raw` when stdout is not a TTY.
6. **Output files:** written atomically with `NamedTempFile::new_in(dir)` and `persist`. This replaces a symlink rather than following it.
7. **Batch file names:**
   - Template fields are sanitised: separators, `:`, NUL, control characters and leading `.`/`-` are stripped, Windows reserved names are rejected, and names are capped at 200 bytes.
   - The final path must stay under the canonicalised `--out-dir`.
   - Existing files are refused without `--force`.
   - Defaults are 100k rows and 50 MiB.
8. **WiFi passwords:** `--password-stdin` keeps secrets out of shell history. Payload escaping for WiFi (`\ ; , : "`) and vCard/MeCard is covered by proptest.
9. **No network access.** cargo-deny bans `reqwest`, `hyper`, `ureq`, `openssl-sys` and `native-tls`. Only crates.io sources are allowed, and licences must be on the allowlist.
10. **Security acceptance tests** from Security §5, adjusted for the overwrite ruling and with no config test. These must pass before v0.1.0.

## 5. Performance: gated vs tracked

**Gated (blocking):**
- **Binary size:** the stripped `x86_64-unknown-linux-musl` binary must be at most 6 MB. The target is 4 MB.
- **Startup:** median of `qr --version` at most 20 ms (hyperfine, 30 runs) on ubuntu. This is a deliberately loose smoke gate.
- **One-shot path:** must not start rayon. A test asserts this through a counter.

**Tracked (nightly artefact, plus an issue if a metric drifts more than 20% from baseline):**
- Startup target of 3 ms.
- Per-format single-render times.
- Batch throughput: 10k × 256 px PNG at ≥2.5k codes/s on a 4-core runner.
- Peak RSS: ≤20 MB for a single code, and flat for a 100k-row batch.
- dhat allocation counts.
- Duplicate dependencies (`cargo tree -d`).

**Not done in v1:** iai-callgrind, PGO, mimalloc and gh-pages trend charts. Each comes back only if tracked data shows a problem.

## 6. Distribution in v1

- **cargo-dist** builds GitHub Release archives (`.tar.xz`/`.zip`) with SHA256 checksums and the shell and PowerShell installers. Binaries are built with `cargo auditable`.
- **Targets:** x86_64 and aarch64 linux-musl, aarch64 and x86_64 apple-darwin (x86_64 cross-built), and x86_64 and aarch64 windows-msvc with a static CRT.
- **Archives** include completions and the man page.
- **`cargo install --git`** is documented.
- **On each target,** a smoke test checks `--version`, generates a PNG and an SVG, decodes the PNG, and compares the result with the input.
- The full matrix runs only on tags and nightly. PRs run `dist plan`.
- Everything else waits for a "go public" checklist PR.

## 7. CI checks (`ci.yml`, all actions SHA-pinned, `permissions: contents: read`, `persist-credentials: false`)

**Blocking on every PR:**

| Check | Command or tool |
|---|---|
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings`, with lint config in `[workspace.lints]` (pedantic subset) |
| Tests | `cargo nextest run --locked` on ubuntu, macos and windows |
| Coverage | `cargo llvm-cov nextest --workspace --fail-under-lines 90` (ubuntu only) |
| Supply chain | `cargo deny check` (advisories, bans, licences, sources); also runs on a daily schedule |
| MSRV | `cargo +1.85 check --locked` |
| Docs | `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps` |
| Workflow lint | `actionlint` and `zizmor` (medium severity and above) |
| PR titles | `amannn/action-semantic-pull-request` (conventional commits) |
| Size and startup | The gates in §5 |
| Packaging | `dist plan` (from PR 12) |

**Nightly (does not block):** cargo-fuzz targets (encode round-trip, payload builders, SVG writer, image loader, path sanitiser, decode) at 10 minutes each, the full target matrix with smoke tests, and the benchmarks.

**Also:** Dependabot runs weekly for cargo and actions, with grouped updates. CI uses `Swatinem/rust-cache`.

## 8. PR stack

Engineer **A** owns core and rendering. Engineer **B** owns CLI, I/O and infrastructure. Each PR adds its own tests, so the 90% coverage gate holds from PR 1.

| # | Title | Scope | Owner | Depends on |
|---|---|---|---|---|
| 1 | `ci: scaffold workspace and CI gates` | Workspace, toolchain, lints, LICENSE (MIT OR Apache-2.0), SECURITY.md, deny.toml, ci.yml (all §7 checks except `dist plan`), Dependabot, a stub `qr --version`, release profile (`lto="fat"`, `codegen-units=1`, `strip`, `panic="abort"`) | B | none |
| 2 | `feat(core): encoder trait, matrix, capacity errors` | `QrEncoder` over fast_qr (qrcode as dev-only oracle, §1); EC, version and mode; exact capacity errors; proptest round-trip through rqrr; divan skeleton. The PR description records the fast_qr bake-off data and the `--mask` finding. | A | 1 |
| 3 | `feat(cli): default generate command and text renderers` | Input resolution, newline handling, terminal, text and ascii renderers, error type and exit codes, `sanitize_for_terminal`, `--color`, `completions`, `man` | B | 2 |
| 4 | `feat(core): vector renderers (svg, pdf, eps) and Color` | Typed SVG writer, PDF and EPS emitters, Color parsing, the XML safety test | A | 2 (built in parallel with 3) |
| 5 | `feat(cli): file output pipeline` | `-o`/`-f` inference, atomic write, `--no-clobber`, stdout and TTY rules, `--json`, `--data-uri`; wires in PR 4 | B | 3, 4 |
| 6 | `feat(core): raster renderers (png, jpeg, webp)` | Indexed PNG, `--scale`/`--size`/`--dpi`, render bounds, per-format bench groups | A | 5 |
| 7 | `feat: styling and --verify` | Shapes, fg/bg/transparent/invert, contrast warnings, rqrr verification with auto-verify, exit code 6 | A | 6 |
| 8 | `feat(payload): structured payload subcommands` | Nine builders, `--print-payload`, `--password-stdin`; adds `fuzz/` and the nightly fuzz workflow | B | 5 |
| 9 | `feat: logo overlay and bounded image loader` | Shared loader with limits, logo compositing, EC forced to H, scale cap, auto-verify | A | 7 |
| 10 | `feat: decode subcommand` | Reuses the PR 9 loader; `--json`, `--raw`, exit code 5; decompression-bomb tests | B | 8, 9 |
| 11 | `feat: batch subcommand` | CSV and line input, template sanitiser, rayon pipeline in 1,024-item chunks with ordered output, NDJSON, `--fail-fast`; path-traversal acceptance tests | B | 10 |
| 12 | `build: release pipeline and README` | cargo-dist config and `release.yml`, smoke-test matrix, nightly bench job, README from the documentation writer (install, examples, exit codes, non-goals, the `qr` name clash) | A (README: docs writer) | 11 |

**How the stack is merged:**
- Squash-merge from the bottom up.
- After each merge, `gh pr edit --base main` the next PR and rebase it.
- PR 4 and PR 8 can be reviewed in parallel. Their branches are still stacked, so CI runs on a known base.

## 9. Open risks

1. **Required checks on a private repo.** If the repo is private on GitHub Free, required status checks and branch protection are unavailable, so CI is enforced only by convention. Anonymous installers also return 404. The user must decide public vs private. Creating a public repo publishes code, so it needs their explicit yes.
2. **Verify false negatives.** rqrr is weaker than phone scanners, particularly on `dots` and logo codes. Auto-verify could therefore reject codes that scan fine. Before PR 7 merges, measure the rqrr failure rate on a corpus of 200 styled codes. If it is above 2%, auto-verify warns instead of failing.
   - **PR 7: measured at 0.0%, so auto-verify fails (exit 6) like `--verify`.** `qr-core/tests/verify_corpus.rs` draws 200 styled codes (80 dots, 80 rounded, 40 square in colour; 8 colour pairs of at least 3:1 including two light-on-dark; PNG, JPEG, WebP and redrawn vector output; 4–10 px per module; margins 4 and 2; every seventh transparent) and rqrr reads all 200. A harsher corpus (1–3 px per module, margins 0–1, contrast down to 3.03:1) also reads 200/200. Both are pinned at ≤2% by the test, so the decision cannot silently go stale. The policy is one constant (`verify::AUTO_VERIFY` in `qr-cli`); the warn path is implemented and tested.
   - Measuring found two hazards that the verifier now handles: rqrr 0.9 trips an internal assertion (`scan >= 1`) on 1-px modules, which would abort a release build, so images under 3 px per module are enlarged before reading; and a JPEG flattens a transparent background onto white, so it is read on white, not on the intended background.
   - Verification rules: raster output is decoded from its own bytes (up to 2,048² px; larger images are redrawn at the largest scale that fits); vector and text output is redrawn by the raster renderer at 10 px per module; transparency is read on the `--bg` colour; a code that yields nothing is retried with luma inverted (light-on-dark); a quiet zone under 4 modules is widened in the background colour, because `--margin` below 4 has its own warning. Shapes keep the three finder patterns square (custom eyes are deferred).
3. **fast_qr maintenance.** The production encoder has a single maintainer, and its mask scoring has already needed a fix: a comment in `placement.rs` records that columns were scored on a transpose taken before masking, so vertical penalties never influenced the chosen mask. Its scoring still differs from ISO (see §1). Mitigations: the `QrEncoder` trait keeps a swap to `qrcode` small; `tests/differential.rs` checks forced-mask matrices bit for bit against `qrcode` and pins the auto-mask agreement and segmentation gap; every encode round-trips through `rqrr` in tests. `--mask` is supported (forced masks are honoured and reported).
4. **Crate name.** `qr-cli` on crates.io may be taken. Check before going public and reserve a fallback name.
5. **Subcommand ambiguity.** Text such as `email` is parsed as a subcommand. It is documented, and `--` escapes it, but users can still be surprised.
6. **Stack rebase churn.** 12 stacked PRs with squash merges mean repeated rebases. Keep each PR under about 400 changed lines.
7. **Runners.** `windows-11-arm` and `ubuntu-24.04-arm` availability or cost on the account is unverified. Fallback: cross-compile with no smoke test for those targets.
8. **Unsigned binaries.** Gatekeeper and SmartScreen will warn on browser downloads. Document this until signing certificates exist.
9. **Unvalidated budgets.** Size and startup budgets are not yet measured. If the 6 MB gate fails, the first lever is replacing `image` with direct use of the `png`, `jpeg-decoder` and `image-webp` crates, before falling back to feature flags.
