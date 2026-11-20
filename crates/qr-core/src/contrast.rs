//! Colour contrast, as WCAG 2 defines it, for the styling safety rails.
//!
//! Scanners threshold an image into dark and light, so a code needs a clear
//! difference in brightness between its modules and its background, and
//! most expect the modules to be the darker of the two.

use crate::color::Color;
use crate::render::RenderOptions;

/// The lowest contrast ratio that does not draw a warning (UX proposal:
/// "warn when fg/bg contrast is below 3:1").
pub const MIN_CONTRAST: f64 = 3.0;

impl Color {
    /// The WCAG 2 relative luminance of the colour's RGB channels, from 0
    /// (black) to 1 (white). Alpha is ignored; see [`Color::over`].
    ///
    /// ```
    /// use qr_core::Color;
    /// assert_eq!(Color::BLACK.relative_luminance(), 0.0);
    /// assert_eq!(Color::WHITE.relative_luminance(), 1.0);
    /// ```
    pub fn relative_luminance(self) -> f64 {
        let linear = |channel: u8| {
            let c = f64::from(channel) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// The WCAG 2 contrast ratio between two colours, from 1 (identical
    /// luminance) to 21 (black and white). The order does not matter.
    ///
    /// ```
    /// use qr_core::Color;
    /// assert_eq!(Color::BLACK.contrast_ratio(Color::WHITE), 21.0);
    /// assert_eq!(Color::WHITE.contrast_ratio(Color::WHITE), 1.0);
    /// ```
    pub fn contrast_ratio(self, other: Self) -> f64 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        let (light, dark) = if a >= b { (a, b) } else { (b, a) };
        (light + 0.05) / (dark + 0.05)
    }
}

/// How the dark and light modules of a rendering compare.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contrast {
    /// The colour dark modules appear in.
    pub dark: Color,
    /// The colour light modules and the quiet zone appear in.
    pub light: Color,
    /// The WCAG 2 contrast ratio between them.
    pub ratio: f64,
    /// Whether the "dark" modules are lighter than the background, which
    /// many scanners cannot read.
    pub inverted: bool,
}

impl Contrast {
    /// Whether the ratio is below [`MIN_CONTRAST`].
    pub fn is_low(&self) -> bool {
        self.ratio < MIN_CONTRAST
    }
}

impl RenderOptions {
    /// The contrast of the colours as they appear on white paper: the
    /// background over white, and the foreground over that. A transparent
    /// background keeps [`RenderOptions::bg`] as the intended backdrop.
    ///
    /// ```
    /// use qr_core::{Color, RenderOptions};
    /// let grey = RenderOptions::default().with_fg(Color::rgb(0xaa, 0xaa, 0xaa));
    /// let contrast = grey.contrast();
    /// assert!(contrast.is_low() && !contrast.inverted);
    /// let inverted = RenderOptions::default()
    ///     .with_fg(Color::WHITE)
    ///     .with_bg(Color::BLACK)
    ///     .contrast();
    /// assert!(inverted.inverted && !inverted.is_low());
    /// ```
    pub fn contrast(&self) -> Contrast {
        let light = self.bg.over(Color::WHITE);
        let dark = self.fg.over(light);
        Contrast {
            dark,
            light,
            ratio: dark.contrast_ratio(light),
            inverted: dark.relative_luminance() > light.relative_luminance(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn known_ratios() {
        // Reference values from WebAIM's contrast checker.
        let navy = Color::rgb(0, 0, 0x80);
        assert!(close(navy.contrast_ratio(Color::WHITE), 16.0));
        let grey = Color::rgb(0x77, 0x77, 0x77);
        assert!(close(grey.contrast_ratio(Color::WHITE), 4.48));
        let red = Color::rgb(255, 0, 0);
        assert!(close(red.contrast_ratio(Color::WHITE), 4.0));
        assert!(close(
            Color::rgb(0x1a, 0x73, 0xe8).relative_luminance(),
            0.183
        ));
        assert_eq!(grey.contrast_ratio(red), red.contrast_ratio(grey));
    }

    #[test]
    fn contrast_of_render_options() {
        let default = RenderOptions::default().contrast();
        assert_eq!((default.dark, default.light), (Color::BLACK, Color::WHITE));
        assert!(close(default.ratio, 21.0) && !default.inverted && !default.is_low());

        let yellow = RenderOptions::default()
            .with_fg(Color::rgb(255, 255, 0))
            .contrast();
        assert!(yellow.is_low() && !yellow.inverted);

        // Translucent colours are judged as they appear.
        let faint = RenderOptions::default()
            .with_fg(Color::rgba(0, 0, 0, 40))
            .contrast();
        assert!(faint.is_low(), "{faint:?}");

        // A transparent background keeps the chosen colour as the backdrop.
        let dark_page = RenderOptions::default()
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK)
            .with_transparent(true)
            .contrast();
        assert!(dark_page.inverted && !dark_page.is_low());

        let same = RenderOptions::default()
            .with_fg(Color::rgb(9, 9, 9))
            .with_bg(Color::rgb(9, 9, 9))
            .contrast();
        assert!(close(same.ratio, 1.0) && !same.inverted);
    }
}
