//! Logos: every format that takes one draws it, keeps the symbol readable
//! and agrees with the drawing that verification reads; the others refuse
//! it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Test geometry on small symbols: the casts cannot truncate.
#![allow(clippy::cast_possible_truncation)]

use qr_core::loader::{Bitmap, decode_image};
use qr_core::logo::{Logo, LogoScale};
use qr_core::verify::{verify_raster, verify_rendering};
use qr_core::{
    Color, EcLevel, EncodeOptions, ModuleShape, RasterFormat, RenderError, RenderOptions,
    VectorFormat, encode, render_raster, render_svg, render_vector,
};

/// A `side` x `side` logo: an opaque red disc on a transparent square.
fn disc(side: u32) -> Bitmap {
    let c = f64::from(side) / 2.0;
    let mut pixels = Vec::new();
    for y in 0..side {
        for x in 0..side {
            let (dx, dy) = (f64::from(x) + 0.5 - c, f64::from(y) + 0.5 - c);
            let inside = dx.hypot(dy) <= c;
            pixels.extend_from_slice(&if inside {
                [220, 20, 20, 255]
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    Bitmap::from_rgba(side, side, pixels).unwrap()
}

/// A solid `width` x `height` logo.
fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Bitmap {
    Bitmap::from_rgba(width, height, rgba.repeat((width * height) as usize)).unwrap()
}

const DATA: &[u8] = b"https://example.com/menu?table=12";

fn code() -> qr_core::QrCode {
    encode(DATA, &EncodeOptions::default().with_ec_level(EcLevel::H)).unwrap()
}

fn with_logo(image: Bitmap, scale: u16) -> RenderOptions {
    let logo = Logo::new(image).with_scale(LogoScale::from_permille(scale).unwrap());
    RenderOptions::default().with_logo(Some(logo))
}

#[test]
fn every_raster_format_draws_the_logo_and_reads_back() {
    let code = code();
    for format in RasterFormat::ALL {
        for shape in ModuleShape::ALL {
            for (image, scale) in [(disc(64), 300), (solid(30, 10, [0, 0, 0, 255]), 200)] {
                let options = with_logo(image, scale).with_shape(shape);
                let bytes = render_raster(code.matrix(), format, &options).unwrap();
                let verified =
                    verify_raster(code.matrix(), format, &bytes, &options, DATA).unwrap();
                assert!(!verified.redrawn, "{format} {shape:?}");
                // The drawing verification would use agrees with the file.
                verify_rendering(code.matrix(), &options, "svg", DATA).unwrap();
            }
        }
    }
}

#[test]
fn the_logo_is_painted_at_the_centre_over_cleared_modules() {
    let code = code();
    let options = with_logo(disc(40), 300).with_scale(4);
    let png = render_raster(code.matrix(), RasterFormat::Png, &options).unwrap();
    let bitmap = decode_image(&png).unwrap();
    let side = bitmap.width();
    let at = |x: u32, y: u32| {
        let i = ((y * side + x) * 4) as usize;
        <[u8; 4]>::try_from(&bitmap.pixels()[i..i + 4]).unwrap()
    };
    // RGB, not the 1-bit palette image a plain code gets.
    assert!(png[25] == 2, "colour type {}", png[25]);
    assert_eq!(at(side / 2, side / 2), [220, 20, 20, 255]);
    // The disc's transparent corners show the cleared background.
    let width = code.matrix().width() as u32;
    let box_px = width * 4 * 3 / 10;
    let corner = side / 2 - box_px / 2 + 1;
    assert_eq!(at(corner, corner), [255, 255, 255, 255]);
    // The finder patterns are untouched.
    assert_eq!(at(16, 16), [0, 0, 0, 255]);

    // Without a logo the same code is a 1-bit palette image.
    let plain = render_raster(code.matrix(), RasterFormat::Png, &RenderOptions::default()).unwrap();
    assert_eq!(plain[25], 3);
}

#[test]
fn translucency_keeps_an_alpha_channel_where_the_format_has_one() {
    let code = code();
    let ghost = with_logo(solid(4, 4, [0, 0, 255, 100]), 200).with_transparent(true);
    let png = render_raster(code.matrix(), RasterFormat::Png, &ghost).unwrap();
    assert_eq!(png[25], 6, "RGBA");
    let centre = decode_image(&png).unwrap();
    let side = centre.width();
    let i = ((side / 2 * side + side / 2) * 4) as usize;
    assert_eq!(&centre.pixels()[i..i + 4], [0, 0, 255, 100]);
    verify_raster(code.matrix(), RasterFormat::Png, &png, &ghost, DATA).unwrap();

    let webp = render_raster(code.matrix(), RasterFormat::Webp, &ghost).unwrap();
    assert!(!decode_image(&webp).unwrap().is_opaque());
    // JPEG flattens it onto white.
    let jpeg = render_raster(code.matrix(), RasterFormat::Jpeg, &ghost).unwrap();
    assert!(decode_image(&jpeg).unwrap().is_opaque());
    verify_raster(code.matrix(), RasterFormat::Jpeg, &jpeg, &ghost, DATA).unwrap();

    // An opaque logo on an opaque background is RGB in WebP too.
    let opaque = with_logo(disc(8), 100).with_fg(Color::rgb(0, 0, 0x80));
    let webp = render_raster(code.matrix(), RasterFormat::Webp, &opaque).unwrap();
    assert!(decode_image(&webp).unwrap().is_opaque());
}

#[test]
fn svg_embeds_the_logo_as_a_png_data_uri() {
    let code = code();
    let options = with_logo(disc(300), 250);
    let svg = render_svg(code.matrix(), &options).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    let images: Vec<_> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "image")
        .collect();
    assert_eq!(images.len(), 1);
    let image = images[0];
    let href = image.attribute("href").unwrap();
    let encoded = href.strip_prefix("data:image/png;base64,").unwrap();
    let png = decode_base64(encoded);
    let embedded = decode_image(&png).unwrap();
    // Twice the displayed size: 0.25 of n modules at 10 px each.
    let n = code.matrix().width() as u32;
    assert_eq!((embedded.width(), embedded.height()), (5 * n, 5 * n));
    assert!(!embedded.is_opaque());
    let width: f64 = image.attribute("width").unwrap().parse().unwrap();
    let x: f64 = image.attribute("x").unwrap().parse().unwrap();
    let n = f64::from(n);
    assert!((width - n / 4.0).abs() < 1e-9, "{width}");
    assert!((x - (4.0 + (n - n / 4.0) / 2.0)).abs() < 1e-9, "{x}");
    assert_eq!(image.attribute("preserveAspectRatio"), Some("none"));
    // The image comes after the modules, so it is painted over them.
    assert!(svg.find("<path").unwrap() < svg.find("<image").unwrap());
    verify_rendering(code.matrix(), &options, "svg", DATA).unwrap();

    // A small logo is embedded at its own size, never enlarged.
    let svg = render_svg(code.matrix(), &with_logo(solid(3, 2, [0, 0, 0, 255]), 300)).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    let href = doc
        .descendants()
        .find(|n| n.tag_name().name() == "image")
        .and_then(|n| n.attribute("href"))
        .unwrap();
    let png = decode_base64(href.strip_prefix("data:image/png;base64,").unwrap());
    let embedded = decode_image(&png).unwrap();
    assert_eq!((embedded.width(), embedded.height()), (3, 2));
    assert!(embedded.is_opaque());
}

#[test]
fn pdf_and_eps_refuse_a_logo() {
    let code = code();
    let options = with_logo(disc(8), 200);
    for (format, name) in [(VectorFormat::Pdf, "pdf"), (VectorFormat::Eps, "eps")] {
        let err = render_vector(code.matrix(), format, &options).unwrap_err();
        assert_eq!(err, RenderError::LogoUnsupported { format: name });
        assert_eq!(
            err.to_string(),
            format!("{name} output cannot carry a logo (use png, jpeg, webp or svg)")
        );
    }
    assert!(render_vector(code.matrix(), VectorFormat::Svg, &options).is_ok());
}

#[test]
fn large_images_and_small_symbols() {
    // A version 1 symbol caps the logo but still reads.
    let small = encode(b"hi", &EncodeOptions::default().with_ec_level(EcLevel::H)).unwrap();
    assert_eq!(small.matrix().width(), 21);
    let options = with_logo(disc(16), 300);
    for format in RasterFormat::ALL {
        let bytes = render_raster(small.matrix(), format, &options).unwrap();
        verify_raster(small.matrix(), format, &bytes, &options, b"hi").unwrap();
    }
    // A raster above the verification limit is redrawn with its logo.
    let code = code();
    let big = with_logo(disc(64), 300).with_size(Some(2_500));
    let png = render_raster(code.matrix(), RasterFormat::Png, &big).unwrap();
    let verified = verify_raster(code.matrix(), RasterFormat::Png, &png, &big, DATA).unwrap();
    assert!(verified.redrawn);
    // A huge logo is resampled down to its place.
    let huge = with_logo(solid(2_000, 1_000, [10, 10, 10, 255]), 200);
    let bytes = render_raster(code.matrix(), RasterFormat::Webp, &huge).unwrap();
    verify_raster(code.matrix(), RasterFormat::Webp, &bytes, &huge, DATA).unwrap();
}

/// Decodes standard padded base64.
fn decode_base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0_u32, 0);
    for c in text.bytes().filter(|c| *c != b'=') {
        let v = ALPHABET.iter().position(|a| *a == c).unwrap() as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}
