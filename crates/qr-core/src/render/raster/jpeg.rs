//! JPEG renderer: baseline JPEG at quality [`JPEG_QUALITY`] through
//! `jpeg-encoder`, fed one pixel row at a time.

use jpeg_encoder::{Density, Encoder, ImageBuffer, JpegColorType, SamplingFactor};

use super::paint::Paint;
use super::{JPEG_MIN_SCALE, JPEG_QUALITY, Picture};
use crate::color::Color;
use crate::logo::Planned;
use crate::matrix::Matrix;
use crate::render::options::{RenderError, RenderOptions};

const FORMAT: &str = "jpeg";

/// Renders `matrix` as a JPEG image.
///
/// JPEG has no alpha channel, so both colours are flattened onto white; a
/// transparent background becomes white. Grey colours give a one-channel
/// (luma) image. Colour images keep full-resolution chroma (4:4:4), so
/// module edges stay sharp. `dpi`, when set, is written as the JFIF density.
///
/// # Errors
///
/// Returns [`RenderError::ScaleTooSmall`] below [`JPEG_MIN_SCALE`] pixels
/// per module, where compression artefacts start to blur modules, any other
/// size-limit error (see [`render_raster`](super::render_raster)), or an
/// encoder error.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_jpeg};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let jpeg = render_jpeg(code.matrix(), &RenderOptions::default())?;
/// assert!(jpeg.starts_with(&[0xff, 0xd8]));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_jpeg(matrix: &Matrix, options: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    let paint = Paint::new(options).flattened(Color::WHITE);
    let plan = Planned::new(matrix, options)?;
    let picture = Picture::new(
        matrix,
        plan.as_ref(),
        options,
        paint.light,
        FORMAT,
        JPEG_MIN_SCALE,
    )?;
    let canvas = &picture.canvas;
    // The canvas is at most MAX_CANVAS_SIDE (16,384) pixels across.
    let side = u16::try_from(canvas.side()).map_err(|_| RenderError::TooLarge {
        format: FORMAT,
        side: u64::from(canvas.side()),
        max: u32::from(u16::MAX),
    })?;

    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, JPEG_QUALITY);
    encoder.set_sampling_factor(SamplingFactor::F_1_1);
    if let Some(dpi) = options.dpi {
        // `Canvas::new` checked dpi against MAX_DPI, which is u16::MAX.
        let dpi = u16::try_from(dpi).unwrap_or(u16::MAX);
        encoder.set_density(Density::Inch { x: dpi, y: dpi });
    }
    let image = Rows {
        picture: &picture,
        side,
        // A logo is drawn in colour, whatever the modules' colours.
        luma: paint.is_grey() && picture.layer.is_none(),
        paint,
        light: ycbcr(paint.light),
        dark: ycbcr(paint.dark),
    };
    encoder
        .encode_image(image)
        .map_err(|error| RenderError::Encoder {
            format: FORMAT,
            message: error.to_string(),
        })?;
    Ok(out)
}

/// The canvas as a `jpeg-encoder` image source. The encoder asks for rows
/// as it compresses each band of blocks, so the full image is never held
/// in memory.
struct Rows<'p, 'm> {
    picture: &'p Picture<'m>,
    side: u16,
    luma: bool,
    paint: Paint,
    light: [u8; 3],
    dark: [u8; 3],
}

impl ImageBuffer for Rows<'_, '_> {
    fn get_jpeg_color_type(&self) -> JpegColorType {
        if self.luma {
            JpegColorType::Luma
        } else {
            JpegColorType::Ycbcr
        }
    }

    fn width(&self) -> u16 {
        self.side
    }

    fn height(&self) -> u16 {
        self.side
    }

