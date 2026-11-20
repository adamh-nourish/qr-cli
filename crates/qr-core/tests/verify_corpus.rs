//! The `rqrr` false-negative rate on styled codes (DECISIONS section 9,
//! risk 2).
//!
//! Every code in the corpus is a correct symbol drawn with a shape and
//! colours that keep at least 3:1 contrast, so any verification failure is
//! a false negative of the verifier, not a bad code. If the rate is above
//! [`MAX_FALSE_NEGATIVE_RATE`], auto-verify must warn instead of failing;
//! this test pins the measured rate so that decision stays valid.
//!
//! A second, harsher corpus (one to three pixels per module, quiet zones of
//! zero or one module, contrast down to 3:1) checks that verification stays
//! reliable, and that it never hands `rqrr` an image it would panic on. It
//! is a low-contrast and thin-margin measurement more than a shape one:
//! below about four pixels per module the shapes collapse to squares (dots
//! and rounded raster output at one and two pixels, and rounded at three,
//! are pixel-identical to square), so the report counts cases by the
//! geometry actually drawn.
//!
//! Both corpora stop at payloads of about 250 bytes (versions up to about
//! 13), so a third, smaller stratum covers versions 20 to 40.
//!
//! Run `cargo test -p qr-core --test verify_corpus -- --nocapture` to print
//! the breakdown.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// A test-only generator: truncating casts are the point.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
// The measured rate is the point of the test; `--nocapture` shows it.
#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use qr_core::verify::{verify_raster, verify_rendering};
use qr_core::{
    Color, EcLevel, EncodeOptions, ModuleShape, RasterFormat, RenderOptions, encode, render_raster,
};

/// The size of the corpus.
const CORPUS: usize = 200;

/// The decision threshold from DECISIONS section 9, risk 2.
const MAX_FALSE_NEGATIVE_RATE: f64 = 0.02;

/// Foreground and background pairs, all at least 3:1. The last two are
/// light on dark, as `--invert` produces.
const PALETTE: [(Color, Color); 8] = [
    (Color::BLACK, Color::WHITE),
    (Color::rgb(0x1a, 0x73, 0xe8), Color::WHITE),
    (Color::rgb(0, 0, 0x80), Color::rgb(0xff, 0xf8, 0xdc)),
    (Color::rgb(0x8b, 0, 0), Color::rgb(0xf5, 0xf5, 0xf5)),
    (Color::rgb(0x00, 0x64, 0x00), Color::rgb(0xff, 0xff, 0xe0)),
    (Color::rgb(0x66, 0x33, 0x99), Color::rgb(0xe6, 0xe6, 0xfa)),
    (Color::WHITE, Color::BLACK),
    (Color::rgb(0xff, 0xd7, 0x00), Color::rgb(0, 0, 0x80)),
];

/// Pairs near the 3:1 warning threshold, for the harsh corpus.
const LOW_CONTRAST: [(Color, Color); 4] = [
    (Color::rgb(0x94, 0x94, 0x94), Color::WHITE),
    (Color::rgb(0xe0, 0x4a, 0x4a), Color::WHITE),
    (Color::rgb(0x2a, 0x8a, 0x2a), Color::rgb(0xf0, 0xf0, 0xf0)),
    (Color::rgb(0xc0, 0xc0, 0xc0), Color::rgb(0x30, 0x30, 0x30)),
];

/// A small deterministic generator (`SplitMix64`), so the corpus is fixed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// A payload like the ones people encode: URLs, text, digits and binary.
fn payload(rng: &mut Rng) -> Vec<u8> {
    match rng.below(4) {
        0 => format!(
            "https://example.com/{}/item?id={}",
            ["shop", "docs", "a/b/c", "menu"][rng.below(4)],
            rng.next() % 1_000_000
        )
        .into_bytes(),
        1 => {
            let words = ["wifi", "hello", "QR", "code", "café", "naïve", "data", "42"];
            (0..=rng.below(30))
                .map(|_| rng.pick(&words))
                .collect::<Vec<_>>()
                .join(" ")
                .into_bytes()
        }
        2 => (0..=rng.below(200))
            .map(|_| b'0' + (rng.next() % 10) as u8)
            .collect(),
        _ => (0..=rng.below(250)).map(|_| rng.next() as u8).collect(),
    }
}

