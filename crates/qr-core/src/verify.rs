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
//! The work is bounded: an image above [`VERIFY_MAX_PIXELS`], or a JPEG or
//! WebP image that would decode to more than [`VERIFY_MAX_DECODED_BYTES`],
//! is not decoded but redrawn, at no more than [`VERIFY_SCALE`] (and its
//! own module size).
//!
//! Memory is bounded too (DECISIONS section 5 tracks peak RSS): `rqrr`
//! keeps its own one-byte-per-pixel copy of the image, so the pixels it
//! reads are produced on the fly, a row at a time, from the source. A PNG
//! is decoded row by row and a drawing is drawn row by row, so neither is
//! ever held as a full bitmap; JPEG and WebP are decoded whole (within
//! [`VERIFY_MAX_DECODED_BYTES`]) and converted to luma in place. Enlarging and widening the quiet zone are
//! done by mapping coordinates, not by copying.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder as _, ImageFormat, ImageReader};
use thiserror::Error;

use crate::color::Color;
use crate::logo::Planned;
use crate::matrix::Matrix;
use crate::render::raster::{GreyImage, GreyRows, luma};
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

/// The most bytes a JPEG or WebP image may decode to (one byte per pixel
/// at [`VERIFY_MAX_PIXELS`]). Their decoders produce the whole image at up
/// to four bytes per pixel (WebP briefly seven), so a larger one is
/// redrawn instead, as vector output is, which keeps the memory that
/// verification adds within DECISIONS section 5's 20 MB. Large images
/// have large modules, where compression artefacts matter least; small
/// ones, where they matter most, are still decoded from their own bytes.
pub const VERIFY_MAX_DECODED_BYTES: u64 = VERIFY_MAX_PIXELS;

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
    /// the output bytes (vector and text output, an image above
    /// [`VERIFY_MAX_PIXELS`], or a JPEG or WebP image above
    /// [`VERIFY_MAX_DECODED_BYTES`]).
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
/// An image above [`VERIFY_MAX_PIXELS`], or a JPEG or WebP image that
/// would decode to more than [`VERIFY_MAX_DECODED_BYTES`], is redrawn
/// instead of decoded.
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
    let decoder = ImageReader::with_format(Cursor::new(bytes), image_format(format))
        .into_decoder()
        .map_err(read_back)?;
    let (width, height) = decoder.dimensions();
    if width != height {
        return Err(VerifyError::ReadBack {
            format: name,
            message: format!("expected a square image, got {width}x{height}"),
        });
    }
    // A PNG is decoded a row at a time, so only its pixel count matters;
    // JPEG and WebP are decoded whole, so their decoded size does too.
    let redraw;
    let plan;
    let decodable = u64::from(width) * u64::from(height) <= VERIFY_MAX_PIXELS
        && (format == RasterFormat::Png || decoder.total_bytes() <= VERIFY_MAX_DECODED_BYTES);
    let (source, redrawn) = if !decodable {
        // The JPEG decoder holds a copy of the file: free it before reading.
        drop(decoder);
        let modules = matrix.width() + 2 * options.margin.min(crate::render::MAX_MARGIN);
        let own = u32::try_from(width as usize / modules.max(1)).unwrap_or(u32::MAX);
        let scale = VERIFY_SCALE
            .min(fitting_scale(matrix, options))
            .min(own.max(1));
        redraw = redraw_options(options, scale);
        plan = Planned::new(matrix, &redraw)?;
        let rows = GreyRows::new(matrix, plan.as_ref(), &redraw, backdrop)?;
        (Source::Drawn(rows), true)
    } else if format == RasterFormat::Png {
        drop(decoder);
        let source = Source::Png {
            bytes,
            backdrop,
            side: width,
        };
        (source, false)
    } else {
        let image = DynamicImage::from_decoder(decoder).map_err(read_back)?;
        (Source::Grey(grey(image, backdrop)), false)
    };
    let layout = Layout::new(source.side(), matrix, options, backdrop);
    read(&source, &layout, expected, name, redrawn)
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
    let redraw = redraw_options(options, scale);
    let plan = Planned::new(matrix, &redraw)?;
    let source = Source::Drawn(GreyRows::new(matrix, plan.as_ref(), &redraw, backdrop)?);
    let layout = Layout::new(source.side(), matrix, options, backdrop);
    read(&source, &layout, expected, format, true)
}

