## Delivery Engineer Proposal: Distribution and Release

### 1. Portability rules for the codebase (apply from PR 1)
- **Pure-Rust dependencies only.** Use `qrcode`/`fast_qr`, `image` with only the `png`/`jpeg`/`webp` features, and `resvg`/`tiny-skia`. Don't depend on system libraries (no `fontconfig`, no `openssl`). If a feature needs text, bundle the font with `include_bytes!` instead of loading system fonts. This keeps the musl static builds and Windows MSVC builds free of build pain.
- **Release profile:** `lto = "fat"`, `codegen-units = 1`, `strip = true`, `opt-level = 3`. Add a `dist` profile that inherits from it. Track binary size in CI and set a budget of under 5 MB.
- **Toolchain and lockfile:** pin the toolchain in `rust-toolchain.toml`, declare `rust-version` (the MSRV) in `Cargo.toml`, commit `Cargo.lock`, and pass `--locked` to every CI build.
- **Generated extras:** an `xtask` (or a hidden subcommand) generates shell completions (bash, zsh, fish, PowerShell, elvish) with `clap_complete` and a man page with `clap_mangen`. These go into the release archives so Homebrew and Scoop can install them.

### 2. Build matrix
| Target | Runner | Notes |
|---|---|---|
| `x86_64-unknown-linux-musl` | ubuntu-24.04 | Main Linux artifact, fully static |
| `aarch64-unknown-linux-musl` | ubuntu-24.04-arm (native) | Static |
| `x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu` | ubuntu x64/arm | Optional, mainly for `cargo binstall` users. musl covers most needs. |
| `aarch64-apple-darwin` | macos-latest (arm64) | |
| `x86_64-apple-darwin` | cross-compiled on arm64 macOS | Intel runners are being retired |
| `x86_64-pc-windows-msvc` | windows-latest | Static CRT (`+crt-static`) |
| `aarch64-pc-windows-msvc` | windows-11-arm | |

- **Smoke test on every native target:** run `qr --version`, generate a PNG and an SVG, decode the PNG back with `rqrr`, and assert the payload matches. This catches platform-specific failures in encoding, paths and line endings.
- **PR CI vs release CI:** PR CI runs test and coverage on the three OS families with host targets only. The full cross-target matrix runs only on release tags and on a nightly schedule, to save Actions minutes (see the private-repo notes).

### 3. Release automation: release-plz plus cargo-dist (recommended)
- **release-plz** handles versioning. It reads conventional commits, bumps the SemVer version, writes `CHANGELOG.md` (git-cliff based, Keep a Changelog format) and opens a "Release PR". Merging that PR creates the `vX.Y.Z` tag.
  - Set `git_release_enable = false` so that cargo-dist owns the GitHub Release.
  - Set `publish = false` until the repo is public.
- **cargo-dist (`dist`)** handles artifacts. It generates `release.yml`, which builds the matrix, produces `.tar.xz`/`.zip` archives, SHA-256 checksums, a shell installer and a PowerShell installer, and optionally a Homebrew formula, an MSI, npm packages and CycloneDX SBOMs. It also runs `dist plan` on PRs, so packaging config is validated before any release.
- **Why not hand-rolled Actions:** about 400 or more lines of YAML that we would have to maintain ourselves, while cargo-dist is the de facto standard for this kind of Rust CLI (ripgrep-style projects, uv, and others). We keep the ability to opt out because the generated workflow is ordinary YAML that we commit.
- **Conventional commits:** enforce them through PR-title linting (`amannn/action-semantic-pull-request`) together with squash merging. This works well with stacked PRs: each squashed PR becomes one changelog entry. Start at `0.1.0` and treat `0.x` as unstable.
- **Token:** release-plz needs a GitHub App token or a fine-grained PAT, not `GITHUB_TOKEN`. Otherwise the Release PR does not trigger CI.

### 4. Supply chain
- **Actions and builds:** pin every action by SHA, keep Dependabot (or Renovate) on for both Actions and Cargo, and build with `cargo auditable` so binaries embed their dependency list for `cargo audit bin`.
- **Checksums and provenance:** publish SHA-256 checksums from v0.1.0. Add Sigstore build-provenance attestations (`actions/attest-build-provenance`, supported by cargo-dist) once the repo is public.
- **Code signing:** deferred. macOS notarization and Windows Authenticode need paid certificates. The curl, Homebrew and Scoop install paths skip quarantine and SmartScreen; browser downloads will show warnings. Document this in the README.

