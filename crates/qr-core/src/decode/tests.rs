use super::*;
use crate::finders::finder_grid;
use crate::loader::decode_image;
use crate::render::{ModuleShape, RenderOptions, render_jpeg, render_png, render_webp};
use crate::{EncodeOptions, encode};

fn png(data: &[u8], options: EncodeOptions, render: &RenderOptions) -> Vec<u8> {
    let code = encode(data, &options).unwrap();
    render_png(code.matrix(), render).unwrap()
}

/// Decodes `image`, which must not be refused.
fn read(image: Bitmap) -> Scan {
    decode_bitmap(image).unwrap()
}

fn bitmap(bytes: &[u8]) -> Bitmap {
    decode_image(bytes).unwrap()
}

/// A white canvas with `parts` pasted at the given offsets.
fn canvas(width: u32, height: u32, parts: &[(&Bitmap, u32, u32)]) -> Bitmap {
    let mut pixels = vec![255u8; (width * height * 4) as usize];
    for (part, ox, oy) in parts {
        for y in 0..part.height() {
            for x in 0..part.width() {
                let src = ((y * part.width() + x) * 4) as usize;
                let dst = (((oy + y) * width + ox + x) * 4) as usize;
                pixels[dst..dst + 4].copy_from_slice(&part.pixels()[src..src + 4]);
            }
        }
    }
    Bitmap::from_rgba(width, height, pixels).unwrap()
}

/// `image` turned a quarter turn clockwise.
fn rotated(image: &Bitmap) -> Bitmap {
    let (w, h) = (image.width(), image.height());
    let mut pixels = vec![0u8; image.pixels().len()];
    for y in 0..h {
        for x in 0..w {
            let src = ((y * w + x) * 4) as usize;
            let (nx, ny) = (h - 1 - y, x);
            let dst = ((ny * h + nx) * 4) as usize;
            pixels[dst..dst + 4].copy_from_slice(&image.pixels()[src..src + 4]);
        }
    }
    Bitmap::from_rgba(h, w, pixels).unwrap()
}

#[test]
fn reports_payload_version_level_and_mask() {
    for level in EcLevel::ALL {
        for mask in [0, 3, 7] {
            let options = EncodeOptions::default()
                .with_ec_level(level)
                .with_mask(Mask::new(mask));
            let code = encode(b"https://example.com/decode", &options).unwrap();
            let bytes = render_png(code.matrix(), &RenderOptions::default()).unwrap();
            let scan = read(bitmap(&bytes));
            assert_eq!(scan.codes.len(), 1, "{level} {mask}");
            let found = &scan.codes[0];
            assert_eq!(found.payload, b"https://example.com/decode");
            assert_eq!(found.version, code.version());
            assert_eq!(found.ec_level, level);
            assert_eq!(found.mask, code.mask());
            assert!(!found.inverted);
            assert_eq!((scan.undecodable, scan.enlarged), (0, 3));
        }
    }
}

#[test]
fn bounds_are_in_source_pixels() {
    // Version 1 at 10 px per module with a 4-module quiet zone: the symbol
    // spans pixels 40 to 250.
    let bytes = png(b"hi", EncodeOptions::default(), &RenderOptions::default());
    let scan = read(bitmap(&bytes));
    let [tl, tr, br, bl] = scan.codes[0].bounds;
    let near = |p: Point, x: i32, y: i32| (p.x - x).abs() <= 3 && (p.y - y).abs() <= 3;
    assert!(near(tl, 40, 40), "{tl:?}");
    assert!(near(tr, 250, 40), "{tr:?}");
    assert!(near(br, 250, 250), "{br:?}");
    assert!(near(bl, 40, 250), "{bl:?}");
}

#[test]
fn every_raster_format_and_shape_reads() {
    let code = encode(b"formats", &EncodeOptions::default()).unwrap();
    for shape in [ModuleShape::Square, ModuleShape::Rounded, ModuleShape::Dots] {
        let options = RenderOptions::default().with_shape(shape);
        for bytes in [
            render_png(code.matrix(), &options).unwrap(),
            render_jpeg(code.matrix(), &options).unwrap(),
            render_webp(code.matrix(), &options).unwrap(),
        ] {
            let scan = read(bitmap(&bytes));
            assert_eq!(scan.codes.len(), 1, "{shape:?}");
            assert_eq!(scan.codes[0].payload, b"formats");
        }
    }
}