/// A decoded JPEG or WebP image as luma on `backdrop`, converted in place
/// so that at most the decoder's own buffer is held.
fn grey(image: DynamicImage, backdrop: Color) -> GreyImage {
    let side = image.width();
    let image = match image {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageRgba8(_) => image,
        // Not produced by our encoders; converted the slow way.
        other => DynamicImage::ImageRgba8(other.into_rgba8()),
    };
    let channels = usize::from(image.color().channel_count());
    let mut pixels = image.into_bytes();
    let count = pixels.len() / channels;
    for i in 0..count {
        // Pixel `i` starts at `i * channels >= i`, so it is read before
        // anything is written over it.
        let value = luma_of(&pixels[i * channels..(i + 1) * channels], backdrop);
        pixels[i] = value;
    }
    pixels.truncate(count);
    pixels.shrink_to_fit();
    GreyImage { side, pixels }
}

/// The luma of one 8-bit grey, grey-alpha, RGB or RGBA pixel on `backdrop`.
fn luma_of(pixel: &[u8], backdrop: Color) -> u8 {
    let color = match *pixel {
        [v] => return v,
        [v, a] => Color::rgba(v, v, v, a),
        [r, g, b] => return luma(Color::rgb(r, g, b)),
        [r, g, b, a, ..] => Color::rgba(r, g, b, a),
        [] => return u8::MAX,
    };
    luma(color.over(backdrop))
}

/// Where the pixels that are read come from. Each pass of [`read`] opens
/// the source afresh and pulls its rows in order.
enum Source<'a> {
    /// A PNG, decoded a row at a time.
    Png {
        /// The file.
        bytes: &'a [u8],
        /// What transparent pixels are flattened onto.
        backdrop: Color,
        /// Width and height in pixels, from the header.
        side: u32,
    },
    /// A decoded image.
    Grey(GreyImage),
    /// The matrix, drawn a row at a time.
    Drawn(GreyRows<'a>),
}

impl Source<'_> {
    fn side(&self) -> u32 {
        match self {
            Self::Png { side, .. } => *side,
            Self::Grey(image) => image.side,
            Self::Drawn(rows) => rows.side(),
        }
    }

    /// Starts reading rows from the top.
    fn rows(&self) -> Result<Rows<'_>, VerifyError> {
        Ok(match self {
            Self::Png {
                bytes, backdrop, ..
            } => {
                let mut decoder = png::Decoder::new(Cursor::new(*bytes));
                decoder.set_transformations(png::Transformations::normalize_to_color8());
                let reader = decoder.read_info().map_err(|e| png_error(&e))?;
                Rows::Png {
                    reader: Box::new(reader),
                    backdrop: *backdrop,
                }
            }
            Self::Grey(image) => Rows::Grey(image),
            Self::Drawn(rows) => Rows::Drawn(rows),
        })
    }
}

fn png_error(e: &png::DecodingError) -> VerifyError {
    VerifyError::ReadBack {
        format: "png",
        message: e.to_string(),
    }
}

/// An open [`Source`], read top to bottom.
enum Rows<'s> {
    Png {
        // Boxed: the reader is large and only one is open at a time.
        reader: Box<png::Reader<Cursor<&'s [u8]>>>,
        backdrop: Color,
    },
    Grey(&'s GreyImage),
    Drawn(&'s GreyRows<'s>),
}

impl Rows<'_> {
    /// Replaces `out` with the luma of row `y`. Rows are asked for in
    /// order, each once.
    fn fill(&mut self, y: u32, out: &mut Vec<u8>) -> Result<(), VerifyError> {
        out.clear();
        match self {
            Self::Png { reader, backdrop } => {
                let channels = reader.output_color_type().0.samples();
                let row = reader
                    .next_row()
                    .map_err(|e| png_error(&e))?
                    .ok_or_else(|| VerifyError::ReadBack {
                        format: "png",
                        message: format!("the image ends before row {y}"),
                    })?;
                out.extend(
                    row.data()
                        .chunks_exact(channels.max(1))
                        .map(|pixel| luma_of(pixel, *backdrop)),
                );
            }
            Self::Grey(image) => {
                let side = image.side as usize;
                let start = y as usize * side;
                out.extend_from_slice(image.pixels.get(start..start + side).unwrap_or(&[]));
            }
            Self::Drawn(rows) => rows.fill_row(y, out),
        }
        Ok(())
    }
}

