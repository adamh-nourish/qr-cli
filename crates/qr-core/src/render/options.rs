//! Options and errors shared by the graphical (vector and raster) renderers.

use thiserror::Error;

use crate::color::Color;
use crate::matrix::Matrix;
use crate::render::shape::ModuleShape;
use crate::render::text::{DEFAULT_MARGIN, MAX_MARGIN};

/// The default size of one module: pixels for SVG, points for PDF and EPS.
pub const DEFAULT_SCALE: u32 = 10;

/// The largest canvas side, in pixels or points, any renderer produces
/// (DECISIONS §4.2). It bounds every size computation and the output size.
pub const MAX_CANVAS_SIDE: u32 = 16_384;

/// The longest document title accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 512;

/// The largest pixel density accepted, in dots per inch. JPEG stores the
/// density in 16 bits, so this is the most every format can record.
pub const MAX_DPI: u32 = 65_535;

/// A symbol could not be rendered with the given options.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderError {
    /// The scale was zero, which would draw nothing.
    #[error("scale must be at least 1")]
    ZeroScale,
    /// The canvas would exceed the format's size limit.
    #[error(
        "rendered size {side} exceeds the {max} limit for {format} (lower --scale, --size or --margin)"
    )]
    TooLarge {
        /// The format being rendered, such as `svg`.
        format: &'static str,
        /// The requested canvas side, saturated at `u64::MAX`.
        side: u64,
        /// The largest side the format allows.
        max: u32,
    },
    /// The requested canvas size cannot hold one pixel (or point) per module.
    #[error(
        "size {size} is too small for {modules} modules including the margin (use at least {modules})"
    )]
    SizeTooSmall {
        /// The requested canvas side.
        size: u32,
        /// The symbol plus quiet zone, in modules.
        modules: u32,
    },
    /// The module size is below a format's minimum, as for JPEG, whose
    /// compression blurs modules narrower than a few pixels.
    #[error(
        "{format} needs at least {min} pixels per module, got {scale} (raise --scale or --size)"
    )]
    ScaleTooSmall {
        /// The format being rendered, such as `jpeg`.
        format: &'static str,
        /// The module size the options produce.
        scale: u32,
        /// The smallest module size the format allows.
        min: u32,
    },
    /// The image would hold more pixels than any raster renderer draws.
    #[error("rendered image of {pixels} pixels exceeds the {max} pixel limit for {format}")]
    TooManyPixels {
        /// The format being rendered, such as `png`.
        format: &'static str,
        /// The requested pixel count.
        pixels: u64,
        /// The largest pixel count allowed.
        max: u64,
    },
    /// The pixel density is zero or above [`MAX_DPI`].
    #[error("dpi {dpi} is out of range (expected 1 to {max})")]
    DpiOutOfRange {
        /// The requested density.
        dpi: u32,
        /// [`MAX_DPI`].
        max: u32,
    },
    /// An image encoder failed. With valid options this is a bug or an
    /// allocation failure, not a usage error.
    #[error("cannot encode {format}: {message}")]
    Encoder {
        /// The format being encoded.
        format: &'static str,
        /// The encoder's own description of the failure.
        message: String,
    },
    /// The document title is longer than [`MAX_TITLE_CHARS`].
    #[error("title is {chars} characters long (the limit is {max})")]
    TitleTooLong {
        /// The number of characters in the title.
        chars: usize,
        /// [`MAX_TITLE_CHARS`].
        max: usize,
    },
}