### 5. Install channels and phasing
| Channel | Phase | Private-repo status |
|---|---|---|
| GitHub Releases plus checksums | v0.1.0 | Works, but only authenticated collaborators can download |
| `curl \| sh` and `irm \| iex` installers (cargo-dist) | v0.1.0 | **Broken.** Anonymous release download returns 404. Test with `gh release download` instead. |
| `cargo install --git` | v0.1.0 | Works with git credentials |
| crates.io (`cargo install qr-…`) | On going public | **Do not publish while private**: it publishes the source. Check the crate name now: `qr-cli` or `qr` may already be taken, so reserve a fallback name. Use crates.io trusted publishing (OIDC) rather than a long-lived token. |
| `cargo binstall` | On going public | Needs `[package.metadata.binstall]` and public assets |
| Homebrew tap (`adamh-nourish/homebrew-tap`, generated by cargo-dist) | On going public | **Broken.** Formula URLs return 404 without auth. The tap repo must also be public. |
| Scoop bucket | On going public | **Broken** (needs public URLs) |
| Docker/OCI on GHCR, `FROM scratch` with the static musl binary, multi-arch | On going public | Private GHCR packages need `docker login`. Image under 5 MB. |
| winget | After 1.0 or a stable CLI | Requires a public URL plus a PR to `microsoft/winget-pkgs` (`winget-releaser` action). Unsigned exe is acceptable. |
| Nix flake (`nix run github:…`) | Later, community-driven | Private repo needs an access token in nix.conf |
| npm wrapper | **Skip** | Adds a JS supply-chain surface and duplicates binstall/brew for no real gain. Revisit only if users ask for `npx qr`. |
| Homebrew core, distro repos (AUR, Debian) | Later | Need adoption and notability first |

### 6. Other things that break or cost money while private
- **Actions minutes:** billed against the private quota, with macOS at a 10x multiplier and Windows at 2x. Hence the full matrix only on tags and nightly.
- **Branch protection and rulesets** (required status checks, needed to make CI "enforce" lint and coverage) are **not available on private repos on the GitHub Free plan**. Confirm whether `adamh-nourish` is on Pro or Team; otherwise enforcement is only by convention until the repo goes public.
- **Coverage services:** Codecov and Coveralls need tokens for private repos. Alternative: enforce the coverage threshold in CI with `cargo llvm-cov --fail-under-lines 90`, with no external service.
- **Artifact attestations** on private repos need GitHub Enterprise Cloud, so add them when the repo goes public.
- **README install instructions** won't work for anyone else until the repo is public. Mark them "available at first public release".

### 7. PR plan for delivery
- **PR 1 (scaffold):**
  - `Cargo.toml` metadata (name, description, `license = "MIT OR Apache-2.0"`, repository, keywords, `rust-version`), both LICENSE files, `rust-toolchain.toml` and the release profile.
  - `.github/workflows/ci.yml` running fmt, clippy `-D warnings`, test and `llvm-cov` on ubuntu, macOS and Windows.
  - PR-title lint, `cargo-deny` (licenses and advisories), and SHA-pinned actions.
- **PR 2 (release plumbing, can land early since it is independent of features):**
  - `dist init` output: `release.yml` and the `dist` section of `Cargo.toml` with all 7 targets, shell and PowerShell installers, checksums, and Homebrew/Scoop configured but publishing disabled.
  - `release-plz.toml` and its workflow with publishing disabled.
  - `CHANGELOG.md` seed.
  - `dist plan` running on PRs.
- **Mid-stack:** the completions and man-page `xtask`, and the cross-platform smoke-test job (round-trip decode).
- **End of stack:** merge the Release PR, which produces `v0.1.0` as a private GitHub Release. Verify the artifacts with `gh release download` on each OS.
- **Go-public checklist (one PR plus manual settings):**
  - Flip visibility and enable rulesets.
  - Create the `homebrew-tap` and `scoop-bucket` repos and the tap token secret.
  - Enable crates.io trusted publishing and binstall metadata.
  - Enable attestations and the GHCR workflow.
  - Turn on `publish = true` in release-plz.
  - Cut `v0.1.1` to exercise every channel end to end.
