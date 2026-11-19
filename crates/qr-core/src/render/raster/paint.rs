//! The two colours a raster image is painted with.

use crate::color::Color;
use crate::render::options::RenderOptions;

/// The resolved colours of light and dark pixels.
///
/// Dark pixels show the foreground painted over the background, as the
/// vector renderers draw it, so a translucent foreground blends with the
/// background colour rather than with whatever is behind the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Paint {
    /// Colour of light modules and the quiet zone.
    pub(crate) light: Color,
    /// Colour of dark modules.
    pub(crate) dark: Color,
}

impl Paint {
    /// The paint for `options`.
    pub(crate) fn new(options: &RenderOptions) -> Self {
        let light = options.background();
        Self {
            light,
            dark: composite(options.fg, light),
        }
    }

    /// Whether both colours are opaque, so no alpha channel is needed.
    pub(crate) fn is_opaque(self) -> bool {
        self.light.is_opaque() && self.dark.is_opaque()
    }

    /// Whether both colours are shades of grey, so one channel can hold them.
    pub(crate) fn is_grey(self) -> bool {
        let grey = |c: Color| c.r == c.g && c.g == c.b;
        grey(self.light) && grey(self.dark)
    }

    /// Both colours painted over an opaque `backdrop`, for formats with no
    /// alpha channel.
    pub(crate) fn flattened(self, backdrop: Color) -> Self {
        Self {
            light: self.light.over(backdrop),
            dark: self.dark.over(backdrop),
        }
    }
}

/// `top` painted over `bottom` (Porter-Duff source-over) with straight,
/// 8-bit alpha. Unlike [`Color::over`], the backdrop may be translucent.
fn composite(top: Color, bottom: Color) -> Color {
    if top.is_opaque() || bottom.is_invisible() {
        return top;
    }
    if top.is_invisible() {
        return bottom;
    }
    let top_a = u32::from(top.a);
    let rest = u32::from(bottom.a) * (255 - top_a);
    // Output alpha, scaled by 255. Both alphas are non-zero, so it is too.
    let out_a = top_a * 255 + rest;
    let channel = |t: u8, b: u8| {
        let mixed = (u32::from(t) * top_a * 255 + u32::from(b) * rest + out_a / 2) / out_a;
        // A weighted mean of two u8 values, so it fits.
        u8::try_from(mixed).unwrap_or(u8::MAX)
    };
    Color::rgba(
        channel(top.r, bottom.r),
        channel(top.g, bottom.g),
        channel(top.b, bottom.b),
        u8::try_from((out_a + 127) / 255).unwrap_or(u8::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paint_is_opaque_grey() {
        let paint = Paint::new(&RenderOptions::default());
        assert_eq!(paint.light, Color::WHITE);
        assert_eq!(paint.dark, Color::BLACK);
        assert!(paint.is_opaque() && paint.is_grey());
    }

    #[test]
    fn transparent_background_keeps_the_foreground() {
        let options = RenderOptions::default()
            .with_fg(Color::rgb(0x1a, 0x73, 0xe8))
            .with_transparent(true);
        let paint = Paint::new(&options);
        assert_eq!(paint.light, Color::rgba(255, 255, 255, 0));
        assert_eq!(paint.dark, Color::rgb(0x1a, 0x73, 0xe8));
        assert!(!paint.is_opaque() && !paint.is_grey());
        let flat = paint.flattened(Color::WHITE);
        assert_eq!(flat.light, Color::WHITE);
        assert_eq!(flat.dark, Color::rgb(0x1a, 0x73, 0xe8));
        assert!(flat.is_opaque());
    }

    #[test]
    fn translucent_foreground_blends_with_the_background() {
        let options = RenderOptions::default().with_fg(Color::rgba(0, 0, 0, 128));
        assert_eq!(Paint::new(&options).dark, Color::rgb(127, 127, 127));
        let invisible = RenderOptions::default().with_fg(Color::TRANSPARENT);
        assert_eq!(Paint::new(&invisible).dark, Color::WHITE);
    }

    #[test]
    fn composite_handles_two_translucent_layers() {
        // 50% red over 50% blue: alpha 0.75, red weighted 2:1 over blue.
        let c = composite(Color::rgba(255, 0, 0, 128), Color::rgba(0, 0, 255, 128));
        assert_eq!(c, Color::rgba(170, 0, 85, 192));
        // Agrees with `Color::over` when the backdrop is opaque.
        for a in [1, 64, 128, 200, 254] {
            let top = Color::rgba(10, 200, 30, a);
            let bottom = Color::rgb(250, 5, 120);
            assert_eq!(composite(top, bottom), top.over(bottom), "alpha {a}");
        }
    }
}