/// One styled code and how it is checked.
#[derive(Debug)]
struct Case {
    data: Vec<u8>,
    ec: EcLevel,
    options: RenderOptions,
    /// A raster format, or `None` for vector output (redrawn).
    format: Option<RasterFormat>,
}

/// How a corpus is drawn.
struct Spec<'a> {
    seed: u64,
    palette: &'a [(Color, Color)],
    formats: &'a [Option<RasterFormat>],
    scales: &'a [u32],
    margins: &'a [usize],
}

/// The corpus the auto-verify decision rests on: typical sizes and quiet
/// zones, every output path.
const STANDARD: Spec<'static> = Spec {
    seed: 0x5eed_c0de,
    palette: &PALETTE,
    formats: &[
        Some(RasterFormat::Png),
        Some(RasterFormat::Png),
        Some(RasterFormat::Jpeg),
        Some(RasterFormat::Webp),
        None,
    ],
    // From the --size of a small web image up to the default.
    scales: &[4, 5, 6, 8, 10, 10],
    margins: &[4, 4, 4, 2],
};

/// Tiny modules, thin quiet zones and low contrast. JPEG needs four pixels
/// per module, so it is left out.
const HARSH: Spec<'static> = Spec {
    seed: 0x0bad_5eed,
    palette: &LOW_CONTRAST,
    formats: &[Some(RasterFormat::Png), Some(RasterFormat::Webp), None],
    scales: &[1, 2, 3],
    margins: &[0, 1],
};

fn corpus(spec: &Spec<'_>) -> Vec<Case> {
    let mut rng = Rng(spec.seed);
    (0..CORPUS)
        .map(|i| {
            // Shapes are what auto-verify mostly guards; a fifth of the
            // corpus is square with custom colours.
            let shape = match i % 5 {
                0 | 1 => ModuleShape::Dots,
                2 | 3 => ModuleShape::Rounded,
                _ => ModuleShape::Square,
            };
            let (fg, bg) = rng.pick(spec.palette);
            let format = rng.pick(spec.formats);
            let scale = rng.pick(spec.scales);
            let margin = rng.pick(spec.margins);
            Case {
                data: payload(&mut rng),
                ec: EcLevel::ALL[rng.below(4)],
                options: RenderOptions::default()
                    .with_shape(shape)
                    .with_fg(fg)
                    .with_bg(bg)
                    .with_scale(scale)
                    .with_margin(margin)
                    .with_transparent(i % 7 == 0),
                format,
            }
        })
        .collect()
}

/// Verifies one case; `Err` holds why it failed.
fn check(case: &Case) -> Result<(), String> {
    let code = encode(&case.data, &EncodeOptions::default().with_ec_level(case.ec)).unwrap();
    let m = code.matrix();
    let result = match case.format {
        Some(format) => {
            let bytes = render_raster(m, format, &case.options).unwrap();
            verify_raster(m, format, &bytes, &case.options, &case.data)
        }
        None => verify_rendering(m, &case.options, "svg", &case.data),
    };
    result.map(|_| ()).map_err(|e| {
        format!(
            "{e} (v{}, {} {}, scale {}, margin {}, {} on {})",
            code.version(),
            case.ec,
            case.options.shape,
            case.options.scale,
            case.options.margin,
            case.options.fg,
            case.options.bg
        )
    })
}