/// Options for the graphical renderers.
///
/// ```
/// use qr_core::{Color, RenderOptions};
/// let options = RenderOptions::default()
///     .with_margin(2)
///     .with_scale(4)
///     .with_fg(Color::rgb(0x1a, 0x73, 0xe8))
///     .with_transparent(true);
/// assert_eq!(options.background(), Color::rgba(255, 255, 255, 0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RenderOptions {
    /// Quiet zone around the symbol, in modules. Renderers clamp it to
    /// [`MAX_MARGIN`].
    pub margin: usize,
    /// Size of one module: pixels for SVG and raster images, points for PDF
    /// and EPS. Ignored when [`Self::size`] is set.
    pub scale: u32,
    /// Target canvas side instead of a module size. Raster images are
    /// exactly this many pixels across: the largest whole module size that
    /// fits is used and the remainder widens the quiet zone. Vector formats
    /// use that same module size, so they are at most this size.
    pub size: Option<u32>,
    /// Pixel density recorded in raster metadata (PNG `pHYs`, JPEG JFIF
    /// density). It does not change the pixels.
    pub dpi: Option<u32>,
    /// Colour of dark modules.
    pub fg: Color,
    /// Colour of light modules and the quiet zone.
    pub bg: Color,
    /// Leave the background unpainted, whatever the alpha of [`Self::bg`].
    pub transparent: bool,
    /// How dark modules are drawn. The finder patterns stay square.
    pub shape: ModuleShape,
    /// Optional document title, written as escaped metadata (SVG `<title>`,
    /// the PDF `/Title` entry and the EPS `%%Title` comment).
    pub title: Option<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            margin: DEFAULT_MARGIN,
            scale: DEFAULT_SCALE,
            size: None,
            dpi: None,
            fg: Color::BLACK,
            bg: Color::WHITE,
            transparent: false,
            shape: ModuleShape::Square,
            title: None,
        }
    }
}

impl RenderOptions {
    /// Returns these options with the given quiet zone, clamped to
    /// [`MAX_MARGIN`].
    #[must_use]
    pub fn with_margin(mut self, margin: usize) -> Self {
        self.margin = margin.min(MAX_MARGIN);
        self
    }

    /// Returns these options with the given module size.
    #[must_use]
    pub fn with_scale(mut self, scale: u32) -> Self {
        self.scale = scale;
        self
    }

    /// Returns these options with a target canvas side (see [`Self::size`]),
    /// or none to use [`Self::scale`].
    #[must_use]
    pub fn with_size(mut self, size: Option<u32>) -> Self {
        self.size = size;
        self
    }

    /// Returns these options with the given pixel density, or none.
    #[must_use]
    pub fn with_dpi(mut self, dpi: Option<u32>) -> Self {
        self.dpi = dpi;
        self
    }

    /// Returns these options with the given dark-module colour.
    #[must_use]
    pub fn with_fg(mut self, fg: Color) -> Self {
        self.fg = fg;
        self
    }

    /// Returns these options with the given light-module colour.
    #[must_use]
    pub fn with_bg(mut self, bg: Color) -> Self {
        self.bg = bg;
        self
    }

    /// Returns these options with the background left unpainted (or not).
    #[must_use]
    pub fn with_transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Returns these options with the given module shape.
    #[must_use]
    pub fn with_shape(mut self, shape: ModuleShape) -> Self {
        self.shape = shape;
        self
    }

    /// Returns these options with the given document title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The background colour to paint: [`Self::bg`], with alpha 0 when
    /// [`Self::transparent`] is set.
    pub fn background(&self) -> Color {
        if self.transparent {
            self.bg.with_alpha(0)
        } else {
            self.bg
        }
    }

    /// The margin the renderers use, clamped because the field is public.
    pub(crate) fn effective_margin(&self) -> usize {
        self.margin.min(MAX_MARGIN)
    }