    fn fill_buffers(&self, y: u16, buffers: &mut [Vec<u8>; 4]) {
        let channels = if self.luma { 1 } else { 3 };
        let y = u32::from(y);
        let in_logo = self.picture.layer.as_ref().is_some_and(|layer| {
            let (start, end) = layer.rows();
            (start..end).contains(&y)
        });
        if in_logo {
            let mut colors = Vec::new();
            self.picture
                .fill_colors(y, self.paint.light, self.paint.dark, &mut colors);
            // The light colour is already flattened onto white and the logo
            // painted over it, so the colours are opaque; `over` is a no-op
            // kept for safety.
            for color in colors {
                let value = ycbcr(color.over(Color::WHITE));
                for (buffer, v) in buffers.iter_mut().zip(value).take(channels) {
                    buffer.push(v);
                }
            }
            return;
        }
        self.picture.canvas.row_runs(y, |dark, len| {
            let color = if dark { self.dark } else { self.light };
            for (buffer, value) in buffers.iter_mut().zip(color).take(channels) {
                buffer.extend(std::iter::repeat_n(value, len as usize));
            }
        });
    }
}

/// `color` (opaque) in the JFIF YCbCr space. For a grey, Y is the grey
/// level and Cb and Cr are 128.
fn ycbcr(color: Color) -> [u8; 3] {
    let (y, cb, cr) = jpeg_encoder::rgb_to_ycbcr(color.r, color.g, color.b);
    [y, cb, cr]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).unwrap()
    }

    #[test]
    fn grey_paint_gives_a_luma_image() {
        let jpeg = render_jpeg(&two_by_two(), &RenderOptions::default()).unwrap();
        let img = decode(&jpeg);
        assert_eq!((img.width(), img.height()), (100, 100));
        assert_eq!(img.color(), image::ColorType::L8);
        let luma = img.to_luma8();
        assert!(luma.get_pixel(45, 45).0[0] < 20, "dark module");
        assert!(luma.get_pixel(45, 55).0[0] > 235, "light module");
        assert!(luma.get_pixel(5, 5).0[0] > 235, "quiet zone");
    }

    #[test]
    fn colour_and_transparency_are_flattened_onto_white() {
        let options = RenderOptions::default()
            .with_fg(Color::rgb(200, 0, 0))
            .with_bg(Color::rgb(0, 0, 255))
            .with_transparent(true);
        let img = decode(&render_jpeg(&two_by_two(), &options).unwrap());
        assert_eq!(img.color(), image::ColorType::Rgb8);
        let rgb = img.to_rgb8();
        let [r, g, b] = rgb.get_pixel(45, 45).0;
        assert!(r > 180 && g < 30 && b < 30, "{r} {g} {b}");
        let [r, g, b] = rgb.get_pixel(5, 5).0;
        assert!(r > 240 && g > 240 && b > 240, "{r} {g} {b}");
    }

    #[test]
    fn dpi_is_written_as_jfif_density() {
        let options = RenderOptions::default().with_dpi(Some(300));
        let jpeg = render_jpeg(&two_by_two(), &options).unwrap();
        // APP0 "JFIF\0", version 1.x, units 1 (inch), then x and y density.
        let jfif = jpeg.windows(5).position(|w| w == b"JFIF\0").unwrap();
        assert_eq!(&jpeg[jfif + 7..jfif + 12], &[1, 1, 44, 1, 44]);
        let plain = render_jpeg(&two_by_two(), &RenderOptions::default()).unwrap();
        let jfif = plain.windows(5).position(|w| w == b"JFIF\0").unwrap();
        assert_eq!(plain[jfif + 7], 0, "no units without --dpi");
    }

    #[test]
    fn small_modules_are_refused() {
        let options = RenderOptions::default().with_scale(3);
        assert_eq!(
            render_jpeg(&two_by_two(), &options),
            Err(RenderError::ScaleTooSmall {
                format: "jpeg",
                scale: 3,
                min: JPEG_MIN_SCALE
            })
        );
        assert!(render_jpeg(&two_by_two(), &options.with_scale(4)).is_ok());
    }
}
