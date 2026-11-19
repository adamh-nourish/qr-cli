//! Options and errors shared by the graphical (vector and, later, raster)
//! renderers.

use thiserror::Error;

use crate::color::Color;
use crate::matrix::Matrix;
use crate::render::text::{DEFAULT_MARGIN, MAX_MARGIN};

/// The default size of one module: pixels for SVG, points for PDF and EPS.
pub const DEFAULT_SCALE: u32 = 10;

/// The largest canvas side, in pixels or points, any renderer produces
/// (DECISIONS §4.2). It bounds every size computation and the output size.
pub const MAX_CANVAS_SIDE: u32 = 16_384;

/// The longest document title accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 512;

/// A symbol could not be rendered with the given options.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderError {
    /// The scale was zero, which would draw nothing.
    #[error("scale must be at least 1")]
    ZeroScale,
    /// The canvas would exceed the format's size limit.
    #[error(
        "rendered size {side} exceeds the {max} limit for {format} (lower --scale or --margin)"
    )]
    TooLarge {
        /// The format being rendered, such as `svg`.
        format: &'static str,
        /// The requested canvas side, saturated at `u64::MAX`.
        side: u64,
        /// The largest side the format allows.
        max: u32,
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
    /// Size of one module: pixels for SVG, points for PDF and EPS.
    pub scale: u32,
    /// Colour of dark modules.
    pub fg: Color,
    /// Colour of light modules and the quiet zone.
    pub bg: Color,
    /// Leave the background unpainted, whatever the alpha of [`Self::bg`].
    pub transparent: bool,
    /// Optional document title, written as escaped metadata (SVG `<title>`,
    /// the PDF `/Title` entry and the EPS `%%Title` comment).
    pub title: Option<String>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            margin: DEFAULT_MARGIN,
            scale: DEFAULT_SCALE,
            fg: Color::BLACK,
            bg: Color::WHITE,
            transparent: false,
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
        if self.scale == 0 {
            return Err(RenderError::ZeroScale);
        }
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
        let side = u64::from(modules) * u64::from(self.scale);
        let side = u32::try_from(side)
            .ok()
            .filter(|s| *s <= max_side)
            .ok_or_else(|| too_large(side))?;
        Ok(Layout {
            margin,
            modules,
            scale: self.scale,
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
        assert_eq!(o.background(), Color::WHITE);
    }

    #[test]
    fn builders_set_every_field() {
        let o = RenderOptions::default()
            .with_margin(usize::MAX)
            .with_scale(3)
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK)
            .with_transparent(true)
            .with_title("t");
        assert_eq!(o.margin, MAX_MARGIN);
        assert_eq!(o.scale, 3);
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
            "rendered size 20000 exceeds the 16384 limit for svg (lower --scale or --margin)"
        );
    }
}