    /// Checks the options against `matrix` and a format's size limit and
    /// returns the layout to draw.
    pub(crate) fn layout(
        &self,
        matrix: &Matrix,
        format: &'static str,
        max_side: u32,
    ) -> Result<Layout, RenderError> {
        if let Some(title) = &self.title {
            let chars = title.chars().count();
            if chars > MAX_TITLE_CHARS {
                return Err(RenderError::TitleTooLong {
                    chars,
                    max: MAX_TITLE_CHARS,
                });
            }
        }
        let too_large = |side: u64| RenderError::TooLarge {
            format,
            side,
            max: max_side,
        };
        let margin = u32::try_from(self.effective_margin()).map_err(|_| too_large(u64::MAX))?;
        let modules = u32::try_from(matrix.width())
            .ok()
            .and_then(|w| margin.checked_mul(2)?.checked_add(w))
            .ok_or_else(|| too_large(u64::MAX))?;
        let scale = match self.size {
            // Whole pixels per module, so every module is the same size.
            Some(size) => match size / modules {
                0 => return Err(RenderError::SizeTooSmall { size, modules }),
                scale => scale,
            },
            None if self.scale == 0 => return Err(RenderError::ZeroScale),
            None => self.scale,
        };
        let side = u64::from(modules) * u64::from(scale);
        let side = u32::try_from(side)
            .ok()
            .filter(|s| *s <= max_side)
            .ok_or_else(|| too_large(side))?;
        Ok(Layout {
            margin,
            modules,
            scale,
            side,
        })
    }
}

