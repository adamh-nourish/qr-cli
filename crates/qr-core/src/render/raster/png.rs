//! PNG renderer: a 1-bit indexed image for two opaque colours, otherwise
//! 8-bit RGBA, streamed row by row through the `png` crate.

use std::io::Write as _;

use png::{BitDepth, ColorType, Compression, Encoder, Filter, PixelDimensions, Unit};

use super::canvas::Canvas;
use super::paint::Paint;
use crate::color::Color;
use crate::matrix::Matrix;
use crate::render::options::{RenderError, RenderOptions};

const FORMAT: &str = "png";

/// Renders `matrix` as a PNG image.
///
/// Two opaque colours give a 1-bit image with a two-entry palette (light is
/// index 0, dark index 1), the smallest file and the least encoding work. A
/// transparent or translucent colour gives 8-bit RGBA. `dpi`, when set, is
/// written as a `pHYs` chunk in pixels per metre.
///
/// # Errors
///
/// Returns [`RenderError`] if the options break a size limit (see
/// [`render_raster`](super::render_raster)) or the encoder fails.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_png};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let png = render_png(code.matrix(), &RenderOptions::default())?;
/// assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_png(matrix: &Matrix, options: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    let canvas = Canvas::new(matrix, options, FORMAT, 1)?;
    let paint = Paint::new(options);
    let side = canvas.side();

    let mut out = Vec::new();
    {
        let mut encoder = Encoder::new(&mut out, side, side);
        // zlib's default level: on a 290 px code the file is 424 bytes, not
        // 1,574 with `Fast` (fdeflate), for about 0.1 ms more work.
        encoder.set_compression(Compression::Balanced);
        if let Some(dpi) = options.dpi {
            let ppm = pixels_per_metre(dpi);
            encoder.set_pixel_dims(Some(PixelDimensions {
                xppu: ppm,
                yppu: ppm,
                unit: Unit::Meter,
            }));
        }
        let indexed = paint.is_opaque();
        if indexed {
            encoder.set_color(ColorType::Indexed);
            encoder.set_depth(BitDepth::One);
            // Row filters work on bytes, which mix eight 1-bit pixels, so they
            // only make the data less regular: unfiltered rows compress best
            // (424 vs 635 bytes with adaptive filtering on the same code).
            encoder.set_filter(Filter::NoFilter);
            encoder.set_palette(palette(paint).to_vec());
        } else {
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
        }

        let mut writer = encoder.write_header().map_err(|e| encode_error(&e))?;
        let mut stream = writer.stream_writer().map_err(|e| encode_error(&e))?;
        let (light, dark) = (rgba(paint.light), rgba(paint.dark));
        canvas.for_each_row(
            |y, row: &mut Vec<u8>| {
                if indexed {
                    canvas.fill_bits(y, row);
                } else {
                    let mut pixels = Vec::new();
                    canvas.fill_row(y, light, dark, &mut pixels);
                    row.clear();
                    row.extend_from_slice(pixels.as_flattened());
                }
            },
            |row| stream.write_all(row).map_err(|e| encode_error(&e.into())),
        )?;
        stream.finish().map_err(|e| encode_error(&e))?;
        writer.finish().map_err(|e| encode_error(&e))?;
    }
    Ok(out)
}

/// The palette for a 1-bit image: light at index 0, dark at index 1.
fn palette(paint: Paint) -> [u8; 6] {
    let (l, d) = (paint.light, paint.dark);
    [l.r, l.g, l.b, d.r, d.g, d.b]
}

/// The RGBA bytes of `color`.
fn rgba(color: Color) -> [u8; 4] {
    [color.r, color.g, color.b, color.a]
}

/// `dpi` in pixels per metre, rounded, as `pHYs` records it.
fn pixels_per_metre(dpi: u32) -> u32 {
    let ppm = (u64::from(dpi) * 10_000 + 127) / 254;
    // At most MAX_DPI / 0.0254, about 2.6 million.
    u32::try_from(ppm).unwrap_or(u32::MAX)
}

fn encode_error(error: &png::EncodingError) -> RenderError {
    RenderError::Encoder {
        format: FORMAT,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>, png::Info<'static>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes.to_vec()))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        let header = reader.info().clone();
        (info, buf, header)
    }

    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    #[test]
    fn opaque_colours_give_a_one_bit_palette_image() {
        let options = RenderOptions::default()
            .with_margin(1)
            .with_scale(2)
            .with_fg(Color::rgb(0x1a, 0x73, 0xe8));
        let png = render_png(&two_by_two(), &options).unwrap();
        let (info, pixels, header) = decode(&png);
        assert_eq!((info.width, info.height), (8, 8));
        assert_eq!(info.color_type, ColorType::Indexed);
        assert_eq!(info.bit_depth, BitDepth::One);
        assert_eq!(
            header.palette.as_deref(),
            Some(&[255, 255, 255, 0x1a, 0x73, 0xe8][..])
        );
        assert!(header.pixel_dims.is_none());
        // One byte per row; rows 2-3 hold the top module row.
        assert_eq!(
            pixels,
            [
                0,
                0,
                0b0011_1100,
                0b0011_1100,
                0b0000_1100,
                0b0000_1100,
                0,
                0
            ]
        );
    }

    #[test]
    fn transparency_gives_rgba() {
        let options = RenderOptions::default()
            .with_margin(0)
            .with_scale(1)
            .with_fg(Color::rgba(255, 0, 0, 128))
            .with_transparent(true);
        let png = render_png(&two_by_two(), &options).unwrap();
        let (info, pixels, _) = decode(&png);
        assert_eq!(info.color_type, ColorType::Rgba);
        assert_eq!(info.bit_depth, BitDepth::Eight);
        let red = [255, 0, 0, 128];
        let clear = [255, 255, 255, 0];
        assert_eq!(pixels, [red, red, clear, red].concat());
    }

    #[test]
    fn dpi_is_written_as_phys() {
        let options = RenderOptions::default().with_dpi(Some(300));
        let png = render_png(&two_by_two(), &options).unwrap();
        let dims = decode(&png).2.pixel_dims.unwrap();
        assert_eq!(
            (dims.xppu, dims.yppu, dims.unit),
            (11_811, 11_811, Unit::Meter)
        );
        assert_eq!(pixels_per_metre(72), 2_835);
        assert_eq!(pixels_per_metre(96), 3_780);
        assert_eq!(pixels_per_metre(65_535), 2_580_118);
    }

    #[test]
    fn limits_come_from_the_canvas() {
        let zero = RenderOptions::default().with_scale(0);
        assert_eq!(
            render_png(&two_by_two(), &zero),
            Err(RenderError::ZeroScale)
        );
        let dpi = RenderOptions::default().with_dpi(Some(0));
        assert!(matches!(
            render_png(&two_by_two(), &dpi),
            Err(RenderError::DpiOutOfRange { .. })
        ));
    }

    #[test]
    fn encoder_errors_name_the_format() {
        let err = encode_error(&png::EncodingError::LimitsExceeded);
        assert!(
            matches!(err, RenderError::Encoder { format: "png", .. }),
            "{err}"
        );
    }
}
