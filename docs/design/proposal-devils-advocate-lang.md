# Devil's Advocate: Language and Scope Challenge

## 1. Language comparison (for this tool specifically)

| Criterion | Rust | Go | TS (Bun/Deno compile) | Python |
|---|---|---|---|---|
| Cold start | ~1–3 ms | ~2–5 ms | ~20–60 ms | ~40–80 ms interpreter, 150–400 ms PyInstaller onefile (unpacks to temp) |
| Binary size | ~2–6 MB (LTO, strip, `panic=abort`) | ~5–10 MB | Bun ~55–100 MB, Deno ~70–100 MB | 10–40 MB frozen |
| Cross-compile | Good with `cargo-dist`, `cross` or `cargo-zigbuild`. Native macOS and Windows runners are easier still | Best in class (`GOOS/GOARCH`, no cgo) | Bun/Deno `--target` works; one runtime per target | Poor: PyInstaller needs one build per OS |
| QR encode | `qrcode` (kennytm; Micro QR, mature, slow-moving), `fast_qr` (fast; no Micro QR), `qrcodegen` (Nayuki reference port) | `skip2/go-qrcode` (effectively unmaintained), `yeqown/go-qrcode` | `qrcode` (soldair; the most-downloaded, very mature) | `segno` (the most standards-complete library in any language: Micro QR, structured append, many writers) |
| QR decode | `rqrr` (pure Rust, QR only), `rxing` (ZXing port) | `gozxing` | `jsQR` (stale), `@zxing/library` (maintenance mode) | `pyzbar`/`zxing-cpp` (native libs, which hurts portability) |
| PNG/SVG/PDF/terminal | `png`/`image`; SVG is string output; PDF via `svg2pdf`/`printpdf` or ~100 LOC of hand-written vector PDF; terminal half-blocks trivial | Similar, fewer PDF options | Good; PDF via `pdfkit` | Excellent (segno does most natively) |
| Security | Memory-safe; `cargo-audit`/`cargo-deny`; small dependency tree if disciplined | Memory-safe; `govulncheck` | Large npm transitive tree, supply-chain exposure | Packaging and native deps are the weak spot |
| Contributor reach | Smallest pool; slow compiles (clean build with `image` ~30–60 s) | Easy | Largest pool | Large pool |
| Local toolchain | Installed | **Not installed** | Node 20 only (Bun/Deno absent) | 3.15 (pre-release/new; risky) |

**Evidence-based challenges to the Rust default:**
- **Python + segno has the best QR feature coverage**, but startup time and frozen-binary portability rule it out for a CLI that is meant to install anywhere.
- **Go is the honest rival.** It has the easiest cross-compilation and contributor accessibility, but its encoder ecosystem is weaker and the toolchain is not installed. Installing it adds friction for every agent, which costs more than any marginal benefit.
- **TS compile produces binaries 15–30x larger than Rust** with 10–20x slower startup, and a heavier supply-chain surface. That fails the performance and security goals.
- **Rust's real risks** are slow compiles (hurts DX and CI time), crate maintenance cadence (`qrcode` releases are infrequent), and dependency bloat if `image`/`resvg` are pulled in by default.

**Mitigations required if we pick Rust:**
- Use `default-features = false` on `image`, or depend only on `png`.
- **Do not use `resvg`.** We generate the SVG ourselves, so rasterise modules directly instead of re-rendering SVG.
- Put decoding (`rqrr`) behind a cargo feature that is on in release builds.
- Use `Swatinem/rust-cache` in CI.
- Enforce a **binary-size budget (<8 MB)** and a **startup budget (<10 ms, measured with `hyperfine`)** as CI checks, so these claims are measured, not asserted.
- Wrap the encoder behind an internal trait, so we can swap `qrcode` for `fast_qr` or `qrcodegen` if one goes unmaintained.

## 2. Scope challenge: cut, defer or gate

"Really comprehensive" is a risk multiplier. Every flag adds test matrix, docs and attack surface. Rank features by value versus risk.

