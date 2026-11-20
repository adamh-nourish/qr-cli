//! Raster renderers: PNG, JPEG and lossless WebP, drawn straight from the
//! module grid (no SVG rasteriser).
//!
//! Every format shares one checked geometry, the [`Canvas`](canvas::Canvas):
//! whole pixels per module (from [`RenderOptions::scale`], or the largest
//! that fits [`RenderOptions::size`]), at most
//! [`MAX_CANVAS_SIDE`](crate::render::MAX_CANVAS_SIDE) pixels per side and
//! [`MAX_PIXELS`] in total, with checked arithmetic throughout
//! (DECISIONS section 4.2). Each encoder then asks the canvas for rows,
//! which it describes as runs of light and dark pixels.

mod canvas;
mod grey;
mod jpeg;
mod logo;
mod paint;
mod png;
mod webp;

use std::fmt;
use std::str::FromStr;

use self::canvas::Canvas;
pub(crate) use self::grey::{GreyImage, GreyRows, luma};
pub use self::jpeg::render_jpeg;
use self::logo::LogoLayer;
pub use self::png::render_png;
pub use self::webp::render_webp;
use super::options::{RenderError, RenderOptions};
use crate::color::Color;
use crate::logo::Planned;
use crate::matrix::Matrix;

/// The most pixels a raster image may hold: 256 megapixels, a 16,000-pixel
/// square (DECISIONS section 4.2).
pub const MAX_PIXELS: u64 = 256_000_000;

/// The JPEG quality setting (DECISIONS section 0, item 12).
pub const JPEG_QUALITY: u8 = 92;

/// The smallest JPEG module size, in pixels. Below it, compression
/// artefacts reach across whole modules.
pub const JPEG_MIN_SCALE: u32 = 4;

/// The raster output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RasterFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG (baseline, quality [`JPEG_QUALITY`]).
    Jpeg,
    /// Lossless WebP.
    Webp,
}

impl RasterFormat {
    /// All raster formats.
    pub const ALL: [Self; 3] = [Self::Png, Self::Jpeg, Self::Webp];

    /// The usual file extension, without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    /// The IANA media type, for `--data-uri` and similar.
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    /// Whether the format records [`RenderOptions::dpi`].
    pub const fn records_dpi(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg)
    }

    /// Whether the format has an alpha channel, so a transparent or
    /// translucent colour survives.
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Png | Self::Webp)
    }
}

impl fmt::Display for RasterFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.extension())
    }
}

/// The name was not a raster format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown raster format '{0}' (expected png, jpeg or webp)")]
pub struct UnknownRasterFormat(pub String);

impl FromStr for RasterFormat {
    type Err = UnknownRasterFormat;

    /// Parses `png`, `jpeg` (or `jpg`) or `webp`, ignoring ASCII case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("jpg") {
            return Ok(Self::Jpeg);
        }
        Self::ALL
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnknownRasterFormat(s.chars().take(32).collect()))
    }
}

/// Renders `matrix` in `format` and returns the file contents.
///
/// # Errors
///
/// Returns [`RenderError`] if the scale is zero, a `size` is smaller than
/// the module count, the image would exceed
/// [`MAX_CANVAS_SIDE`](crate::render::MAX_CANVAS_SIDE) pixels per side or
/// [`MAX_PIXELS`] in total, `dpi` is out of range, a JPEG module would be
/// under [`JPEG_MIN_SCALE`] pixels, or the encoder fails.
///
/// ```
/// use qr_core::{EncodeOptions, RasterFormat, RenderOptions, encode, render_raster};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let options = RenderOptions::default().with_size(Some(300));
/// for format in RasterFormat::ALL {
///     let bytes = render_raster(code.matrix(), format, &options)?;
///     assert!(!bytes.is_empty());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_raster(
    matrix: &Matrix,
    format: RasterFormat,
    options: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    match format {
        RasterFormat::Png => render_png(matrix, options),
        RasterFormat::Jpeg => render_jpeg(matrix, options),
        RasterFormat::Webp => render_webp(matrix, options),
    }
}

/// A canvas and, with a logo, the layer painted over it.
pub(crate) struct Picture<'m> {
    pub(crate) canvas: Canvas<'m>,
    pub(crate) layer: Option<LogoLayer>,
}

impl<'m> Picture<'m> {
    /// The picture of `matrix` (or of `plan`'s cleared matrix, with the
    /// logo over it) whose light pixels are `light`.
    ///
    /// # Errors
    ///
    /// Any [`RenderError`] from [`Canvas::new`] or [`LogoLayer::new`].
    pub(crate) fn new(
        matrix: &'m Matrix,
        plan: Option<&'m Planned<'_>>,
        options: &RenderOptions,
        light: Color,
        format: &'static str,
        min_scale: u32,
    ) -> Result<Self, RenderError> {
        let Some(plan) = plan else {
            let canvas = Canvas::new(matrix, options, format, min_scale)?;
            return Ok(Self {
                canvas,
                layer: None,
            });
        };
        let canvas = Canvas::new(&plan.matrix, options, format, min_scale)?;
        let layer = LogoLayer::new(plan, &canvas, light, format)?;
        Ok(Self {
            canvas: canvas.with_fresh_rows(layer.rows()),
            layer: Some(layer),
        })
    }

    /// Replaces `out` with row `y` as colours: the modules in `light` and
    /// `dark`, then the logo over them.
    pub(crate) fn fill_colors(&self, y: u32, light: Color, dark: Color, out: &mut Vec<Color>) {
        self.canvas.fill_row(y, light, dark, out);
        if let Some(layer) = &self.layer {
            layer.paint(y, out, |c| c);
        }
    }
}

#[cfg(test)]
mod tests;
