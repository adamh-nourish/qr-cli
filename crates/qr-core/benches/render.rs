//! Renderer micro-benchmarks (divan), one group per output format. Run with
//! `cargo bench -p qr-core --bench render`.
//!
//! Each group renders full symbols at versions 1, 10 and 40 (EC M) at the
//! default 10 px per module and 4-module margin, so the image is 290, 570
//! or 1,850 px across. Encoding is done once, outside the timed loop. These
//! numbers are tracked, not gated (DECISIONS.md section 5).

use std::hint::black_box;

use qr_core::{
    EcLevel, EncodeOptions, Matrix, Mode, RasterFormat, RenderOptions, VectorFormat, Version,
    VersionChoice, encode, max_chars, render_raster, render_vector,
};

fn main() {
    divan::main();
}

const VERSIONS: [u8; 3] = [1, 10, 40];

/// The matrix of a byte-mode symbol of `version` filled to capacity.
fn full_matrix(version: u8) -> Matrix {
    let version = Version::new(version).unwrap_or(Version::MAX);
    let len = max_chars(Mode::Byte, version, EcLevel::M);
    let data: Vec<u8> = (0..len)
        .map(|i| b"abcdefghijklmnopqrstuvwxyz/:."[i % 29])
        .collect();
    let options = EncodeOptions::default().with_version(VersionChoice::Exact(version));
    match encode(&data, &options) {
        Ok(code) => code.matrix().clone(),
        Err(err) => unreachable!("benchmark payload must encode: {err}"),
    }
}

fn raster(bencher: divan::Bencher, format: RasterFormat, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default();
    bencher.bench(|| render_raster(black_box(&matrix), format, black_box(&options)));
}

fn vector(bencher: divan::Bencher, format: VectorFormat, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default();
    bencher.bench(|| render_vector(black_box(&matrix), format, black_box(&options)));
}

/// 1-bit indexed PNG.
#[divan::bench(args = VERSIONS)]
fn png(bencher: divan::Bencher, version: u8) {
    raster(bencher, RasterFormat::Png, version);
}

/// RGBA PNG (transparent background), the slower PNG path.
#[divan::bench(args = VERSIONS)]
fn png_rgba(bencher: divan::Bencher, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default().with_transparent(true);
    bencher.bench(|| render_raster(black_box(&matrix), RasterFormat::Png, black_box(&options)));
}

/// Greyscale baseline JPEG at quality 92.
#[divan::bench(args = VERSIONS)]
fn jpeg(bencher: divan::Bencher, version: u8) {
    raster(bencher, RasterFormat::Jpeg, version);
}

/// Lossless WebP.
#[divan::bench(args = VERSIONS)]
fn webp(bencher: divan::Bencher, version: u8) {
    raster(bencher, RasterFormat::Webp, version);
}

/// SVG.
#[divan::bench(args = VERSIONS)]
fn svg(bencher: divan::Bencher, version: u8) {
    vector(bencher, VectorFormat::Svg, version);
}

/// PDF.
#[divan::bench(args = VERSIONS)]
fn pdf(bencher: divan::Bencher, version: u8) {
    vector(bencher, VectorFormat::Pdf, version);
}

/// EPS.
#[divan::bench(args = VERSIONS)]
fn eps(bencher: divan::Bencher, version: u8) {
    vector(bencher, VectorFormat::Eps, version);
}
