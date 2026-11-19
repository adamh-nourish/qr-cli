//! Verification: read a rendered code back with `rqrr` and check that it
//! holds exactly the encoded bytes (`--verify`, DECISIONS section 3).
//!
//! - Raster output ([`verify_raster`]) is decoded from the file's own bytes,
//!   so encoder bugs and JPEG artefacts are caught too.
//! - Vector and text output ([`verify_rendering`]) has no pixels, so the
//!   matrix is drawn by our raster renderer with the same shape and colours
//!   at [`VERIFY_SCALE`] pixels per module.
//!
//! Transparent pixels are flattened onto the intended backdrop (the
//! background colour, as if the image were placed on a page of that
//! colour). `rqrr` only reads dark modules on a light background, so an
//! image that yields nothing is tried again with its luma inverted, as
//! phone scanners do for light-on-dark codes.
//!
//! A quiet zone narrower than the standard four modules has its own
//! warning, so verification checks the symbol rather than the margin: the
//! image is read with its quiet zone widened to four modules in the
//! background colour, as if it were placed on a page of that colour.
//!
//! The work is bounded: an image above [`VERIFY_MAX_PIXELS`] is not decoded
//! but redrawn at the largest module size that fits the budget.

use std::io::Cursor;

use image::{ImageFormat, ImageReader};
use thiserror::Error;

use crate::color::Color;
use crate::matrix::Matrix;
use crate::render::raster::{GreyImage, greyscale, luma};
use crate::render::{DEFAULT_MARGIN, DEFAULT_SCALE, RasterFormat, RenderError, RenderOptions};

/// The module size, in pixels, that vector and text output is drawn at for
/// verification: the raster default.
pub const VERIFY_SCALE: u32 = DEFAULT_SCALE;

/// The smallest module size, in pixels, handed to `rqrr`. Smaller images
/// are enlarged first: with one pixel per module `rqrr` 0.9 can trip an
/// internal assertion, which would abort a release build.
const MIN_READ_SCALE: usize = 3;

/// The most pixels verification decodes: 2,048 x 2,048. A version 40
/// symbol with the default quiet zone fits at 11 pixels per module.
pub const VERIFY_MAX_PIXELS: u64 = 2_048 * 2_048;

/// Why a rendering could not be verified.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum VerifyError {
    /// No QR code could be read from the image.
    #[error("no QR code could be read back from the rendered {format}")]
    NotFound {
        /// The output format, such as `png`.
        format: &'static str,
    },
    /// A code was read, but it holds different data.
    #[error(
        "the rendered {format} reads back as {decoded} bytes that differ from the {expected} bytes encoded"
    )]
    Mismatch {
        /// The output format, such as `png`.
        format: &'static str,
        /// The number of bytes encoded.
        expected: usize,
        /// The number of bytes read back.
        decoded: usize,
    },
    /// The image to read back could not be drawn.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// Our own output could not be decoded back to pixels. This is a bug.
    #[error("cannot read back the rendered {format}: {message}")]
    ReadBack {
        /// The output format, such as `png`.
        format: &'static str,
        /// The decoder's description of the failure.
        message: String,
    },
}

impl VerifyError {
    /// Whether the code itself failed verification (it could not be read,
    /// or read back as different data), as opposed to verification being
    /// unable to run.
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::NotFound { .. } | Self::Mismatch { .. })
    }
}

/// How a rendering was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    /// The side of the image that was read, in pixels.
    pub side: u32,
    /// Whether the image was drawn for verification rather than decoded from
    /// the output bytes (vector and text output, or an image above
    /// [`VERIFY_MAX_PIXELS`]).
    pub redrawn: bool,
    /// Whether the code was only read with its luma inverted (light modules
    /// on a dark background).
    pub inverted: bool,
}