**Keep (high value, low risk):**
- Encode text, stdin or file input.
- ECC levels L/M/Q/H, version min/max, quiet-zone, scale.
- FG/BG colours, with a **contrast check that warns or fails**.
- Outputs: PNG, SVG, terminal (UTF-8 half-block and ANSI), stdout piping.
- Typed payload builders: URL, WiFi, vCard 3.0/MeCard, email, SMS, tel, geo. These are cheap, but **the escaping rules must be fuzzed** (WiFi `\ ; , : "`).
- `--verify`: decode round-trip after rendering. This is the most valuable trust feature; no mainstream CLI does it by default.
- `decode` subcommand for PNG/JPEG input. It reuses `rqrr` and doubles as the test oracle.
- Shell completions and man page (`clap_complete`, `clap_mangen`).
- JSON output mode for scripting. Meaningful exit codes.

**Gate behind `--verify` / ECC H (medium risk):**
- Logo overlay: forces ECC H, caps area at ~20%, verifies automatically, and gets local file input only.
- Module styles (dots, rounded): offer at most 2–3. Styled codes measurably fail on older scanners.
- PDF output: vector, hand-written or via `svg2pdf`. Low demand but cheap if kept minimal.
- Micro QR: supported by kennytm `qrcode`. Keep if that crate is chosen, otherwise defer.

**Defer to v2:**
- Batch/CSV mode: path-traversal risk in templated output names. If built, canonicalise paths and refuse `..`.
- Config files.
- Structured append.
- EPS, WebP, GIF.
- Gradients.
- Clipboard support: `arboard` on Linux X11/Wayland is a portability tax.

**Cut:**
- Fetching logos or inputs by URL. This brings SSRF, a network dependency and a much larger TLS stack.
- Webcam scanning.
- HTTP server or GUI modes.
- "Artistic" or AI QR codes.
- JPEG *output*: lossy compression is wrong for QR.
- EPC/SEPA/crypto payment payloads. These carry financial-correctness liability and need region-specific validation; defer unless there is real demand.
- Dynamic or short-link QR. That is a SaaS, not a CLI.

## 3. Security and quality must-haves

- Hard input-size limit: the QR max is 2,953 bytes in binary mode at ECC L. Reject anything larger before encoding.
- Refuse to overwrite files without `--force`.
- Escape XML in SVG metadata or titles.
- On decode, set `image` decoder limits (max dimensions and allocation) to prevent decompression bombs.
- `#![forbid(unsafe_code)]`.
- CI: `cargo-deny` (licences and advisories), `clippy -D warnings`, `rustfmt --check`, and `cargo-llvm-cov` with a **≥90% line threshold** on the library crate. Exclude only `main.rs` glue.
- Property tests (`proptest`) for encode→decode round-trips, plus a fuzz target for the payload escapers.
- Workspace layout: `qr-core` (library, pure, no I/O) and `qr-cli` (clap, I/O). This makes the core testable and coverage honest.

## 4. Recommendation

**Choose Rust**, conditionally:
1. **Prove the claims in PR 1.** A spike measuring binary size and startup with `hyperfine`, against budgets enforced in CI.
2. **Keep the dependency tree minimal.** No `resvg`, `image` with `default-features = false`, and the encoder behind an internal trait.
3. **Ship a focused v1.** Core encode, PNG/SVG/terminal output, payload builders, `--verify`, `decode`, completions and JSON output. Treat logo, styles and PDF as gated extras, and drop the cut list explicitly in the README's "non-goals".
4. **Make distribution part of v1, not an afterthought.** Use `cargo-dist` for GitHub Releases (macOS arm64/x64, Linux musl x64/arm64, Windows x64), a Homebrew tap, and `cargo install`/`cargo binstall`. musl static builds give real Linux portability.

**Reasons to revisit the decision:**
- If the team finds Rust compile times or contributor friction blocking during the first two PRs, the fallback is **Go**, not TS or Python.
- Pick the encoder crate on evidence, not familiarity: the most recent release date on crates.io, `cargo-audit` results, and whether a 1,000-input round-trip through `rqrr` passes.
