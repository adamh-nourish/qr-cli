//! Renderer micro-benchmarks (divan), one group per output format. Run with
//! `cargo bench -p qr-core --bench render`.
//!
//! Each group renders full symbols at versions 1, 10 and 40 (EC M) at the
//! default 10 px per module and 4-module margin, so the image is 290, 570
//! or 1,850 px across. Encoding is done once, outside the timed loop. These
//! numbers are tracked, not gated (DECISIONS.md section 5).

use std::hint::black_box;

use qr_core::verify::{verify_raster, verify_rendering};
use qr_core::{
    EcLevel, EncodeOptions, Matrix, Mode, ModuleShape, RasterFormat, RenderOptions, VectorFormat,
    Version, VersionChoice, encode, max_chars, render_raster, render_vector,
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

/// 1-bit PNG with dotted modules (per-pixel-row shape sampling).
#[divan::bench(args = VERSIONS)]
fn png_dots(bencher: divan::Bencher, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default().with_shape(ModuleShape::Dots);
    bencher.bench(|| render_raster(black_box(&matrix), RasterFormat::Png, black_box(&options)));
}

/// SVG with rounded modules (Bezier outlines).
#[divan::bench(args = VERSIONS)]
fn svg_rounded(bencher: divan::Bencher, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default().with_shape(ModuleShape::Rounded);
    bencher.bench(|| render_vector(black_box(&matrix), VectorFormat::Svg, black_box(&options)));
}

/// `--verify` of a PNG: decode the file and read it back with rqrr.
#[divan::bench(args = VERSIONS)]
fn verify_png(bencher: divan::Bencher, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default().with_shape(ModuleShape::Dots);
    let png = render_raster(&matrix, RasterFormat::Png, &options).unwrap_or_default();
    // The payload does not matter for timing; a mismatch reads just as far.
    bencher.bench(|| verify_raster(&matrix, RasterFormat::Png, black_box(&png), &options, b""));
}

/// `--verify` of vector output: redraw the matrix and read it back.
#[divan::bench(args = VERSIONS)]
fn verify_svg(bencher: divan::Bencher, version: u8) {
    let matrix = full_matrix(version);
    let options = RenderOptions::default().with_shape(ModuleShape::Rounded);
    bencher.bench(|| verify_rendering(black_box(&matrix), &options, "svg", b""));
}

/// One batch item as PR 11 will run it: encode a payload (automatic mode,
/// version and mask, EC M) and render a 256 px 1-bit PNG. Argument 0 is a
/// short URL with a numeric id (version 2 or 3); 10, 20 and 40 are byte
/// payloads that fill that version. The §5 batch target (2.5k codes/s on 4
/// cores) is about 1.6 ms per item per core, so the estimate is
/// `4 / median` codes per second, before I/O and scheduling overhead.
#[divan::bench(args = [0, 10, 20, 40])]
fn batch_item_png_256(bencher: divan::Bencher, version: u8) {
    let data: Vec<u8> = if version == 0 {
        b"https://example.com/item/000123".to_vec()
    } else {
        let version = Version::new(version).unwrap_or(Version::MAX);
        let len = max_chars(Mode::Byte, version, EcLevel::M);
        (0..len)
            .map(|i| b"abcdefghijklmnopqrstuvwxyz/:."[i % 29])
            .collect()
    };
    let encode_options = EncodeOptions::default();
    let options = RenderOptions::default().with_size(Some(256));
    bencher.bench(|| {
        encode(black_box(&data), black_box(&encode_options))
            .map(|code| render_raster(code.matrix(), RasterFormat::Png, &options))
    });
}
