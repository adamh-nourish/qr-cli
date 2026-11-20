//! A logo painted over raster rows.

use super::canvas::Canvas;
use super::paint::composite;
use crate::color::Color;
use crate::logo::{PixelRect, Planned, resample};
use crate::render::options::RenderError;

/// A logo resampled to its place on one canvas and painted over the light
/// colour, ready to be copied into rows.
#[derive(Debug, Clone)]
pub(crate) struct LogoLayer {
    rect: PixelRect,
    /// `rect.width * rect.height` colours, row-major.
    pixels: Vec<Color>,
}

impl LogoLayer {
    /// The layer for `plan` on `canvas`, whose cleared modules show
    /// `light`. Translucent logo pixels are blended with `light`, as they
    /// would be over the cleared background.
    ///
    /// # Errors
    ///
    /// [`RenderError::Encoder`] if the resampled logo cannot be allocated,
    /// [`RenderError::TooLarge`] if its position overflows.
    pub(crate) fn new(
        plan: &Planned<'_>,
        canvas: &Canvas<'_>,
        light: Color,
        format: &'static str,
    ) -> Result<Self, RenderError> {
        let rect = plan
            .geometry
            .pixels(canvas.scale(), canvas.origin())
            .filter(|r| {
                r.x.checked_add(r.width)
                    .is_some_and(|end| end <= canvas.side())
            })
            .filter(|r| {
                r.y.checked_add(r.height)
                    .is_some_and(|end| end <= canvas.side())
            })
            .ok_or(RenderError::TooLarge {
                format,
                side: u64::from(canvas.side()),
                max: canvas.side(),
            })?;
        let mut pixels = resample(plan.logo.image(), rect.width, rect.height).ok_or_else(|| {
            RenderError::Encoder {
                format,
                message: format!("cannot allocate the {}x{} logo", rect.width, rect.height),
            }
        })?;
        for px in &mut pixels {
            *px = composite(*px, light);
        }
        Ok(Self { rect, pixels })
    }

    /// The pixel rows the logo covers, `[start, end)`.
    pub(crate) fn rows(&self) -> (u32, u32) {
        (self.rect.y, self.rect.y + self.rect.height)
    }

    /// Whether every pixel of the painted logo is opaque.
    pub(crate) fn is_opaque(&self) -> bool {
        self.pixels.iter().all(|c| c.is_opaque())
    }

    /// Paints the logo's part of pixel row `y` into `row`, converting each
    /// colour with `map`. Rows the logo does not cross are left alone.
    pub(crate) fn paint<T>(&self, y: u32, row: &mut [T], map: impl Fn(Color) -> T) {
        if !self.rect.covers_row(y) {
            return;
        }
        let width = self.rect.width as usize;
        let start = (y - self.rect.y) as usize * width;
        let Some(source) = self.pixels.get(start..start + width) else {
            return;
        };
        let x = self.rect.x as usize;
        if let Some(target) = row.get_mut(x..x + width) {
            for (out, color) in target.iter_mut().zip(source) {
                *out = map(*color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::Bitmap;
    use crate::logo::Logo;
    use crate::render::options::RenderOptions;
    use crate::{EncodeOptions, encode};

    #[test]
    fn paints_the_logo_into_its_rows() {
        let code = encode(b"layer", &EncodeOptions::default()).unwrap();
        let red = Bitmap::from_rgba(1, 1, vec![255, 0, 0, 128]).unwrap();
        let options = RenderOptions::default()
            .with_scale(2)
            .with_logo(Some(Logo::new(red)));
        let plan = Planned::new(code.matrix(), &options).unwrap().unwrap();
        let canvas = Canvas::new(&plan.matrix, &options, "png", 1).unwrap();
        let layer = LogoLayer::new(&plan, &canvas, Color::WHITE, "png").unwrap();
        // Half-transparent red over white.
        assert!(layer.is_opaque());
        let (start, end) = layer.rows();
        assert!(start < end && end <= canvas.side());

        let mut row = vec![Color::BLACK; canvas.side() as usize];
        layer.paint(start, &mut row, |c| c);
        let painted: Vec<usize> = (0..row.len()).filter(|i| row[*i] != Color::BLACK).collect();
        assert_eq!(painted.len(), layer.rect.width as usize);
        assert_eq!(row[painted[0]], Color::rgb(255, 127, 127));
        // Centred: as much room on the left as on the right (within a pixel).
        let left = painted[0];
        let right = row.len() - 1 - painted[painted.len() - 1];
        assert!(left.abs_diff(right) <= 1, "{left} {right}");

        let mut untouched = vec![0_u8; canvas.side() as usize];
        layer.paint(end, &mut untouched, |_| 1);
        layer.paint(start, &mut untouched[..3], |_| 1);
        assert!(untouched.iter().all(|v| *v == 0));

        let clear = LogoLayer::new(&plan, &canvas, Color::TRANSPARENT, "png").unwrap();
        assert!(!clear.is_opaque());
    }
}
