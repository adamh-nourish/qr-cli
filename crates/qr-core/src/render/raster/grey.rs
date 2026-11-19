//! An 8-bit greyscale bitmap of the canvas, for reading a code back with a
//! decoder (see [`crate::verify`]) without encoding and decoding a file.

use super::canvas::Canvas;
use super::paint::Paint;
use crate::color::Color;
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

/// Draws `matrix` with `options` (shapes and colours included) flattened
/// onto the opaque `backdrop`, as a scanner would see it printed.
///
/// # Errors
///
/// Returns [`RenderError`] if the options break a size limit or the buffer
/// cannot be allocated.
pub(crate) fn greyscale(
    matrix: &Matrix,
    options: &RenderOptions,
    backdrop: Color,
) -> Result<GreyImage, RenderError> {
    let canvas = Canvas::new(matrix, options, "verify", 1)?;
    let paint = Paint::new(options).flattened(backdrop);
    let side = canvas.side();
    let len = u64::from(side) * u64::from(side);
    let mut pixels = Vec::new();
    usize::try_from(len)
        .ok()
        .and_then(|len| pixels.try_reserve_exact(len).ok())
        .ok_or_else(|| RenderError::Encoder {
            format: "verify",
            message: format!("cannot allocate {len} bytes for the image"),
        })?;
    let (light, dark) = (luma(paint.light), luma(paint.dark));
    canvas.for_each_row(
        |y, row: &mut Vec<u8>| canvas.fill_row(y, light, dark, row),
        |row| {
            pixels.extend_from_slice(row);
            Ok::<(), RenderError>(())
        },
    )?;
    Ok(GreyImage { side, pixels })
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