/// The checked geometry of one rendering. Every value fits in `u32` and
/// `side = modules * scale <= max_side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    /// Quiet zone, in modules.
    pub(crate) margin: u32,
    /// Symbol plus quiet zone, in modules.
    pub(crate) modules: u32,
    /// Size of one module.
    pub(crate) scale: u32,
    /// Canvas side: `modules * scale`.
    pub(crate) side: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_module() -> Matrix {
        Matrix::from_modules(1, vec![true]).unwrap()
    }

    #[test]
    fn defaults_match_the_cli_defaults() {
        let o = RenderOptions::default();
        assert_eq!((o.margin, o.scale), (4, 10));
        assert_eq!((o.fg, o.bg), (Color::BLACK, Color::WHITE));
        assert!(!o.transparent && o.title.is_none());
        assert_eq!(o.shape, ModuleShape::Square);
        assert_eq!((o.size, o.dpi), (None, None));
        assert_eq!(o.background(), Color::WHITE);
    }

    #[test]
    fn builders_set_every_field() {
        let o = RenderOptions::default()
            .with_margin(usize::MAX)
            .with_scale(3)
            .with_size(Some(7))
            .with_dpi(Some(300))
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK)
            .with_transparent(true)
            .with_shape(ModuleShape::Dots)
            .with_title("t");
        assert_eq!(o.shape, ModuleShape::Dots);
        assert_eq!(o.margin, MAX_MARGIN);
        assert_eq!((o.scale, o.size, o.dpi), (3, Some(7), Some(300)));
        assert_eq!((o.fg, o.bg), (Color::WHITE, Color::BLACK));
        assert_eq!(o.background(), Color::TRANSPARENT);
        assert_eq!(o.title.as_deref(), Some("t"));
    }

    #[test]
    fn layout_is_checked() {
        let m = one_module();
        let layout = RenderOptions::default()
            .with_margin(2)
            .with_scale(3)
            .layout(&m, "svg", 100)
            .unwrap();
        assert_eq!(
            layout,
            Layout {
                margin: 2,
                modules: 5,
                scale: 3,
                side: 15
            }
        );

        let zero = RenderOptions::default()
            .with_scale(0)
            .layout(&m, "svg", 100);
        assert_eq!(zero, Err(RenderError::ZeroScale));

        let big = RenderOptions::default()
            .with_scale(u32::MAX)
            .layout(&m, "pdf", 100);
        assert_eq!(
            big,
            Err(RenderError::TooLarge {
                format: "pdf",
                side: 9 * u64::from(u32::MAX),
                max: 100
            })
        );
        let just_over = RenderOptions::default()
            .with_scale(12)
            .layout(&m, "eps", 100);
        assert!(matches!(
            just_over,
            Err(RenderError::TooLarge { side: 108, .. })
        ));
        assert!(
            RenderOptions::default()
                .with_scale(11)
                .layout(&m, "eps", 99)
                .is_ok()
        );
    }

    #[test]
    fn size_picks_the_largest_whole_scale() {
        let m = one_module(); // 9 modules with the default margin
        let layout = |size| {
            RenderOptions::default()
                .with_size(Some(size))
                .layout(&m, "svg", 100)
        };
        assert_eq!(layout(9).map(|l| (l.scale, l.side)), Ok((1, 9)));
        assert_eq!(layout(26).map(|l| (l.scale, l.side)), Ok((2, 18)));
        assert_eq!(layout(27).map(|l| (l.scale, l.side)), Ok((3, 27)));
        assert_eq!(
            layout(8),
            Err(RenderError::SizeTooSmall {
                size: 8,
                modules: 9
            })
        );
        assert_eq!(
            layout(0),
            Err(RenderError::SizeTooSmall {
                size: 0,
                modules: 9
            })
        );
        // The size overrides even a zero scale.
        let o = RenderOptions::default().with_scale(0).with_size(Some(18));
        assert_eq!(o.layout(&m, "svg", 100).map(|l| l.scale), Ok(2));
        assert!(matches!(
            layout(200),
            Err(RenderError::TooLarge { side: 198, .. })
        ));
    }

    #[test]
    fn public_fields_are_clamped_too() {
        let o = RenderOptions {
            margin: usize::MAX,
            ..RenderOptions::default()
        };
        let layout = o.with_scale(1).layout(&one_module(), "svg", u32::MAX);
        assert_eq!(
            layout.map(|l| l.margin),
            Ok(u32::try_from(MAX_MARGIN).unwrap())
        );
    }

    #[test]
    fn titles_are_bounded() {
        let ok = RenderOptions::default().with_title("é".repeat(MAX_TITLE_CHARS));
        assert!(ok.layout(&one_module(), "svg", MAX_CANVAS_SIDE).is_ok());
        let long = RenderOptions::default().with_title("a".repeat(MAX_TITLE_CHARS + 1));
        let err = long
            .layout(&one_module(), "svg", MAX_CANVAS_SIDE)
            .unwrap_err();
        assert_eq!(
            err,
            RenderError::TitleTooLong {
                chars: MAX_TITLE_CHARS + 1,
                max: MAX_TITLE_CHARS
            }
        );
        assert_eq!(
            err.to_string(),
            "title is 513 characters long (the limit is 512)"
        );
        assert_eq!(
            RenderError::ZeroScale.to_string(),
            "scale must be at least 1"
        );
        assert_eq!(
            RenderError::TooLarge {
                format: "svg",
                side: 20_000,
                max: 16_384
            }
            .to_string(),
            "rendered size 20000 exceeds the 16384 limit for svg (lower --scale, --size or --margin)"
        );
        assert_eq!(
            RenderError::SizeTooSmall {
                size: 20,
                modules: 29
            }
            .to_string(),
            "size 20 is too small for 29 modules including the margin (use at least 29)"
        );
        assert_eq!(
            RenderError::ScaleTooSmall {
                format: "jpeg",
                scale: 2,
                min: 4
            }
            .to_string(),
            "jpeg needs at least 4 pixels per module, got 2 (raise --scale or --size)"
        );
        assert_eq!(
            RenderError::TooManyPixels {
                format: "png",
                pixels: 300,
                max: 200
            }
            .to_string(),
            "rendered image of 300 pixels exceeds the 200 pixel limit for png"
        );
        assert_eq!(
            RenderError::DpiOutOfRange { dpi: 0, max: 9 }.to_string(),
            "dpi 0 is out of range (expected 1 to 9)"
        );
        assert_eq!(
            RenderError::Encoder {
                format: "webp",
                message: "boom".into()
            }
            .to_string(),
            "cannot encode webp: boom"
        );
    }
}