#[test]
fn finds_every_code_in_an_image_in_reading_order() {
    let a = bitmap(&png(
        b"first",
        EncodeOptions::default(),
        &RenderOptions::default(),
    ));
    let b = bitmap(&png(
        b"second",
        EncodeOptions::default(),
        &RenderOptions::default(),
    ));
    let c = bitmap(&png(
        b"third",
        EncodeOptions::default(),
        &RenderOptions::default(),
    ));
    let image = canvas(700, 700, &[(&b, 380, 10), (&c, 10, 400), (&a, 10, 10)]);
    let scan = read(image);
    let payloads: Vec<&[u8]> = scan.codes.iter().map(|c| c.payload.as_slice()).collect();
    assert_eq!(payloads, [&b"first"[..], b"second", b"third"]);
}

#[test]
fn light_on_dark_codes_are_read_inverted() {
    let render = RenderOptions::default()
        .with_fg(Color::WHITE)
        .with_bg(Color::BLACK);
    let scan = read(bitmap(&png(b"inverse", EncodeOptions::default(), &render)));
    assert_eq!(scan.codes.len(), 1);
    assert_eq!(scan.codes[0].payload, b"inverse");
    assert!(scan.codes[0].inverted);
}

#[test]
fn transparency_is_read_on_white() {
    let render = RenderOptions::default().with_transparent(true);
    let scan = read(bitmap(&png(b"clear", EncodeOptions::default(), &render)));
    assert_eq!(scan.codes[0].payload, b"clear");
}

#[test]
fn one_pixel_modules_are_enlarged_and_do_not_panic() {
    // The case that trips rqrr 0.9's `scan >= 1` assertion when read as is.
    for margin in [0, 1, 4] {
        let render = RenderOptions::default().with_scale(1).with_margin(margin);
        let bytes = png(b"tiny modules", EncodeOptions::default(), &render);
        let scan = read(bitmap(&bytes));
        assert_eq!(scan.enlarged, 3);
        // A code without a quiet zone may not be found; it must not panic.
        if margin == 4 {
            assert_eq!(scan.codes.len(), 1, "margin {margin}");
            assert_eq!(scan.codes[0].payload, b"tiny modules");
            // The bounds are mapped back to source pixels.
            let [tl, _, br, _] = scan.codes[0].bounds;
            assert_eq!((tl, br), (Point { x: 4, y: 4 }, Point { x: 25, y: 25 }));
        }
    }
}

#[test]
fn rotated_codes_read() {
    let image = bitmap(&png(
        b"turned",
        EncodeOptions::default(),
        &RenderOptions::default(),
    ));
    let scan = read(rotated(&image));
    assert_eq!(scan.codes[0].payload, b"turned");
}

#[test]
fn blank_and_noisy_images_hold_no_code() {
    let blank = Bitmap::from_rgba(50, 40, vec![255; 50 * 40 * 4]).unwrap();
    let scan = read(blank);
    assert!(scan.is_empty());
    assert_eq!(scan.undecodable, 0);
    let dark = Bitmap::from_rgba(3, 3, vec![0; 36]).unwrap();
    assert!(read(dark).is_empty());
    // A deterministic pseudo-random pattern.
    let mut state = 0x2545_f491_u32;
    let noise: Vec<u8> = (0..64 * 64)
        .flat_map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let v = if state & 1 == 0 { 0 } else { 255 };
            [v, v, v, 255]
        })
        .collect();
    assert!(read(Bitmap::from_rgba(64, 64, noise).unwrap()).is_empty());
}

