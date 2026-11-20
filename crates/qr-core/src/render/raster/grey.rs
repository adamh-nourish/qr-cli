//! 8-bit greyscale views of the canvas and of decoded images, for reading a
//! code back with a decoder (see [`crate::verify`]) without encoding and
//! decoding a file.

use super::Picture;
use super::paint::Paint;
use crate::color::Color;
use crate::logo::Planned;
use crate::matrix::Matrix;
use crate::render::options::{RenderError, RenderOptions};

/// A square greyscale image, row-major, one byte per pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GreyImage {
    /// Width and height in pixels.
    pub(crate) side: u32,
    /// `side * side` luma values.
    pub(crate) pixels: Vec<u8>,
}

/// `matrix` drawn with `options` (shapes, colours and logo included)
/// flattened onto an opaque backdrop, as a scanner would see it printed,
/// produced a row at a time so that no full-size bitmap is held.
pub(crate) struct GreyRows<'m> {
    picture: Picture<'m>,
    light: u8,
    dark: u8,
}

impl<'m> GreyRows<'m> {
    /// The drawing of `matrix` with `options` on `backdrop`; with a logo,
    /// `plan` is its plan for `options` (see [`Planned::new`]).
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] if the options break a size limit.
    pub(crate) fn new(
        matrix: &'m Matrix,
        plan: Option<&'m Planned<'_>>,
        options: &RenderOptions,
        backdrop: Color,
    ) -> Result<Self, RenderError> {
        let paint = Paint::new(options).flattened(backdrop);
        let picture = Picture::new(matrix, plan, options, paint.light, "verify", 1)?;
        Ok(Self {
            picture,
            light: luma(paint.light),
            dark: luma(paint.dark),
        })
    }

    /// Width and height in pixels.
    pub(crate) fn side(&self) -> u32 {
        self.picture.canvas.side()
    }

    /// Replaces `out` with the luma values of row `y`.
    pub(crate) fn fill_row(&self, y: u32, out: &mut Vec<u8>) {
        self.picture.canvas.fill_row(y, self.light, self.dark, out);
        if let Some(layer) = &self.picture.layer {
            // Painted over the opaque light colour, so already opaque.
            layer.paint(y, out, luma);
        }
    }
}

/// The Rec. 601 luma of an opaque colour, as most decoders compute it.
pub(crate) fn luma(color: Color) -> u8 {
    let weighted = 299 * u32::from(color.r) + 587 * u32::from(color.g) + 114 * u32::from(color.b);
    // A weighted mean of u8 values (the weights add up to 1000), so it fits.
    u8::try_from((weighted + 500) / 1000).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::shape::ModuleShape;

    /// The whole drawing as one bitmap.
    fn greyscale(
        matrix: &Matrix,
        options: &RenderOptions,
        backdrop: Color,
    ) -> Result<GreyImage, RenderError> {
        let rows = GreyRows::new(matrix, None, options, backdrop)?;
        let side = rows.side();
        let mut pixels = Vec::new();
        let mut row = Vec::new();
        for y in 0..side {
            rows.fill_row(y, &mut row);
            pixels.extend_from_slice(&row);
        }
        Ok(GreyImage { side, pixels })
    }

    #[test]
    fn draws_the_canvas_in_grey() {
        let m = Matrix::from_modules(2, vec![true, true, false, true]).unwrap();
        let options = RenderOptions::default()
            .with_margin(1)
            .with_scale(1)
            .with_fg(Color::rgb(255, 0, 0));
        let image = greyscale(&m, &options, Color::WHITE).unwrap();
        assert_eq!(image.side, 4);
        let (w, r) = (255, 76);
        assert_eq!(
            image.pixels,
            [w, w, w, w, w, r, r, w, w, w, r, w, w, w, w, w]
        );
    }

    #[test]
    fn transparency_is_flattened_onto_the_backdrop() {
        let m = Matrix::from_modules(1, vec![true]).unwrap();
        let options = RenderOptions::default()
            .with_margin(1)
            .with_scale(1)
            .with_fg(Color::WHITE)
            .with_transparent(true);
        let image = greyscale(&m, &options, Color::BLACK).unwrap();
        assert_eq!(image.pixels, [0, 0, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn shapes_and_limits_come_from_the_canvas() {
        let m = Matrix::from_modules(1, vec![true]).unwrap();
        let dots = RenderOptions::default()
            .with_margin(0)
            .with_scale(10)
            .with_shape(ModuleShape::Dots);
        let image = greyscale(&m, &dots, Color::WHITE).unwrap();
        assert_eq!(image.pixels[0], 255, "dot corners are light");
        assert_eq!(image.pixels[55], 0, "dot centre is dark");
        assert_eq!(
            greyscale(&m, &dots.with_scale(0), Color::WHITE),
            Err(RenderError::ZeroScale)
        );
        assert_eq!(luma(Color::rgb(0, 0, 0x80)), 15);
    }
}
