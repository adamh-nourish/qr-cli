//! Output tests for the raster renderers.
//!
//! Every format is decoded back to pixels with the `image` crate, flattened
//! onto white, and read with `rqrr`'s full symbol detection, so each image
//! is checked end to end as a scanner would see it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use image::{GenericImageView as _, ImageFormat};
use proptest::prelude::*;
use qr_core::{
    Color, EcLevel, EncodeOptions, RasterFormat, RenderError, RenderOptions, encode, render_raster,
};

fn image_format(format: RasterFormat) -> ImageFormat {
    match format {
        RasterFormat::Png => ImageFormat::Png,
        RasterFormat::Jpeg => ImageFormat::Jpeg,
        RasterFormat::Webp => ImageFormat::WebP,
    }
}

/// Decodes `bytes` and returns the image size and the greyscale pixels,
/// with any transparency flattened onto white.
fn greyscale(bytes: &[u8], format: RasterFormat) -> (u32, u32, Vec<u8>) {
    let img = image::load_from_memory_with_format(bytes, image_format(format)).unwrap();
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8();
    let luma = rgba
        .pixels()
        .map(|p| {
            let [red, green, blue, alpha] = p.0.map(u32::from);
            let luma = (299 * red + 587 * green + 114 * blue) / 1000;
            u8::try_from((luma * alpha + 255 * (255 - alpha)) / 255).unwrap()
        })
        .collect();
    (w, h, luma)
}

/// Decodes the QR code in `bytes`.
fn scan(bytes: &[u8], format: RasterFormat) -> Vec<u8> {
    let (w, h, luma) = greyscale(bytes, format);
    let (w, h) = (w as usize, h as usize);
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| luma[y * w + x]);
    let grids = img.detect_grids();
    assert_eq!(
        grids.len(),
        1,
        "expected exactly one symbol in the {format}"
    );
    let mut data = Vec::new();
    grids[0]
        .decode_to(&mut data)
        .unwrap_or_else(|e| panic!("rqrr failed to decode the {format}: {e:?}"));
    data
}

fn round_trip(data: &[u8], ec: EcLevel, format: RasterFormat, options: &RenderOptions) {
    let code = encode(data, &EncodeOptions::default().with_ec_level(ec)).unwrap();
    let bytes = render_raster(code.matrix(), format, options).unwrap();
    assert_eq!(scan(&bytes, format), data, "{format} with {options:?}");
}

#[test]
fn every_format_round_trips_with_default_options() {
    let payloads: [&[u8]; 4] = [
        b"hello",
        b"https://example.com/a/longer/path?with=query&and=more",
        b"0123456789012345678901234567890123456789",
        "caf\u{e9} \u{2603} bytes".as_bytes(),
    ];
    for format in RasterFormat::ALL {
        for data in payloads {
            for ec in EcLevel::ALL {
                round_trip(data, ec, format, &RenderOptions::default());
            }
        }
    }
}

#[test]
fn every_format_round_trips_with_colours_and_sizes() {
    let styles = [
        RenderOptions::default().with_size(Some(333)),
        RenderOptions::default().with_scale(4).with_margin(2),
        RenderOptions::default()
            .with_fg(Color::rgb(0x1a, 0x23, 0x7e))
            .with_bg(Color::rgb(0xff, 0xf8, 0xe1)),
        RenderOptions::default()
            .with_fg(Color::rgb(0x0b, 0x50, 0x20))
            .with_transparent(true),
        RenderOptions::default().with_fg(Color::rgba(0, 0, 0, 200)),
        RenderOptions::default().with_dpi(Some(300)),
    ];
    for format in RasterFormat::ALL {
        for options in &styles {
            round_trip(b"https://example.com/styled", EcLevel::M, format, options);
        }
    }
}

#[test]
fn large_symbols_round_trip() {
    let data = vec![b'x'; 1_200];
    for format in RasterFormat::ALL {
        round_trip(
            &data,
            EcLevel::L,
            format,
            &RenderOptions::default().with_scale(4),
        );
    }
}

#[test]
fn size_is_exact_and_scale_is_the_module_size() {
    let code = encode(b"hello", &EncodeOptions::default()).unwrap();
    for format in RasterFormat::ALL {
        let options = RenderOptions::default().with_size(Some(500));
        let bytes = render_raster(code.matrix(), format, &options).unwrap();
        let (w, h, _) = greyscale(&bytes, format);
        assert_eq!((w, h), (500, 500), "{format}");

        let options = RenderOptions::default().with_scale(7).with_margin(1);
        let bytes = render_raster(code.matrix(), format, &options).unwrap();
        let (w, _, _) = greyscale(&bytes, format);
        assert_eq!(w, 23 * 7, "{format}");
    }
}

#[test]
fn bounds_are_errors_not_allocations() {
    let code = encode(
        &[b'x'; 2_000],
        &EncodeOptions::default().with_ec_level(EcLevel::L),
    )
    .unwrap();
    // A large symbol at a scale that passes the side check would be 268 MP.
    let width = u32::try_from(code.matrix().width() + 8).unwrap();
    let options = RenderOptions::default().with_scale(16_384 / width);
    for format in RasterFormat::ALL {
        let err = render_raster(code.matrix(), format, &options).unwrap_err();
        assert!(
            matches!(err, RenderError::TooManyPixels { .. }),
            "{format}: {err}"
        );
        let err =
            render_raster(code.matrix(), format, &options.clone().with_scale(16_384)).unwrap_err();
        assert!(
            matches!(err, RenderError::TooLarge { .. }),
            "{format}: {err}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn random_payloads_round_trip(
        data in proptest::collection::vec(any::<u8>(), 1..200),
        format in prop::sample::select(RasterFormat::ALL.to_vec()),
        scale in 4_u32..8,
    ) {
        let code = encode(&data, &EncodeOptions::default()).unwrap();
        let options = RenderOptions::default().with_scale(scale);
        let bytes = render_raster(code.matrix(), format, &options).unwrap();
        prop_assert_eq!(scan(&bytes, format), data);
    }
}
