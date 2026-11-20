//! Where a logo goes: computed once in module units (thousandths of a
//! module, exactly), then mapped onto each renderer's pixels.

use super::{KEEP_OUT, Logo};
use crate::matrix::Matrix;

/// Thousandths of a module per module.
const MILLI: u64 = 1_000;

/// The pad cleared around the logo, in thousandths of a module.
const PAD: u64 = MILLI / 2;

/// A rectangle of pixels: `x`, `y` is the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PixelRect {
    /// Whether pixel row `y` crosses the rectangle.
    pub(crate) fn covers_row(&self, y: u32) -> bool {
        y >= self.y && y - self.y < self.height
    }
}

/// The placement of a logo on a symbol `modules` wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Geometry {
    /// The symbol's width in modules.
    modules: u64,
    /// The logo's width and height, in thousandths of a module.
    width: u64,
    height: u64,
    /// The modules cleared, `[start, end)` across and down, in matrix
    /// coordinates (no quiet zone).
    cleared_x: (usize, usize),
    cleared_y: (usize, usize),
}

impl Geometry {
    /// Places `logo` on a symbol `modules` wide, or `None` if the symbol
    /// is too small to clear anything outside [`KEEP_OUT`].
    pub(crate) fn new(modules: usize, logo: &Logo) -> Option<Self> {
        let n = u64::try_from(modules).ok()?;
        let keep_out = KEEP_OUT as u64;
        // The widest box whose padded, whole-module clearing stays inside
        // the keep-out border: n - 2 * KEEP_OUT modules, less the pads.
        let room = n
            .checked_sub(2 * keep_out)?
            .checked_mul(MILLI)?
            .checked_sub(2 * PAD)?;
        if room == 0 {
            return None;
        }
        let side = n.checked_mul(u64::from(logo.scale().permille()))?.min(room);
        let (w, h) = (
            u64::from(logo.image().width()),
            u64::from(logo.image().height()),
        );
        // Fit the image's aspect ratio inside the square box, rounding.
        let (width, height) = if w >= h {
            (side, (side * h + w / 2) / w)
        } else {
            ((side * w + h / 2) / h, side)
        };
        let (width, height) = (width.max(1), height.max(1));
        let span = |extent: u64| -> Option<(usize, usize)> {
            // Doubled to keep the centre (n / 2 modules) exact.
            let twice_lo = (n * MILLI).checked_sub(extent + 2 * PAD)?;
            let twice_hi = n * MILLI + extent + 2 * PAD;
            let lo = (twice_lo / (2 * MILLI)).max(keep_out);
            let hi = twice_hi.div_ceil(2 * MILLI).min(n - keep_out);
            Some((usize::try_from(lo).ok()?, usize::try_from(hi).ok()?))
        };
        Some(Self {
            modules: n,
            width,
            height,
            cleared_x: span(width)?,
            cleared_y: span(height)?,
        })
    }

    /// `matrix` with the modules under the logo cleared to light.
    pub(crate) fn clear(&self, matrix: &Matrix) -> Matrix {
        matrix.with_light_rect(self.cleared_x, self.cleared_y)
    }

    /// The modules cleared across and down, `[start, end)`.
    #[cfg(test)]
    pub(crate) fn cleared(&self) -> ((usize, usize), (usize, usize)) {
        (self.cleared_x, self.cleared_y)
    }

    /// The logo's rectangle on a canvas with `scale` pixels per module
    /// whose symbol starts `origin` pixels from the top-left corner, or
    /// `None` on overflow. Each side is at least one pixel.
    pub(crate) fn pixels(&self, scale: u32, origin: u32) -> Option<PixelRect> {
        let scale = u64::from(scale);
        let size = |extent: u64| ((extent * scale + MILLI / 2) / MILLI).max(1);
        let (width, height) = (size(self.width), size(self.height));
        let symbol = self.modules.checked_mul(scale)?;
        let start = |extent: u64| u64::from(origin).checked_add(symbol.checked_sub(extent)? / 2);
        Some(PixelRect {
            x: u32::try_from(start(width)?).ok()?,
            y: u32::try_from(start(height)?).ok()?,
            width: u32::try_from(width).ok()?,
            height: u32::try_from(height).ok()?,
        })
    }

