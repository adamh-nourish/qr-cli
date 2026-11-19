//! Encoder micro-benchmarks (divan). Run with `cargo bench -p qr-core`.
//!
//! Groups follow `docs/design/proposal-performance.md`: encode by version and
//! error correction level. Later PRs add one group per renderer. These numbers
//! are tracked, not gated (DECISIONS.md section 5).

use std::hint::black_box;

use qr_core::{EcLevel, EncodeOptions, Mask, Mode, Version, VersionChoice, encode, max_chars};

fn main() {
    divan::main();
}

/// Byte-mode data that fills `version` at `ec` to capacity.
fn full_payload(version: u8, ec: EcLevel) -> (Vec<u8>, EncodeOptions) {
    let version = Version::new(version).unwrap_or(Version::MAX);
    let len = max_chars(Mode::Byte, version, ec);
    let data = (0..len)
        .map(|i| b"abcdefghijklmnopqrstuvwxyz/:."[i % 29])
        .collect();
    let options = EncodeOptions::default()
        .with_ec_level(ec)
        .with_version(VersionChoice::Exact(version));
    (data, options)
}

/// Full encode, automatic mask, EC level M, at representative versions.
#[divan::bench(args = [1, 5, 10, 20, 25, 40])]
fn encode_by_version(bencher: divan::Bencher, version: u8) {
    let (data, options) = full_payload(version, EcLevel::M);
    bencher.bench(|| encode(black_box(&data), black_box(&options)));
}

/// Version 20 across error correction levels.
#[divan::bench(args = EcLevel::ALL)]
fn encode_v20_by_ec_level(bencher: divan::Bencher, ec: EcLevel) {
    let (data, options) = full_payload(20, ec);
    bencher.bench(|| encode(black_box(&data), black_box(&options)));
}

/// Forced mask: isolates mask selection cost from the rest of encoding.
#[divan::bench(args = [5, 20, 40])]
fn encode_fixed_mask(bencher: divan::Bencher, version: u8) {
    let (data, options) = full_payload(version, EcLevel::M);
    let options = options.with_mask(Mask::new(0));
    bencher.bench(|| encode(black_box(&data), black_box(&options)));
}