/// Verifies raster output: decodes `bytes` (as rendered from `matrix` with
/// `options` in `format`) and reads the code back.
///
/// # Errors
///
/// [`VerifyError::NotFound`] or [`VerifyError::Mismatch`] if the image does
/// not read back as `expected`; [`VerifyError::ReadBack`] if the bytes are
/// not a valid image of `format`; [`VerifyError::Render`] if an oversized
/// image cannot be redrawn.
///
/// ```
/// use qr_core::verify::verify_raster;
/// use qr_core::{EncodeOptions, RasterFormat, RenderOptions, encode, render_raster};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let options = RenderOptions::default();
/// let png = render_raster(code.matrix(), RasterFormat::Png, &options)?;
/// let verified = verify_raster(code.matrix(), RasterFormat::Png, &png, &options, b"hello")?;
/// assert_eq!(verified.side, 290);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn verify_raster(
    matrix: &Matrix,
    format: RasterFormat,
    bytes: &[u8],
    options: &RenderOptions,
    expected: &[u8],
) -> Result<Verified, VerifyError> {
    let name = format.extension();
    let backdrop = backdrop(options, format.has_alpha());
    let read_back = |e: image::ImageError| VerifyError::ReadBack {
        format: name,
        message: e.to_string(),
    };
    let reader = ImageReader::with_format(Cursor::new(bytes), image_format(format));
    let (width, height) = reader.into_dimensions().map_err(read_back)?;
    if width != height {
        return Err(VerifyError::ReadBack {
            format: name,
            message: format!("expected a square image, got {width}x{height}"),
        });
    }
    if u64::from(width) * u64::from(height) > VERIFY_MAX_PIXELS {
        let scale = fitting_scale(matrix, options);
        let image = greyscale(matrix, &redraw_options(options, scale), backdrop)?;
        return read(
            &readable(image, matrix, options, backdrop),
            expected,
            name,
            true,
        );
    }
    let decoded = image::load_from_memory_with_format(bytes, image_format(format))
        .map_err(read_back)?
        .into_rgba8();
    let pixels = decoded
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            luma(Color::rgba(r, g, b, a).over(backdrop))
        })
        .collect();
    let image = GreyImage {
        side: width,
        pixels,
    };
    read(
        &readable(image, matrix, options, backdrop),
        expected,
        name,
        false,
    )
}

/// Verifies output that has no pixels of its own (vector or text formats,
/// named by `format`): draws `matrix` with `options` at [`VERIFY_SCALE`]
/// pixels per module (less if needed to stay within [`VERIFY_MAX_PIXELS`])
/// and reads the code back.
///
/// # Errors
///
/// [`VerifyError::NotFound`] or [`VerifyError::Mismatch`] if the drawing
/// does not read back as `expected`, or [`VerifyError::Render`] if it
/// cannot be drawn.
///
/// ```
/// use qr_core::verify::verify_rendering;
/// use qr_core::{EncodeOptions, ModuleShape, RenderOptions, encode};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let options = RenderOptions::default().with_shape(ModuleShape::Dots);
/// let verified = verify_rendering(code.matrix(), &options, "svg", b"hello")?;
/// assert!(verified.redrawn);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn verify_rendering(
    matrix: &Matrix,
    options: &RenderOptions,
    format: &'static str,
    expected: &[u8],
) -> Result<Verified, VerifyError> {
    let scale = VERIFY_SCALE.min(fitting_scale(matrix, options));
    let backdrop = backdrop(options, true);
    let image = greyscale(matrix, &redraw_options(options, scale), backdrop)?;
    read(
        &readable(image, matrix, options, backdrop),
        expected,
        format,
        true,
    )
}

/// `image` prepared for `rqrr`: enlarged to at least [`MIN_READ_SCALE`]
/// pixels per module, and with its quiet zone widened to
/// [`DEFAULT_MARGIN`] modules in the background colour as drawn on
/// `backdrop`.
fn readable(
    image: GreyImage,
    matrix: &Matrix,
    options: &RenderOptions,
    backdrop: Color,
) -> GreyImage {
    let margin = options.margin.min(crate::render::MAX_MARGIN);
    let modules = (matrix.width() + 2 * margin).max(1);
    let scale = (image.side as usize / modules).max(1);
    let image = if scale < MIN_READ_SCALE {
        enlarge(&image, MIN_READ_SCALE.div_ceil(scale))
    } else {
        image
    };
    let Some(missing) = DEFAULT_MARGIN.checked_sub(margin).filter(|m| *m > 0) else {
        return image;
    };
    let scale = image.side as usize / modules;
    let pad = missing * scale;
    let side = image.side as usize + 2 * pad;
    let light = luma(options.background().over(backdrop));
    let mut pixels = vec![light; side * pad];
    for row in image.pixels.chunks_exact(image.side as usize) {
        pixels.extend(std::iter::repeat_n(light, pad));
        pixels.extend_from_slice(row);
        pixels.extend(std::iter::repeat_n(light, pad));
    }
    pixels.extend(std::iter::repeat_n(light, side * pad));
    GreyImage {
        side: u32::try_from(side).unwrap_or(image.side),
        pixels,
    }
}

