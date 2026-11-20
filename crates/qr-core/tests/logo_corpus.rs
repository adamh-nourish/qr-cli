//! The `rqrr` false-negative rate on logo codes (DECISIONS section 9,
//! risk 2, re-measured for logos).
//!
//! rqrr is weaker than phone scanners on logo codes, so before logos are
//! auto-verified like other styled codes the rate has to be at most
//! [`MAX_FALSE_NEGATIVE_RATE`]; above it, auto-verify must warn instead of
//! failing. Every code in the corpus is drawn exactly as the CLI draws a
//! logo code: EC level H (forced by `--logo`), a logo from 0.05 to 0.3 of
//! the symbol width over cleared modules, every shape, colours of at
//! least 3:1 and every output path (PNG, JPEG and WebP decoded from their
//! own bytes, SVG redrawn). The logos range from easy (a disc on
//! transparency) to hostile (a black-and-white checkerboard and random
//! noise, which look like modules).
//!
//! Run `cargo test -p qr-core --test logo_corpus -- --nocapture` to print
//! the breakdown.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss, clippy::print_stdout)]

use std::collections::BTreeMap;

use qr_core::loader::Bitmap;
use qr_core::logo::{Logo, LogoScale};
use qr_core::verify::{verify_raster, verify_rendering};
use qr_core::{
    Color, EcLevel, EncodeOptions, ModuleShape, RasterFormat, RenderOptions, encode, render_raster,
};

/// The size of the corpus.
const CORPUS: usize = 200;

/// The decision threshold from DECISIONS section 9, risk 2.
const MAX_FALSE_NEGATIVE_RATE: f64 = 0.02;

/// Foreground and background pairs, all at least 3:1 (two light on dark).
const PALETTE: [(Color, Color); 6] = [
    (Color::BLACK, Color::WHITE),
    (Color::BLACK, Color::WHITE),
    (Color::rgb(0x1a, 0x73, 0xe8), Color::WHITE),
    (Color::rgb(0, 0, 0x80), Color::rgb(0xff, 0xf8, 0xdc)),
    (Color::rgb(0x8b, 0, 0), Color::rgb(0xf5, 0xf5, 0xf5)),
    (Color::WHITE, Color::BLACK),
];

/// `SplitMix64`, so the corpus is fixed.
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

/// The kinds of logo drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// A solid dark square.
    Dark,
    /// A solid brand-coloured square.
    Brand,
    /// A disc on a transparent square.
    Disc,
    /// A wide (3:1) dark banner, like a word mark.
    Banner,
    /// A tall (1:3) shape.
    Tall,
    /// A horizontal gradient.
    Gradient,
    /// Black and white squares: looks like modules.
    Checker,
    /// Random pixels.
    Noise,
}

const KINDS: [Kind; 8] = [
    Kind::Dark,
    Kind::Brand,
    Kind::Disc,
    Kind::Banner,
    Kind::Tall,
    Kind::Gradient,
    Kind::Checker,
    Kind::Noise,
];

fn logo_image(kind: Kind, rng: &mut Rng) -> Bitmap {
    let side = [16_u32, 48, 100, 256][rng.below(4)];
    let (w, h) = match kind {
        Kind::Banner => (side, (side / 3).max(1)),
        Kind::Tall => ((side / 3).max(1), side),
        _ => (side, side),
    };
    let cell = (side / 8).max(1);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let px: [u8; 4] = match kind {
                Kind::Dark | Kind::Banner | Kind::Tall => [20, 20, 20, 255],
                Kind::Brand => [0xe8, 0x40, 0x1a, 255],
                Kind::Disc => {
                    let (dx, dy) = (
                        f64::from(x) - f64::from(w) / 2.0,
                        f64::from(y) - f64::from(h) / 2.0,
                    );
                    if dx.hypot(dy) <= f64::from(w) / 2.0 {
                        [0x1a, 0x73, 0xe8, 255]
                    } else {
                        [0, 0, 0, 0]
                    }
                }
                Kind::Gradient => {
                    let v = (x * 255 / w.max(1)) as u8;
                    [v, 255 - v, 128, 255]
                }
                Kind::Checker => {
                    if (x / cell + y / cell) % 2 == 0 {
                        [0, 0, 0, 255]
                    } else {
                        [255, 255, 255, 255]
                    }
                }
                Kind::Noise => {
                    let v = rng.next();
                    [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
                }
            };
            pixels.extend_from_slice(&px);
        }
    }
    Bitmap::from_rgba(w, h, pixels).unwrap()
}

