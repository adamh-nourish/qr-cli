## Performance review proposal: qr-cli (Rust)

Note: all numbers below are targets to validate on the first benchmark run (CI runner, x86_64 Linux, plus an M-series Mac). They are not measured facts. Re-baseline in PR 1 of the stack, then lock in.

### 1. Language/crate verdict
Rust is right: no runtime, ~1 ms process start, single static binary. Performance is dominated by (a) process startup for the common one-shot case and (b) raster/encode work for large batches.

### 2. Performance goals (release build, 1 vCPU unless stated)
| Metric | Target | Hard budget (CI fail) |
|---|---|---|
| Cold startup (`qr --version`) | <= 3 ms | 8 ms |
| One code, 100-char payload, to terminal (UTF8 half-blocks) | <= 5 ms wall | 12 ms |
| Same, SVG file | <= 5 ms | 12 ms |
| Same, PNG 512px | <= 8 ms | 20 ms |
| Same, PNG 2048px | <= 25 ms | 60 ms |
| Same, JPEG/WebP 512px | <= 12 ms | 30 ms |
| Same, PDF/EPS (vector) | <= 6 ms | 15 ms |
| Max-size code (v40-L, ~2.9 KB) encode only | <= 3 ms | 8 ms |
| Styled output (rounded modules, logo overlay, gradient) 1024px | <= 40 ms | 100 ms |
| Batch, 10k short payloads to PNG 256px, 8 cores | >= 5k codes/s (<= 2 s total) | 2.5k/s |
| Batch, 10k to SVG, 8 cores | >= 20k codes/s | 10k/s |
| Peak RSS, single code | <= 12 MB | 20 MB |
| Peak RSS, batch 100k lines streamed | <= 60 MB, flat (not proportional to N) | 100 MB |
| Release binary, default features, stripped | <= 3 MB | 5 MB |
| Release binary, all features | <= 8 MB | 12 MB |

Startup rules: no lazy_static/regex compile at startup, no config file scan unless a flag or env var requests it, no network or TLS stack in default features, no tokio. clap derive adds ~0.3 MB and under 1 ms; use `clap` with `default-features=false` plus needed features, and generate shell completions and man pages at build or release time (`clap_complete`, `clap_mangen`) rather than at runtime.

### 3. Crate choices
- **QR encoding: `qrcode` vs `fast_qr`.** fast_qr is typically 2-6x faster on micro-benchmarks and has built-in SVG/image/shape helpers, but encoder cost is microseconds either way, so it is not the bottleneck (see budgets above: encode is under 1 ms of a 5 ms run). Correctness, ECI/Kanji/structured-append/Micro QR coverage and maintenance matter more. Recommendation: put the encoder behind a trait (`QrEncoder`); ship `qrcode` by default if it covers the feature list (Micro QR, mask selection, ECC levels, mixed-mode segmentation), and run a benchmark bake-off with `fast_qr` in PR 1. Switch only if fast_qr is more than 3x faster at v20+ AND passes the same conformance test vectors. Devil's advocate should hold us to this data.
- **Raster: draw directly, do not rasterise SVG through resvg.** A QR code is a boolean grid; filling rectangles into a `Vec<u8>` (1 byte/pixel for greyscale, palette for 2 colours) is O(pixels) with trivial constants. resvg pulls in tiny-skia, usvg, fontdb: +2-3 MB binary and 10-50x slower for this job. Make resvg an optional feature (`svg-raster`) used only for exotic styled templates, or omit it entirely. Styled shapes (circles, rounded corners) can be done with a small analytic coverage routine (supersample 4x only on edge modules).
- **PNG:** use the `png` crate directly (not full `image`), with `Compression::Fast` default, 1-bit/indexed colour type for bilevel codes (8-32x smaller encode work and file size). Offer `--optimize` to use `oxipng` as an optional feature.
- **JPEG/WebP/GIF/BMP:** each behind a Cargo feature. `image` with `default-features=false` plus only requested codecs; prefer `jpeg-encoder` and `webp` (lossless) as lightweight alternatives. Default feature set: terminal, SVG, PNG, plus text formats (ASCII, ANSI, UTF8, JSON matrix). Full release adds the rest.
- **PDF/EPS:** emit by hand (vector path ops; a code is a few KB). Avoid `printpdf`/`lopdf` weight in default features.
- **Parallelism:** `rayon` only in the batch path, behind a feature or at least not initialised for single-code runs (rayon thread-pool spin-up costs ~0.3-1 ms; do not touch it in the one-shot path). Use bounded pipeline: reader thread, rayon `par_bridge` or chunked `par_iter` over 1024-line chunks, ordered writer. Memory is bounded by chunk size, not input size. Default threads = physical cores, `--jobs N` to override.
- **Output:** one `BufWriter` (64 KB) on stdout; lock stdout once; skip terminal-capability probing unless stdout is a TTY. Reuse per-thread scratch buffers (grid, pixel buffer) in batch mode to avoid allocator churn; consider `mimalloc` only if benchmarks show allocator contention (do not add by default: +~100 KB, musl malloc is the real concern, see below).