/// `image` scaled up `factor` times, each pixel becoming a square block.
fn enlarge(image: &GreyImage, factor: usize) -> GreyImage {
    let side = image.side as usize * factor;
    let mut pixels = Vec::with_capacity(side * side);
    for row in image.pixels.chunks_exact((image.side as usize).max(1)) {
        let start = pixels.len();
        for &p in row {
            pixels.extend(std::iter::repeat_n(p, factor));
        }
        for _ in 1..factor {
            pixels.extend_from_within(start..start + side);
        }
    }
    GreyImage {
        side: u32::try_from(side).unwrap_or(image.side),
        pixels,
    }
}

/// The largest whole module size that keeps the drawing within
/// [`VERIFY_MAX_PIXELS`], at least 1.
fn fitting_scale(matrix: &Matrix, options: &RenderOptions) -> u32 {
    // `VERIFY_MAX_PIXELS` is a square of this side.
    const MAX_SIDE: usize = 2_048;
    let modules = matrix
        .width()
        .saturating_add(options.margin.min(crate::render::MAX_MARGIN) * 2);
    u32::try_from(MAX_SIDE / modules.max(1)).unwrap_or(1).max(1)
}

/// `options` with only the geometry that affects reading: the shape,
/// colours and margin, at `scale` pixels per module.
fn redraw_options(options: &RenderOptions, scale: u32) -> RenderOptions {
    RenderOptions {
        size: None,
        dpi: None,
        title: None,
        ..options.clone()
    }
    .with_scale(scale)
}

/// The opaque colour transparent pixels are flattened onto: the intended
/// background colour for a transparent image in a format with an alpha
/// channel, otherwise white (JPEG flattens transparency onto white).
fn backdrop(options: &RenderOptions, alpha: bool) -> Color {
    if options.transparent && alpha {
        options.bg.over(Color::WHITE)
    } else {
        Color::WHITE
    }
}

fn image_format(format: RasterFormat) -> ImageFormat {
    match format {
        RasterFormat::Png => ImageFormat::Png,
        RasterFormat::Jpeg => ImageFormat::Jpeg,
        RasterFormat::Webp => ImageFormat::WebP,
    }
}

/// What one attempt to read an image found.
enum Read {
    /// A symbol holding the expected data.
    Match,
    /// Symbols were decoded, but none held the expected data; the length of
    /// the first one is kept for the error.
    Mismatch(usize),
    /// Nothing decoded.
    Nothing,
}

/// Reads `image`, then its inverse if nothing decodes.
fn read(
    image: &GreyImage,
    expected: &[u8],
    format: &'static str,
    redrawn: bool,
) -> Result<Verified, VerifyError> {
    let verified = |inverted| Verified {
        side: image.side,
        redrawn,
        inverted,
    };
    match scan(image, expected, false) {
        Read::Match => return Ok(verified(false)),
        Read::Mismatch(decoded) => {
            return Err(VerifyError::Mismatch {
                format,
                expected: expected.len(),
                decoded,
            });
        }
        Read::Nothing => {}
    }
    match scan(image, expected, true) {
        Read::Match => Ok(verified(true)),
        Read::Mismatch(decoded) => Err(VerifyError::Mismatch {
            format,
            expected: expected.len(),
            decoded,
        }),
        Read::Nothing => Err(VerifyError::NotFound { format }),
    }
}