/// How the source is placed in the image handed to `rqrr`: enlarged to at
/// least [`MIN_READ_SCALE`] pixels per module, and with its quiet zone
/// widened to [`DEFAULT_MARGIN`] modules in the background colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    /// The side of the source, in pixels.
    source: usize,
    /// How many times each source pixel is repeated across and down.
    factor: usize,
    /// The pixels of quiet zone added on each side.
    pad: usize,
    /// The side of the image read, in pixels.
    side: usize,
    /// The luma of the added quiet zone.
    light: u8,
}

impl Layout {
    fn new(source: u32, matrix: &Matrix, options: &RenderOptions, backdrop: Color) -> Self {
        let source = source as usize;
        let margin = options.margin.min(crate::render::MAX_MARGIN);
        let modules = (matrix.width() + 2 * margin).max(1);
        let scale = (source / modules).max(1);
        let factor = if scale < MIN_READ_SCALE {
            MIN_READ_SCALE.div_ceil(scale)
        } else {
            1
        };
        let missing = DEFAULT_MARGIN.saturating_sub(margin);
        let pad = missing * (source * factor / modules);
        Self {
            source,
            factor,
            pad,
            side: source * factor + 2 * pad,
            light: luma(options.background().over(backdrop)),
        }
    }
}

/// Produces the pixels `rqrr` reads, in row-major order, from a source's
/// rows and a [`Layout`], inverted if asked.
struct Pixels<'s> {
    rows: Rows<'s>,
    layout: Layout,
    invert: bool,
    row: Vec<u8>,
    current: Option<usize>,
    failed: Option<VerifyError>,
}

impl<'s> Pixels<'s> {
    fn new(rows: Rows<'s>, layout: Layout, invert: bool) -> Self {
        Self {
            rows,
            layout,
            invert,
            row: Vec::new(),
            current: None,
            failed: None,
        }
    }

    /// The pixel at (`x`, `y`). Calls must not go back to an earlier row.
    fn pixel(&mut self, x: usize, y: usize) -> u8 {
        let Layout {
            source,
            factor,
            pad,
            light,
            ..
        } = self.layout;
        let inner = |v: usize| v.checked_sub(pad).filter(|v| *v < source * factor);
        let luma = match (inner(x), inner(y)) {
            (Some(x), Some(y)) => {
                let y = y / factor;
                if self.current != Some(y) {
                    self.current = Some(y);
                    let row = u32::try_from(y).unwrap_or(u32::MAX);
                    if let Err(e) = self.rows.fill(row, &mut self.row) {
                        self.failed.get_or_insert(e);
                    }
                }
                self.row.get(x / factor).copied().unwrap_or(light)
            }
            _ => light,
        };
        if self.invert { !luma } else { luma }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    /// A symbol holding the expected data.
    Match,
    /// Symbols were decoded, but none held the expected data; the length of
    /// the first one is kept for the error.
    Mismatch(usize),
    /// Nothing decoded.
    Nothing,
}

/// Reads `source` placed by `layout`, then its inverse if that does not
/// find the expected data.
///
/// The inverse is also tried after a mismatch: a light-on-dark image can
/// hold a stray dark-on-light pattern that decodes to something else.
fn read(
    source: &Source<'_>,
    layout: &Layout,
    expected: &[u8],
    format: &'static str,
    redrawn: bool,
) -> Result<Verified, VerifyError> {
    let side = u32::try_from(layout.side).unwrap_or(u32::MAX);
    let verified = |inverted| Verified {
        side,
        redrawn,
        inverted,
    };
    let normal = scan(source, layout, expected, false)?;
    if normal == Read::Match {
        return Ok(verified(false));
    }
    let inverse = scan(source, layout, expected, true)?;
    match (normal, inverse) {
        (_, Read::Match) => Ok(verified(true)),
        (Read::Mismatch(decoded), _) | (_, Read::Mismatch(decoded)) => Err(VerifyError::Mismatch {
            format,
            expected: expected.len(),
            decoded,
        }),
        _ => Err(VerifyError::NotFound { format }),
    }
}

/// Detects and decodes every symbol in `source` placed by `layout`
/// (inverted if asked).
fn scan(
    source: &Source<'_>,
    layout: &Layout,
    expected: &[u8],
    invert: bool,
) -> Result<Read, VerifyError> {
    let mut pixels = Pixels::new(source.rows()?, *layout, invert);
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(layout.side, layout.side, |x, y| {
            pixels.pixel(x, y)
        });
    if let Some(e) = pixels.failed {
        return Err(e);
    }
    let mut first = None;
    for grid in prepared.detect_grids() {
        let mut data = Vec::new();
        if grid.decode_to(&mut data).is_ok() {
            if data == expected {
                return Ok(Read::Match);
            }
            first.get_or_insert(data.len());
        }
    }
    Ok(first.map_or(Read::Nothing, Read::Mismatch))
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
    fn light_on_dark_codes_with_other_data_are_a_mismatch() {
        let m = code(b"something else");
        let options = RenderOptions::default()
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK);
        let err = verify_rendering(&m, &options, "svg", b"night mode").unwrap_err();
        assert_eq!(
            err,
            VerifyError::Mismatch {
                format: "svg",
                expected: 10,
                decoded: 14
            }
        );
    }