    /// The logo's rectangle in modules, from the top-left of a symbol
    /// with a quiet zone of `margin` modules: `(x, y, width, height)`.
    pub(crate) fn modules(&self, margin: u32) -> (f64, f64, f64, f64) {
        let milli = |v: u64| u32::try_from(v).map_or(f64::from(u32::MAX), f64::from) / 1_000.0;
        let n = milli(self.modules * MILLI);
        let (w, h) = (milli(self.width), milli(self.height));
        let m = f64::from(margin);
        (m + (n - w) / 2.0, m + (n - h) / 2.0, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::Bitmap;
    use crate::logo::LogoScale;

    fn logo(width: u32, height: u32, permille: u16) -> Logo {
        let image = Bitmap::from_rgba(width, height, vec![0; (width * height * 4) as usize]);
        Logo::new(image.unwrap()).with_scale(LogoScale::from_permille(permille).unwrap())
    }

    #[test]
    fn a_square_logo_is_centred_with_a_padded_hole() {
        // Version 5: 37 modules; 0.2 gives a 7.4-module box.
        let g = Geometry::new(37, &logo(10, 10, 200)).unwrap();
        assert_eq!((g.width, g.height), (7_400, 7_400));
        // 18.5 +- (3.7 + 0.5) = 14.3 .. 22.7, widened to whole modules.
        assert_eq!(g.cleared(), ((14, 23), (14, 23)));
        let px = g.pixels(10, 40).unwrap();
        assert_eq!(
            px,
            PixelRect {
                x: 40 + 148,
                y: 188,
                width: 74,
                height: 74
            }
        );
        assert!(px.covers_row(188) && px.covers_row(261));
        assert!(!px.covers_row(187) && !px.covers_row(262));
        let (left, top, width, height) = g.modules(4);
        assert!((left - 18.8).abs() < 1e-9 && (top - left).abs() < 1e-9);
        assert!((width - 7.4).abs() < 1e-9 && (height - 7.4).abs() < 1e-9);
    }

    #[test]
    fn the_aspect_ratio_is_kept_inside_the_box() {
        let wide = Geometry::new(37, &logo(200, 50, 300)).unwrap();
        assert_eq!((wide.width, wide.height), (11_100, 2_775));
        let ((x0, x1), (y0, y1)) = wide.cleared();
        assert!(x1 - x0 > y1 - y0);
        let tall = Geometry::new(37, &logo(50, 200, 300)).unwrap();
        assert_eq!((tall.width, tall.height), (2_775, 11_100));
        // A sliver is still at least a thousandth of a module, one pixel.
        let sliver = Geometry::new(37, &logo(8_000, 1, 50)).unwrap();
        assert_eq!(sliver.height, 1);
        assert_eq!(sliver.pixels(1, 0).unwrap().height, 1);
    }

    #[test]
    fn small_symbols_cap_the_box_and_keep_function_patterns() {
        // Version 1: 21 modules leaves modules 9..12 for the hole.
        let g = Geometry::new(21, &logo(1, 1, 300)).unwrap();
        assert_eq!((g.width, g.height), (2_000, 2_000));
        assert_eq!(g.cleared(), ((9, 12), (9, 12)));
        // Version 40: 177 modules, nowhere near the cap.
        let g = Geometry::new(177, &logo(1, 1, 300)).unwrap();
        assert_eq!(g.width, 53_100);
        let ((x0, x1), _) = g.cleared();
        assert!(x0 >= KEEP_OUT && x1 <= 177 - KEEP_OUT);
        // Too small for any hole.
        assert!(Geometry::new(19, &logo(1, 1, 300)).is_none());
        assert!(Geometry::new(2, &logo(1, 1, 300)).is_none());
    }

    #[test]
    fn clearing_only_touches_the_hole() {
        let full = Matrix::from_modules(21, vec![true; 21 * 21]).unwrap();
        let g = Geometry::new(21, &logo(1, 1, 200)).unwrap();
        let cleared = g.clear(&full);
        let ((x0, x1), (y0, y1)) = g.cleared();
        for y in 0..21 {
            for x in 0..21 {
                let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                assert_eq!(cleared.is_dark(x, y), !inside, "({x}, {y})");
            }
        }
    }

    #[test]
    fn pixel_overflow_is_none() {
        let g = Geometry::new(37, &logo(1, 1, 200)).unwrap();
        assert!(g.pixels(u32::MAX, u32::MAX).is_none());
    }
}