/// A payload like the ones people put behind a logo: URLs, text, digits.
fn payload(rng: &mut Rng) -> Vec<u8> {
    match rng.below(4) {
        0 | 1 => format!(
            "https://example.com/{}/item?id={}",
            ["shop", "docs", "a/b/c", "menu"][rng.below(4)],
            rng.next() % 1_000_000
        )
        .into_bytes(),
        2 => {
            let words = ["wifi", "hello", "QR", "code", "café", "data", "42"];
            (0..=rng.below(20))
                .map(|_| rng.pick(&words))
                .collect::<Vec<_>>()
                .join(" ")
                .into_bytes()
        }
        _ => (0..=rng.below(120)).map(|_| rng.next() as u8).collect(),
    }
}

#[derive(Debug)]
struct Case {
    data: Vec<u8>,
    kind: Kind,
    options: RenderOptions,
    /// A raster format, or `None` for SVG (redrawn).
    format: Option<RasterFormat>,
}

fn corpus() -> Vec<Case> {
    let mut rng = Rng(0x1060_c0de);
    let scales = [50, 100, 150, 200, 200, 250, 300, 300];
    let formats = [
        Some(RasterFormat::Png),
        Some(RasterFormat::Png),
        Some(RasterFormat::Jpeg),
        Some(RasterFormat::Webp),
        None,
    ];
    (0..CORPUS)
        .map(|i| {
            let kind = KINDS[i % KINDS.len()];
            let shape = ModuleShape::ALL[(i / KINDS.len()) % 3];
            let (fg, bg) = rng.pick(&PALETTE);
            let logo = Logo::new(logo_image(kind, &mut rng))
                .with_scale(LogoScale::from_permille(rng.pick(&scales)).unwrap());
            Case {
                data: payload(&mut rng),
                kind,
                options: RenderOptions::default()
                    .with_shape(shape)
                    .with_fg(fg)
                    .with_bg(bg)
                    .with_scale(rng.pick(&[4, 5, 6, 8, 10]))
                    .with_transparent(i % 9 == 0)
                    .with_logo(Some(logo)),
                format: rng.pick(&formats),
            }
        })
        .collect()
}

fn check(case: &Case) -> Result<u8, String> {
    let options = EncodeOptions::default().with_ec_level(EcLevel::H);
    let code = encode(&case.data, &options).unwrap();
    let m = code.matrix();
    let result = match case.format {
        Some(format) => {
            let bytes = render_raster(m, format, &case.options).unwrap();
            verify_raster(m, format, &bytes, &case.options, &case.data)
        }
        None => verify_rendering(m, &case.options, "svg", &case.data),
    };
    let version = code.version().number();
    result.map(|_| version).map_err(|e| {
        let logo = case.options.logo.as_ref().unwrap();
        format!(
            "{e} (v{version}, {:?} logo at {}, {}, {:?}, scale {})",
            case.kind,
            logo.scale(),
            case.options.shape,
            case.format,
            case.options.scale,
        )
    })
}

#[test]
fn logo_false_negative_rate_is_within_the_auto_verify_threshold() {
    let cases = corpus();
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut by_scale: BTreeMap<u16, (usize, usize)> = BTreeMap::new();
    let mut versions = BTreeMap::new();
    let mut failures = Vec::new();
    for case in &cases {
        let scale = case.options.logo.as_ref().unwrap().scale().permille();
        let result = check(case);
        let failed = usize::from(result.is_err());
        for entry in [
            by_kind.entry(format!("{:?}", case.kind)).or_default(),
            by_scale.entry(scale).or_default(),
        ] {
            entry.0 += 1;
            entry.1 += failed;
        }
        match result {
            Ok(version) => *versions.entry(version).or_insert(0) += 1,
            Err(why) => failures.push(why),
        }
    }
    let rate = failures.len() as f64 / cases.len() as f64;
    println!(
        "logo: rqrr false negatives: {} of {} logo codes ({:.1}%)",
        failures.len(),
        cases.len(),
        rate * 100.0
    );
    for (kind, (total, failed)) in &by_kind {
        println!("  {kind}: {failed} of {total}");
    }
    for (scale, (total, failed)) in &by_scale {
        println!("  scale 0.{scale:03}: {failed} of {total}");
    }
    println!("  versions read: {versions:?}");
    for why in &failures {
        println!("  failed: {why}");
    }
    // The corpus spans small and medium symbols and every scale.
    assert!(versions.keys().any(|v| *v <= 2) && versions.keys().any(|v| *v >= 7));
    assert_eq!(by_scale.len(), 6);
    assert!(
        rate <= MAX_FALSE_NEGATIVE_RATE,
        "the logo false-negative rate {:.1}% is above {:.0}%: auto-verify must warn, not fail",
        rate * 100.0,
        MAX_FALSE_NEGATIVE_RATE * 100.0
    );
}