### 4. Release profile
```
[profile.release]
opt-level = 3        # evaluate "s" for the minimal build only
lto = "fat"          # thin if release build time > 5 min in CI
codegen-units = 1
panic = "abort"
strip = "symbols"
debug = 0
```
- Add `[profile.dist]` (inherits release) used by release workflow; keep `release` fast to iterate with `lto="thin"` and a `profile.bench` with `debug=1` for profiling.
- musl targets use a slow allocator under multithreading; either build Linux release against glibc plus a musl build with `mimalloc` enabled via feature, and benchmark batch throughput on both. Gate musl-default on measured batch result.
- Consider PGO as a later optimisation only; expected gain 5-10% on raster path, not worth CI complexity initially. Avoid UPX (breaks AV scanners on Windows, hurts startup).
- Track `cargo bloat` and `cargo build --timings` for size regressions; `cargo-deny`/`cargo tree -d` to keep duplicate dependency versions out.

### 5. Benchmarking approach
- **Micro (in-process):** `criterion` (stable, widely known; or `divan` which is simpler and has allocation counting). Recommendation: `divan` for ergonomics plus `divan`'s alloc profiler, or criterion if we want HTML reports. Pick one, not both. Groups: encode by version (v1, v10, v25, v40) and ECC; render per format; style variants; batch chunk throughput.
- **Macro (process-level):** `hyperfine --warmup 5 --runs 50 --shell=none` for startup and each format; export JSON. Include a `scripts/bench.sh` that builds the dist profile first. Run batch scenarios with a generated 10k/100k-line fixture and `/usr/bin/time -l` (macOS) or `-v` (Linux) for peak RSS.
- **Binary size:** a CI step that records `stat` size of the stripped binary per target and fails over budget.
- **Memory:** `dhat-rs` behind a feature flag for allocation count regression tests on one-shot path (target: <= 200 allocations for single terminal render).

### 6. CI gating on regressions
Shared CI runners are noisy (10-30% variance), so strict time-based gates cause false failures. Policy:
1. **Hard gates (deterministic, always blocking):** binary size budget, allocation counts (dhat), startup under the generous hard budget above, `cargo build` of the minimal feature set.
2. **Soft gates (advisory):** criterion/divan comparison against base branch in the same job (run base and PR back-to-back on the same runner, use instruction counts via `iai-callgrind` where Linux is available: deterministic, zero noise). Block at >10% instruction-count regression; warn at >15% wall-time regression via `critcmp` or `github-action-benchmark` comment on the PR.
3. **Nightly/weekly:** full macro suite on all three OSes, trend stored as artifact (gh-pages JSON), alert issue if a metric drifts >20% from the 30-day median.
Use a dedicated benchmark job that only runs on PRs touching `src/`, to save CI minutes. Do not run benchmarks in the coverage job (instrumentation distorts them).

### 7. Suggested sequencing in the stacked PRs
- PR 1: scaffold, release profile, encoder trait, bench harness skeleton with baseline numbers committed as `bench/baseline.json`.
- Format PRs each add a benchmark group alongside the renderer.
- Batch PR adds rayon pipeline and the throughput/RSS fixtures.
- CI PR wires size gate, instruction-count gate and nightly trend job.

### 8. Open questions for devil's advocate and delivery
- Is `fast_qr` worth the swap on evidence? (benchmark in PR 1.)
- glibc vs musl default for Linux release (batch throughput vs portability).
- Whether styled output (logos, gradients) justifies pulling a 2D raster library, or whether a bespoke renderer is enough.