    /// `a` and `b` side by side on one row (both `side` pixels square).
    fn side_by_side(a: &GreyImage, b: &GreyImage) -> GreyImage {
        let side = a.side as usize;
        let mut pixels = Vec::new();
        for y in 0..2 * side {
            for x in 0..2 * side {
                let (image, x) = if x < side { (a, x) } else { (b, x - side) };
                pixels.push(image.pixels.get(y * side + x).copied().unwrap_or(255));
            }
        }
        GreyImage {
            side: a.side * 2,
            pixels,
        }
    }

    fn drawn(matrix: &Matrix, options: &RenderOptions) -> GreyImage {
        let rows = GreyRows::new(matrix, None, options, Color::WHITE).unwrap();
        let mut pixels = Vec::new();
        let mut row = Vec::new();
        for y in 0..rows.side() {
            rows.fill_row(y, &mut row);
            pixels.extend_from_slice(&row);
        }
        GreyImage {
            side: rows.side(),
            pixels,
        }
    }

    #[test]
    fn a_mismatch_still_tries_the_inverse() {
        // A dark-on-light code holding other data beside the light-on-dark
        // code that holds the expected data: the normal pass finds only the
        // first, so the inverse must still be tried.
        let (other, wanted) = (code(b"decoy data"), code(b"the real one"));
        assert_eq!(other.width(), wanted.width());
        let options = RenderOptions::default().with_scale(4);
        let dark = drawn(&other, &options);
        let light = drawn(
            &wanted,
            &options.clone().with_fg(Color::WHITE).with_bg(Color::BLACK),
        );
        let source = Source::Grey(side_by_side(&dark, &light));
        let layout = Layout {
            source: source.side() as usize,
            factor: 1,
            pad: 0,
            side: source.side() as usize,
            light: 255,
        };
        assert_eq!(
            scan(&source, &layout, b"the real one", false).unwrap(),
            Read::Mismatch(10)
        );
        let verified = read(&source, &layout, b"the real one", "png", false).unwrap();
        assert!(verified.inverted);
        // Neither pass finding the data reports the normal pass's symbol.
        let err = read(&source, &layout, b"neither", "png", false).unwrap_err();
        assert_eq!(
            err,
            VerifyError::Mismatch {
                format: "png",
                expected: 7,
                decoded: 10
            }
        );
    }

    #[test]
    fn decoded_pixels_are_flattened_to_luma_in_place() {
        let navy = Color::rgb(0, 0, 0x80);
        assert_eq!(luma_of(&[7], navy), 7);
        assert_eq!(luma_of(&[255, 0], navy), luma(navy));
        assert_eq!(luma_of(&[255, 0, 0], navy), 76);
        assert_eq!(luma_of(&[255, 255, 255, 0], navy), luma(navy));
        assert_eq!(luma_of(&[], navy), 255);

        let image = image::ImageBuffer::from_pixel(2, 2, image::Rgb([0u16, 0, 0]));
        let grey_image = grey(DynamicImage::ImageRgb16(image), Color::WHITE);
        assert_eq!(grey_image.side, 2);
        assert_eq!(grey_image.pixels, [0, 0, 0, 0]);
        let image = image::ImageBuffer::from_pixel(2, 2, image::LumaA([9u8, 255]));
        assert_eq!(
            grey(DynamicImage::ImageLumaA8(image), Color::WHITE).pixels,
            [9; 4]
        );
    }