/// Detects and decodes every symbol in `image` (inverted if asked).
fn scan(image: &GreyImage, expected: &[u8], invert: bool) -> Read {
    let side = image.side as usize;
    let pixel = |x: usize, y: usize| {
        let luma = image.pixels.get(y * side + x).copied().unwrap_or(u8::MAX);
        if invert { u8::MAX - luma } else { luma }
    };
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(side, side, pixel);
    let mut first = None;
    for grid in prepared.detect_grids() {
        let mut data = Vec::new();
        if grid.decode_to(&mut data).is_ok() {
            if data == expected {
                return Read::Match;
            }
            first.get_or_insert(data.len());
        }
    }
    first.map_or(Read::Nothing, Read::Mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ModuleShape, render_raster};
    use crate::{EncodeOptions, encode};

    fn code(data: &[u8]) -> Matrix {
        encode(data, &EncodeOptions::default())
            .unwrap()
            .matrix()
            .clone()
    }

    #[test]
    fn every_raster_format_reads_back() {
        let m = code(b"verify me");
        for shape in ModuleShape::ALL {
            let options = RenderOptions::default().with_shape(shape);
            for format in RasterFormat::ALL {
                let bytes = render_raster(&m, format, &options).unwrap();
                let verified = verify_raster(&m, format, &bytes, &options, b"verify me").unwrap();
                assert_eq!(
                    verified,
                    Verified {
                        side: 290,
                        redrawn: false,
                        inverted: false
                    },
                    "{format} {shape}"
                );
            }
        }
    }

    #[test]
    fn different_data_is_a_mismatch() {
        let m = code(b"one thing");
        let options = RenderOptions::default();
        let png = render_raster(&m, RasterFormat::Png, &options).unwrap();
        let err = verify_raster(&m, RasterFormat::Png, &png, &options, b"another").unwrap_err();
        assert_eq!(
            err,
            VerifyError::Mismatch {
                format: "png",
                expected: 7,
                decoded: 9
            }
        );
        assert!(err.is_failure());
        assert_eq!(
            err.to_string(),
            "the rendered png reads back as 9 bytes that differ from the 7 bytes encoded"
        );
        let err = verify_rendering(&m, &options, "svg", b"x").unwrap_err();
        assert!(matches!(err, VerifyError::Mismatch { format: "svg", .. }));
    }

    #[test]
    fn invisible_codes_are_not_found() {
        let m = code(b"ghost");
        let options = RenderOptions::default().with_fg(Color::rgb(250, 250, 250));
        let err = verify_rendering(&m, &options, "pdf", b"ghost").unwrap_err();
        assert_eq!(err, VerifyError::NotFound { format: "pdf" });
        assert!(err.is_failure());
        assert_eq!(
            err.to_string(),
            "no QR code could be read back from the rendered pdf"
        );
    }

    #[test]
    fn light_on_dark_codes_are_read_inverted() {
        let m = code(b"night mode");
        let options = RenderOptions::default()
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK);
        let verified = verify_rendering(&m, &options, "svg", b"night mode").unwrap();
        assert!(verified.inverted && verified.redrawn);
        assert_eq!(verified.side, 290);

        // A transparent image is read on its intended (black) background.
        let transparent = options.with_transparent(true);
        let png = render_raster(&m, RasterFormat::Png, &transparent).unwrap();
        let verified =
            verify_raster(&m, RasterFormat::Png, &png, &transparent, b"night mode").unwrap();
        assert!(verified.inverted && !verified.redrawn);
    }

    #[test]
    fn thin_quiet_zones_are_widened_in_the_background_colour() {
        let m = code(b"edge to edge");
        let options = RenderOptions::default()
            .with_margin(0)
            .with_scale(4)
            .with_bg(Color::rgb(0xff, 0xf8, 0xdc));
        for format in RasterFormat::ALL {
            let bytes = render_raster(&m, format, &options).unwrap();
            let verified = verify_raster(&m, format, &bytes, &options, b"edge to edge").unwrap();
            // 21 modules plus 4 on each side, at 4 pixels per module.
            assert_eq!(verified.side, 29 * 4, "{format}");
        }
        let verified = verify_rendering(&m, &options.with_margin(1), "svg", b"edge to edge");
        assert_eq!(verified.unwrap().side, 29 * VERIFY_SCALE);

        let image = GreyImage {
            side: 1,
            pixels: vec![0],
        };
        let one = Matrix::from_modules(1, vec![true]).unwrap();
        let padded = readable(
            image,
            &one,
            &RenderOptions::default().with_margin(0),
            Color::WHITE,
        );
        // Enlarged to 3 pixels per module, then 4 modules of quiet zone.
        assert_eq!(padded.side, 27);
        assert_eq!(
            padded.pixels.iter().filter(|p| p.is_power_of_two()).count(),
            0
        );
        assert_eq!(
            padded
                .pixels
                .iter()
                .map(|&p| usize::from(p == 0))
                .sum::<usize>(),
            9
        );
        assert_eq!(padded.pixels[13 * 27 + 13], 0);
        assert_eq!(padded.pixels[0], 255);
    }

    #[test]
    fn one_pixel_modules_are_enlarged_before_reading() {
        let m = code(b"tiny");
        let options = RenderOptions::default()
            .with_scale(1)
            .with_shape(ModuleShape::Dots);
        for format in [RasterFormat::Png, RasterFormat::Webp] {
            let bytes = render_raster(&m, format, &options).unwrap();
            let verified = verify_raster(&m, format, &bytes, &options, b"tiny").unwrap();
            assert_eq!(verified.side, 29 * 3, "{format}");
        }
        let image = GreyImage {
            side: 2,
            pixels: vec![1, 2, 3, 4],
        };
        assert_eq!(
            enlarge(&image, 2).pixels,
            [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]
        );
    }

    #[test]
    fn jpeg_transparency_is_read_on_white() {
        // JPEG has no alpha: a transparent navy background becomes white,
        // so gold modules are drawn on white, and that is what is read.
        let m = code(b"gold");
        let options = RenderOptions::default()
            .with_fg(Color::rgb(0x1a, 0x73, 0xe8))
            .with_bg(Color::BLACK)
            .with_transparent(true)
            .with_margin(1);
        let jpeg = render_raster(&m, RasterFormat::Jpeg, &options).unwrap();
        let verified = verify_raster(&m, RasterFormat::Jpeg, &jpeg, &options, b"gold").unwrap();
        assert!(!verified.inverted);
        assert_eq!(backdrop(&options, false), Color::WHITE);
        assert_eq!(backdrop(&options, true), Color::BLACK);
    }

    #[test]
    fn large_images_are_redrawn_within_the_budget() {
        let m = code(b"big");
        let options = RenderOptions::default().with_size(Some(2_100));
        let png = render_raster(&m, RasterFormat::Png, &options).unwrap();
        let verified = verify_raster(&m, RasterFormat::Png, &png, &options, b"big").unwrap();
        assert!(verified.redrawn);
        // 29 modules: 2,048 / 29 = 70 pixels per module.
        assert_eq!(verified.side, 29 * 70);

        let vector = verify_rendering(&m, &options, "svg", b"big").unwrap();
        assert_eq!(vector.side, 29 * VERIFY_SCALE);
        assert_eq!(fitting_scale(&m, &options.with_margin(1_000)), 9);
    }

    #[test]
    fn broken_bytes_and_bad_options_are_errors() {
        let m = code(b"x");
        let options = RenderOptions::default();
        let err = verify_raster(&m, RasterFormat::Png, b"not a png", &options, b"x").unwrap_err();
        assert!(matches!(err, VerifyError::ReadBack { format: "png", .. }));
        assert!(!err.is_failure());
        assert!(
            err.to_string()
                .starts_with("cannot read back the rendered png: ")
        );

        // A non-square image cannot be ours.
        let mut wide = Vec::new();
        image::RgbImage::new(4, 2)
            .write_to(&mut Cursor::new(&mut wide), ImageFormat::Png)
            .unwrap();
        let err = verify_raster(&m, RasterFormat::Png, &wide, &options, b"x").unwrap_err();
        assert_eq!(
            err,
            VerifyError::ReadBack {
                format: "png",
                message: "expected a square image, got 4x2".into()
            }
        );

        let err = verify_rendering(&m, &options.with_margin(0).with_scale(0), "eps", b"x");
        // The scale is replaced, so a zero scale still verifies.
        assert!(err.is_ok());
        let render = VerifyError::from(RenderError::ZeroScale);
        assert!(!render.is_failure());
        assert_eq!(render.to_string(), "scale must be at least 1");
    }
}
