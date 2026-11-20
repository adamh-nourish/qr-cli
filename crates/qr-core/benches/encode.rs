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

/// Forced mask. `qrcode`'s automatic mask search draws and scores all
/// eight masks, so forcing one skips most of the work: compare with
/// `encode_by_version` to see what the lowest-penalty search costs.
#[divan::bench(args = [5, 20, 40])]
fn encode_fixed_mask(bencher: divan::Bencher, version: u8) {
    let (data, options) = full_payload(version, EcLevel::M);
    let options = options.with_mask(Mask::new(0));
    bencher.bench(|| encode(black_box(&data), black_box(&options)));
}

/// Mixed data that automatic mode splits into segments: a short URL with a
/// numeric id, and a byte prefix on 1,000 digits (EC level M).
#[divan::bench(args = ["url", "ref"])]
fn encode_segmented(bencher: divan::Bencher, payload: &str) {
    let data: Vec<u8> = if payload == "url" {
        b"https://example.com/orders/1234567890123456".to_vec()
    } else {
        let mut data = b"ref:".to_vec();
        data.extend((0..1_000).map(|i| b"0123456789"[i % 10]));
        data
    };
    let options = EncodeOptions::default();
    bencher.bench(|| encode(black_box(&data), black_box(&options)));
}

/// The `qrcode` crate called directly on the same full payloads (automatic
/// mask, EC level M). The difference from `encode_by_version` is the cost
/// of `qr_core`'s planning, segment hand-off and mask recovery.
#[divan::bench(args = [5, 20, 40])]
fn qrcode_direct(bencher: divan::Bencher, version: u8) {
    let (data, _) = full_payload(version, EcLevel::M);
    let qr_version = qrcode::Version::Normal(i16::from(version));
    bencher
        .bench(|| qrcode::QrCode::with_version(black_box(&data), qr_version, qrcode::EcLevel::M));
}