    #[test]
    fn a_truncated_png_is_a_read_back_error() {
        let m = code(b"cut short");
        let options = RenderOptions::default().with_fg(Color::rgb(0x1a, 0x73, 0xe8));
        let png = render_raster(&m, RasterFormat::Png, &options).unwrap();
        let cut = &png[..png.len() / 2];
        let err = verify_raster(&m, RasterFormat::Png, cut, &options, b"cut short").unwrap_err();
        assert!(
            matches!(err, VerifyError::ReadBack { format: "png", .. }),
            "{err:?}"
        );
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
        let options = RenderOptions::default().with_margin(0);
        let layout = Layout::new(1, &one, &options, Color::WHITE);
        // Enlarged to 3 pixels per module, then 4 modules of quiet zone.
        assert_eq!(
            layout,
            Layout {
                source: 1,
                factor: 3,
                pad: 12,
                side: 27,
                light: 255
            }
        );
        let padded = materialise(&Source::Grey(image), &layout, false);
        assert_eq!(padded.len(), 27 * 27);
        let dark: Vec<usize> = (0..padded.len()).filter(|&i| padded[i] == 0).collect();
        assert_eq!(dark.len(), 9);
        assert!(padded.iter().all(|&p| p == 0 || p == 255));
        assert_eq!(padded[13 * 27 + 13], 0);
        assert_eq!(padded[12 * 27 + 12], 0);
        assert_eq!(padded[0], 255);
        let inverted = materialise(
            &Source::Grey(GreyImage {
                side: 1,
                pixels: vec![0],
            }),
            &layout,
            true,
        );
        assert_eq!(inverted[13 * 27 + 13], 255);
        assert_eq!(inverted[0], 0);
    }

    /// Every pixel `rqrr` would be handed, in order.
    fn materialise(source: &Source<'_>, layout: &Layout, invert: bool) -> Vec<u8> {
        let mut pixels = Pixels::new(source.rows().unwrap(), *layout, invert);
        let mut out = Vec::new();
        for y in 0..layout.side {
            for x in 0..layout.side {
                out.push(pixels.pixel(x, y));
            }
        }
        assert!(pixels.failed.is_none());
        out
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
        let layout = Layout {
            source: 2,
            factor: 2,
            pad: 0,
            side: 4,
            light: 255,
        };
        assert_eq!(
            materialise(&Source::Grey(image), &layout, false),
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
        // Redrawn at the vector verification scale, like vector output.
        assert_eq!(verified.side, 29 * VERIFY_SCALE);

        let vector = verify_rendering(&m, &options, "svg", b"big").unwrap();
        assert_eq!(vector.side, 29 * VERIFY_SCALE);
        assert_eq!(fitting_scale(&m, &options.with_margin(1_000)), 9);
    }

    #[test]
    fn large_colour_jpeg_and_webp_are_redrawn_at_their_own_scale() {
        let m = code(b"big colour");
        // 29 modules at 50 px: 1,450 px, 2.1 megapixels (under the pixel
        // budget), but 6.3 MB as decoded RGB (over the byte budget). They
        // are redrawn as vector output is, at VERIFY_SCALE.
        let options = RenderOptions::default()
            .with_scale(50)
            .with_fg(Color::rgb(0x1a, 0x73, 0xe8));
        for format in [RasterFormat::Jpeg, RasterFormat::Webp] {
            let bytes = render_raster(&m, format, &options).unwrap();
            let verified = verify_raster(&m, format, &bytes, &options, b"big colour").unwrap();
            assert!(verified.redrawn, "{format}");
            assert_eq!(verified.side, 29 * VERIFY_SCALE, "{format}");
        }
        // A PNG is read a row at a time, so it is decoded at this size.
        let png = render_raster(&m, RasterFormat::Png, &options).unwrap();
        let verified = verify_raster(&m, RasterFormat::Png, &png, &options, b"big colour").unwrap();
        assert!(!verified.redrawn);
        // So is a grey JPEG: one byte per pixel.
        let grey = options.with_fg(Color::BLACK).with_shape(ModuleShape::Dots);
        let jpeg = render_raster(&m, RasterFormat::Jpeg, &grey).unwrap();
        let verified = verify_raster(&m, RasterFormat::Jpeg, &jpeg, &grey, b"big colour").unwrap();
        assert!(!verified.redrawn);
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