#[test]
fn a_damaged_code_is_counted_as_undecodable() {
    let image = bitmap(&png(
        b"damaged beyond repair",
        EncodeOptions::default().with_ec_level(EcLevel::L),
        &RenderOptions::default(),
    ));
    // Blank out the data area between the finder patterns, keeping the
    // finders and timing patterns so the symbol is still located.
    let (w, h) = (image.width(), image.height());
    let mut pixels = image.into_pixels();
    for y in 130..200 {
        for x in 130..200 {
            let at = ((y * w + x) * 4) as usize;
            pixels[at..at + 3].fill(0);
        }
    }
    let scan = read(Bitmap::from_rgba(w, h, pixels).unwrap());
    assert!(scan.is_empty());
    assert_eq!(scan.undecodable, 1);
}

#[test]
fn the_read_factor_shrinks_with_the_image() {
    assert_eq!(read_factor(1, 1), 3);
    assert_eq!(read_factor(1_365, 1_365), 3);
    assert_eq!(read_factor(1_366, 1_366), 2);
    assert_eq!(read_factor(2_048, 2_048), 2);
    assert_eq!(read_factor(2_049, 2_048), 1);
    assert_eq!(read_factor(8_192, 8_192), 1);
    assert_eq!(read_factor(usize::MAX, usize::MAX), 1);
}

#[test]
fn ec_bits_map_to_levels() {
    assert_eq!(ec_level(0), Some(EcLevel::M));
    assert_eq!(ec_level(1), Some(EcLevel::L));
    assert_eq!(ec_level(2), Some(EcLevel::H));
    assert_eq!(ec_level(3), Some(EcLevel::Q));
    assert_eq!(ec_level(4), None);
}

#[test]
fn out_of_range_metadata_is_rejected() {
    let meta = |version, ecc_level, mask| rqrr::MetaData {
        version: rqrr::Version(version),
        ecc_level,
        mask,
    };
    let bounds = [rqrr::Point { x: 0, y: 0 }; 4];
    let ok = decoded_code(vec![1], &meta(1, 0, 0), &bounds, 1, false);
    assert!(ok.is_some());
    for bad in [meta(0, 0, 0), meta(41, 0, 0), meta(1, 4, 0), meta(1, 0, 8)] {
        assert!(decoded_code(vec![], &bad, &bounds, 1, false).is_none());
    }
    // Corners map back with floor division, also left of the image.
    let bounds = [rqrr::Point { x: -1, y: 5 }; 4];
    let code = decoded_code(vec![], &meta(1, 0, 0), &bounds, 3, true).unwrap();
    assert_eq!(code.bounds[0], Point { x: -1, y: 1 });
    assert!(code.inverted);
    // rqrr's far corners, one module out, are pulled back onto the symbol.
    let far = |x, y| rqrr::Point { x, y };
    let bounds = [far(40, 40), far(260, 40), far(260, 260), far(40, 260)];
    let code = decoded_code(vec![], &meta(1, 0, 0), &bounds, 1, false).unwrap();
    assert_eq!(
        code.bounds,
        [
            Point { x: 40, y: 40 },
            Point { x: 250, y: 40 },
            Point { x: 250, y: 250 },
            Point { x: 40, y: 250 }
        ]
    );
}

#[test]
fn debug_output_hides_the_payload() {
    let bytes = png(
        b"secret",
        EncodeOptions::default(),
        &RenderOptions::default(),
    );
    let scan = read(bitmap(&bytes));
    let text = format!("{:?}", scan.codes[0]);
    assert!(
        text.contains("bytes: 6") && !text.contains("secret"),
        "{text}"
    );
}

#[test]
fn the_guard_is_needed_one_pixel_modules_panic_rqrr_unenlarged() {
    // Pins the hazard the guard exists for: if a future rqrr stops
    // panicking here, the guard (and its cost) can be reconsidered.
    let render = RenderOptions::default().with_scale(1);
    let bytes = png(b"tiny modules", EncodeOptions::default(), &render);
    let grey = Grey::from_bitmap(bitmap(&bytes));
    let unguarded = std::panic::catch_unwind(|| scan(&grey, 1, false));
    assert!(unguarded.is_err(), "rqrr no longer panics on 1-px modules");
    assert_eq!(scan(&grey, 3, false).unwrap().codes.len(), 1);
}

