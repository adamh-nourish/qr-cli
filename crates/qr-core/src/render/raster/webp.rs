//! Lossless WebP renderer, through the `image` crate's VP8L encoder.

use image::ExtendedColorType;
use image::codecs::webp::WebPEncoder;

use super::Picture;
use super::paint::Paint;
use crate::color::Color;
use crate::logo::Planned;
use crate::matrix::Matrix;
use crate::render::options::{RenderError, RenderOptions};

const FORMAT: &str = "webp";

/// Renders `matrix` as a lossless WebP image.
///
/// The pixel layout is the narrowest that holds the colours: grey, grey
/// with alpha, RGB or RGBA (with a logo, RGB or RGBA). The encoder needs the whole image in memory,
/// so the buffer is reserved fallibly and an allocation failure is an
/// error rather than an abort. WebP has no density field, so `dpi` is not
/// recorded.
///
/// # Errors
///
/// Returns [`RenderError`] if the options break a size limit (see
/// [`render_raster`](super::render_raster)), the buffer cannot be
/// allocated, or the encoder fails.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_webp};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let webp = render_webp(code.matrix(), &RenderOptions::default())?;
/// assert!(webp.starts_with(b"RIFF") && &webp[8..12] == b"WEBP");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_webp(matrix: &Matrix, options: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    let paint = Paint::new(options);
    let plan = Planned::new(matrix, options)?;
    let picture = Picture::new(matrix, plan.as_ref(), options, paint.light, FORMAT, 1)?;
    let canvas = &picture.canvas;
    let side = canvas.side();
    let (layout, light, dark) = match &picture.layer {
        None => pixel_layout(paint),
        Some(layer) if paint.is_opaque() && layer.is_opaque() => {
            (ExtendedColorType::Rgb8, rgb(paint.light), rgb(paint.dark))
        }
        Some(_) => (
            ExtendedColorType::Rgba8,
            rgba(paint.light),
            rgba(paint.dark),
        ),
    };

    let len = u64::from(side) * u64::from(side) * light.len() as u64;
    let mut pixels = Vec::new();
    usize::try_from(len)
        .ok()
        .and_then(|len| pixels.try_reserve_exact(len).ok())
        .ok_or_else(|| error(format!("cannot allocate {len} bytes for the image")))?;
    if picture.layer.is_some() {
        let channels = light.len();
        canvas.for_each_row(
            |y, row: &mut Vec<Color>| picture.fill_colors(y, paint.light, paint.dark, row),
            |row| {
                for c in row {
                    pixels.extend_from_slice(&[c.r, c.g, c.b, c.a][..channels]);
                }
                Ok::<(), RenderError>(())
            },
        )?;
    } else {
        canvas.for_each_row(
            |y, row: &mut Vec<&[u8]>| canvas.fill_row(y, light.as_slice(), dark.as_slice(), row),
            |row| {
                for px in row {
                    pixels.extend_from_slice(px);
                }
                Ok::<(), RenderError>(())
            },
        )?;
    }

    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out)
        .encode(&pixels, side, side, layout)
        .map_err(|e| error(e.to_string()))?;
    Ok(out)
}

/// The narrowest pixel layout for `paint` and the light and dark pixels in it.
fn pixel_layout(paint: Paint) -> (ExtendedColorType, Vec<u8>, Vec<u8>) {
    let (l, d) = (paint.light, paint.dark);
    match (paint.is_grey(), paint.is_opaque()) {
        (true, true) => (ExtendedColorType::L8, vec![l.r], vec![d.r]),
        (true, false) => (ExtendedColorType::La8, vec![l.r, l.a], vec![d.r, d.a]),
        (false, true) => (ExtendedColorType::Rgb8, rgb(l), rgb(d)),
        (false, false) => (ExtendedColorType::Rgba8, rgba(l), rgba(d)),
    }
}

fn rgb(c: Color) -> Vec<u8> {
    vec![c.r, c.g, c.b]
}

fn rgba(c: Color) -> Vec<u8> {
    vec![c.r, c.g, c.b, c.a]
}

fn error(message: String) -> RenderError {
    RenderError::Encoder {
        format: FORMAT,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    fn decode(bytes: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
            .unwrap()
            .to_rgba8()
    }

    #[test]
    fn pixels_are_exact_in_every_layout() {
        let cases = [
            (Color::BLACK, Color::WHITE, false, ExtendedColorType::L8),
            (Color::BLACK, Color::WHITE, true, ExtendedColorType::La8),
            (
                Color::rgb(0x1a, 0x73, 0xe8),
                Color::WHITE,
                false,
                ExtendedColorType::Rgb8,
            ),
            (
                Color::rgba(200, 0, 0, 128),
                Color::WHITE,
                true,
                ExtendedColorType::Rgba8,
            ),
        ];
        for (fg, bg, transparent, expected) in cases {
            let options = RenderOptions::default()
                .with_margin(0)
                .with_scale(2)
                .with_fg(fg)
                .with_bg(bg)
                .with_transparent(transparent);
            let paint = Paint::new(&options);
            assert_eq!(pixel_layout(paint).0, expected);
            let img = decode(&render_webp(&two_by_two(), &options).unwrap());
            assert_eq!(img.dimensions(), (4, 4));
            let px = |c: Color| image::Rgba([c.r, c.g, c.b, c.a]);
            assert_eq!(*img.get_pixel(0, 0), px(paint.dark), "{expected:?}");
            assert_eq!(*img.get_pixel(0, 3), px(paint.light), "{expected:?}");
            assert_eq!(*img.get_pixel(3, 3), px(paint.dark), "{expected:?}");
        }
    }

    #[test]
    fn errors_name_the_format() {
        assert_eq!(error("boom".into()).to_string(), "cannot encode webp: boom");
        let zero = RenderOptions::default().with_scale(0);
        assert_eq!(
            render_webp(&two_by_two(), &zero),
            Err(RenderError::ZeroScale)
        );
    }
}