/// The shape `case` is actually drawn with: a raster image whose modules
/// are too small for its shape to show is pixel-identical to a square one.
fn effective_shape(case: &Case) -> String {
    let shape = case.options.shape;
    let Some(format) = case.format else {
        return shape.to_string();
    };
    if shape == ModuleShape::Square {
        return shape.to_string();
    }
    let code = encode(&case.data, &EncodeOptions::default().with_ec_level(case.ec)).unwrap();
    let styled = render_raster(code.matrix(), format, &case.options).unwrap();
    let square = case.options.clone().with_shape(ModuleShape::Square);
    if render_raster(code.matrix(), format, &square).unwrap() == styled {
        format!("{shape} (drawn square)")
    } else {
        shape.to_string()
    }
}

/// Verifies every case of `spec`, prints a report and returns the
/// false-negative rate.
fn measure(name: &str, spec: &Spec<'_>) -> f64 {
    let cases = corpus(spec);
    assert_eq!(cases.len(), CORPUS);
    report(name, &cases)
}

/// Verifies `cases`, prints a report by the geometry actually drawn and
/// returns the false-negative rate.
fn report(name: &str, cases: &[Case]) -> f64 {
    let mut by_shape: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for case in cases {
        let entry = by_shape.entry(effective_shape(case)).or_default();
        entry.0 += 1;
        if let Err(why) = check(case) {
            entry.1 += 1;
            failures.push(why);
        }
    }
    let rate = failures.len() as f64 / cases.len().max(1) as f64;
    println!(
        "{name}: rqrr false negatives: {} of {} styled codes ({:.1}%)",
        failures.len(),
        cases.len(),
        rate * 100.0
    );
    for (shape, (total, failed)) in &by_shape {
        println!("  {shape}: {failed} of {total}");
    }
    for why in &failures {
        println!("  failed: {why}");
    }
    rate
}

#[test]
fn false_negative_rate_is_within_the_auto_verify_threshold() {
    let rate = measure("standard", &STANDARD);
    assert!(
        rate <= MAX_FALSE_NEGATIVE_RATE,
        "the false-negative rate {:.1}% is above {:.0}%: auto-verify must warn, not fail",
        rate * 100.0,
        MAX_FALSE_NEGATIVE_RATE * 100.0
    );
}

#[test]
fn harsh_corpus_reads_back_without_panicking() {
    let rate = measure("harsh", &HARSH);
    assert!(
        rate <= MAX_FALSE_NEGATIVE_RATE,
        "the harsh false-negative rate is {:.1}%",
        rate * 100.0
    );
}

/// Versions 20 to 40 (payloads of 700 to 2,300 bytes), every shape and
/// output path, at the module sizes a large code is printed at.
fn high_versions() -> Vec<Case> {
    let mut rng = Rng(0x7a11_c0de);
    let formats = [
        Some(RasterFormat::Png),
        Some(RasterFormat::Jpeg),
        Some(RasterFormat::Webp),
        None,
    ];
    let mut cases = Vec::new();
    for (i, len) in [700, 1_000, 1_500, 2_300].into_iter().enumerate() {
        for (j, shape) in ModuleShape::ALL.into_iter().enumerate() {
            let (fg, bg) = rng.pick(&PALETTE);
            cases.push(Case {
                data: (0..len).map(|_| rng.next() as u8).collect(),
                ec: EcLevel::M,
                options: RenderOptions::default()
                    .with_shape(shape)
                    .with_fg(fg)
                    .with_bg(bg)
                    .with_scale(rng.pick(&[4, 5, 6]))
                    .with_margin(rng.pick(&[4, 2])),
                format: formats[(i + j) % formats.len()],
            });
        }
    }
    cases
}

#[test]
fn high_versions_read_back() {
    let cases = high_versions();
    let versions: Vec<u8> = cases
        .iter()
        .map(|c| {
            encode(&c.data, &EncodeOptions::default().with_ec_level(c.ec))
                .unwrap()
                .version()
                .number()
        })
        .collect();
    assert!(versions.iter().all(|v| *v >= 20), "{versions:?}");
    assert!(versions.contains(&40), "{versions:?}");
    let rate = report("high versions", &cases);
    assert!(
        rate <= MAX_FALSE_NEGATIVE_RATE,
        "the high-version false-negative rate is {:.1}%",
        rate * 100.0
    );
}