#[test]
fn twice_the_size_is_enough_for_one_pixel_modules() {
    // Images between 1,366 and 2,048 pixels across are enlarged only
    // twice; one-pixel modules then still read and do not panic.
    let render = RenderOptions::default().with_scale(1);
    for data in [&b"tiny"[..], &[b'x'; 300][..]] {
        let tiny = bitmap(&png(data, EncodeOptions::default(), &render));
        let image = canvas(1_400, 1_400, &[(&tiny, 700, 700)]);
        let scan = read(image);
        assert_eq!(scan.enlarged, 2);
        assert_eq!(scan.codes.len(), 1);
        assert_eq!(scan.codes[0].payload, data);
        assert_eq!(scan.codes[0].bounds[0], Point { x: 704, y: 704 });
    }
}

#[test]
#[allow(clippy::cast_possible_truncation)] // Pseudo-random bits.
fn damaged_thin_module_codes_never_panic() {
    // Read at their own size, damaged codes at 1 px per module trip rqrr's
    // assertion about one time in seven; enlarged, never (3,000 cases
    // when measured, PR description). A deterministic sample keeps it so.
    let mut state = 0x9e37_79b9_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as usize
    };
    for _ in 0..120 {
        let data: Vec<u8> = (0..=next() % 120).map(|_| next() as u8).collect();
        let render = RenderOptions::default()
            .with_scale(1 + (next() % 3) as u32)
            .with_margin(next() % 5);
        let image = bitmap(&png(&data, EncodeOptions::default(), &render));
        let (w, h) = (image.width(), image.height());
        let mut pixels = image.into_pixels();
        for _ in 0..next() % 40 {
            let at = (next() % (pixels.len() / 4)) * 4;
            let flipped = !pixels[at];
            pixels[at..at + 3].fill(flipped);
        }
        let scan = read(Bitmap::from_rgba(w, h, pixels).unwrap());
        for code in &scan.codes {
            assert_eq!(code.payload, data);
        }
    }
}

#[test]
fn a_dense_grid_of_finder_patterns_is_refused_quickly() {
    // 40 x 40 finder patterns: about 194 s of CPU in rqrr 0.9.3 before the
    // bound. Refused after one pass over the pixels; the generous limit
    // allows for unoptimised, instrumented test builds.
    let image = finder_grid(40, 3, 4, 1_365);
    let started = std::time::Instant::now();
    let err = decode_bitmap(image).unwrap_err();
    assert_eq!(
        err,
        DecodeError::TooManyFinders {
            limit: MAX_FINDER_CANDIDATES
        }
    );
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    assert_eq!(
        err.to_string(),
        "the image holds more than 64 finder-like patterns, too many to search"
    );
}

#[test]
fn the_inverse_is_bounded_too() {
    // Light patterns on dark: the normal pass finds nothing to group and
    // the inverted pass refuses the image.
    let image = finder_grid(12, 3, 2, 400);
    let (w, h) = (image.width(), image.height());
    let mut pixels = image.into_pixels();
    for px in pixels.chunks_mut(4) {
        px[..3].iter_mut().for_each(|c| *c = !*c);
    }
    let err = decode_bitmap(Bitmap::from_rgba(w, h, pixels).unwrap()).unwrap_err();
    assert!(matches!(err, DecodeError::TooManyFinders { .. }));
}

#[test]
fn a_sheet_of_codes_is_within_the_bound() {
    // Twelve codes, 36 finder patterns, are searched rather than refused.
    // (rqrr 0.9 pairs some finder patterns across neighbouring codes, so
    // not every code in so regular a sheet is read.)
    let parts: Vec<Bitmap> = (0..12)
        .map(|i| {
            let render = RenderOptions::default().with_scale(4);
            bitmap(&png(
                format!("code {i}").as_bytes(),
                EncodeOptions::default(),
                &render,
            ))
        })
        .collect();
    let placed: Vec<(&Bitmap, u32, u32)> = (0u32..)
        .zip(&parts)
        .map(|(i, part)| {
            (
                part,
                (i % 4) * 160 + (i / 4) * 25,
                (i / 4) * 160 + (i % 4) * 15,
            )
        })
        .collect();
    let scan = read(canvas(760, 600, &placed));
    assert!(!scan.is_empty());
}
