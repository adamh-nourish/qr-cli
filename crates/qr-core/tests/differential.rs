//! Differential tests against the `qrcode` crate's own high-level API.
//!
//! The production backend drives `qrcode`'s low-level API with segments and
//! a version chosen by `qr_core::plan`, and recovers the mask from the
//! format information. These tests check that this path is
//! indistinguishable from calling `qrcode` directly whenever both pick the
//! same version (the same automatic mask and bit-identical matrices), that
//! `qr_core` never picks a larger version, and that it reports `DataTooLong`
//! only when `qrcode` does. `qr_core` can be smaller because `plan` also
//! tries one segment in the narrowest mode, which `qrcode`'s greedy
//! segmentation sometimes beats by a version.
//!
//! These tests check the plumbing against `qrcode`, not the standard: the
//! independent checks are the `rqrr` round trips and `tests/mask_penalty.rs`.
//!
//! `qrcode` treats byte pairs that look like Shift JIS as Kanji, which
//! `qr_core` deliberately never does (UTF-8 text would not round-trip), so
//! the comparisons use data `qrcode` cannot read as Kanji: ASCII, or a fixed
//! byte mode fed to `qrcode` as one byte segment.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point in this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use qr_core::{
    CapacityUnit, EcLevel, EncodeError, EncodeOptions, Mask, Mode, ModeChoice, QrCode, QrEncoder,
    QrcodeEncoder, Segment, Version, VersionChoice, encode,
};

fn reference_ec(ec: EcLevel) -> qrcode::EcLevel {
    match ec {
        EcLevel::L => qrcode::EcLevel::L,
        EcLevel::M => qrcode::EcLevel::M,
        EcLevel::Q => qrcode::EcLevel::Q,
        EcLevel::H => qrcode::EcLevel::H,
    }
}

fn version_number(version: qrcode::Version) -> u8 {
    match version {
        qrcode::Version::Normal(v) => u8::try_from(v).unwrap(),
        qrcode::Version::Micro(_) => unreachable!("Micro QR is never chosen automatically"),
    }
}

fn dark_modules(code: &qrcode::QrCode) -> Vec<bool> {
    code.to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect()
}

/// `qrcode`'s fully automatic encoding (segmentation, version and mask).
fn reference_auto(data: &[u8], ec: EcLevel) -> Result<qrcode::QrCode, qrcode::types::QrError> {
    qrcode::QrCode::with_error_correction_level(data, reference_ec(ec))
}

fn assert_same_symbol(ours: &QrCode, reference: &qrcode::QrCode) {
    assert_eq!(
        ours.version().number(),
        version_number(reference.version()),
        "version"
    );
    assert_eq!(
        ours.matrix().modules(),
        dark_modules(reference).as_slice(),
        "matrix (v{}, mask {})",
        ours.version(),
        ours.mask()
    );
}

/// Our automatic encode against `qrcode`'s: never a larger version, the
/// same symbol whenever the version is the same, and too long only when
/// `qrcode` is too.
///
/// `qr_core` can pick a smaller version than `qrcode`, whose greedy
/// segmentation sometimes needs more bits than one segment in the
/// narrowest mode (`plan` also tries that segment). Review of PR 8b
/// (GitHub #9) found `XSL790106502KNC+` at Q: v1 here, v2 in `qrcode`.
fn assert_matches_qrcode(data: &[u8], ec: EcLevel) {
    let ours = encode(data, &EncodeOptions::default().with_ec_level(ec));
    match (ours, reference_auto(data, ec)) {
        (Ok(ours), Ok(reference)) => {
            let theirs = version_number(reference.version());
            assert!(
                ours.version().number() <= theirs,
                "v{} > v{theirs}",
                ours.version()
            );
            if ours.version().number() == theirs {
                assert_same_symbol(&ours, &reference);
            }
        }
        (Ok(_) | Err(EncodeError::DataTooLong(_)), Err(qrcode::types::QrError::DataTooLong)) => {}
        (ours, reference) => panic!(
            "{} bytes at {ec}: ours {ours:?}, qrcode {:?}",
            data.len(),
            reference.map(|r| r.version())
        ),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Printable ASCII with digit and upper-case runs, so segmentation
    /// splits and merges in every combination.
    #[test]
    fn automatic_encoding_matches_qrcode_on_mixed_ascii(
        data in "([0-9]{0,40}|[A-Z $%*+./:-]{0,30}|[a-z@?&=_]{0,20}){0,30}",
        ec in prop::sample::select(EcLevel::ALL.to_vec()),
    ) {
        assert_matches_qrcode(data.as_bytes(), ec);
    }

    /// Any ASCII bytes, including control characters.
    #[test]
    fn automatic_encoding_matches_qrcode_on_arbitrary_ascii(
        data in prop::collection::vec(0u8..0x80, 0..1_200),
        ec in prop::sample::select(EcLevel::ALL.to_vec()),
    ) {
        assert_matches_qrcode(&data, ec);
    }

    /// A forced mask is drawn exactly as `qrcode` draws it at that version.
    #[test]
    fn forced_masks_match_qrcode(
        data in "[0-9A-Za-z:/.-]{0,300}",
        mask in 0u8..=7,
    ) {
        let options = EncodeOptions::default().with_mask(Mask::new(mask));
        let ours = encode(data.as_bytes(), &options).unwrap();
        let reference =
            reference_with_mask(data.as_bytes(), ours.version(), ours.mask(), ours.segments());
        prop_assert_eq!(ours.matrix().modules(), reference.as_slice());
    }
}

/// `qrcode`'s own segmentation at `version`, with `mask` forced.
///
/// Where `qrcode`'s segments do not fit `version`, `qr_core` chose a
/// smaller version by packing the data as one segment in its narrowest
/// mode; `ours` must then be that one segment, and it is drawn instead.
fn reference_with_mask(data: &[u8], version: Version, mask: Mask, ours: &[Segment]) -> Vec<bool> {
    use qrcode::canvas::{Canvas, MaskPattern};
    let version = qrcode::Version::Normal(i16::from(version.number()));
    let ec = qrcode::EcLevel::M;
    let greedy = || {
        let mut bits = qrcode::bits::Bits::new(version);
        bits.push_optimal_data(data)?;
        bits.push_terminator(ec)?;
        Ok::<_, qrcode::types::QrError>(bits)
    };
    let bits = greedy().unwrap_or_else(|_| {
        let [single] = ours else {
            panic!("qrcode's segments do not fit, but ours are {ours:?}")
        };
        assert_eq!(
            (single.mode, single.range()),
            (Mode::narrowest_for(data), 0..data.len())
        );
        let mut bits = qrcode::bits::Bits::new(version);
        match single.mode {
            Mode::Numeric => bits.push_numeric_data(data),
            Mode::Alphanumeric => bits.push_alphanumeric_data(data),
            Mode::Byte => bits.push_byte_data(data),
        }
        .unwrap();
        bits.push_terminator(ec).unwrap();
        bits
    });
    let (codewords, ec_codewords) =
        qrcode::ec::construct_codewords(&bits.into_bytes(), version, ec).unwrap();
    let mut canvas = Canvas::new(version, ec);
    canvas.draw_all_functional_patterns();
    canvas.draw_data(&codewords, &ec_codewords);
    let pattern = [
        MaskPattern::Checkerboard,
        MaskPattern::HorizontalLines,
        MaskPattern::VerticalLines,
        MaskPattern::DiagonalLines,
        MaskPattern::LargeCheckerboard,
        MaskPattern::Fields,
        MaskPattern::Diamonds,
        MaskPattern::Meadow,
    ][usize::from(mask.number())];
    canvas.apply_mask(pattern);
    canvas
        .into_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect()
}

#[test]
fn every_mask_matches_qrcode_at_version_40() {
    let data: Vec<u8> = (0..2_300u32)
        .map(|i| b"qr-cli/0123:"[(i % 12) as usize])
        .collect();
    for mask in Mask::all() {
        let ours = encode(&data, &EncodeOptions::default().with_mask(Some(mask))).unwrap();
        assert_eq!(ours.version(), Version::MAX);
        assert_eq!(
            ours.matrix().modules(),
            reference_with_mask(&data, Version::MAX, mask, ours.segments()).as_slice()
        );
    }
}

/// Known-answer table: versions at EC level M for typical payloads. Before
/// the switch to `qrcode` (PR 8b), single-mode auto needed the version in
/// the second column; segmentation now matches `qrcode` (third column).
/// A change to segmentation must update this table deliberately.
#[test]
fn segmentation_matches_qrcode_on_typical_payloads() {
    let digits = |n: usize| "0123456789".repeat(n / 10 + 1)[..n].to_owned();
    // (payload, version before PR 8b, version now and with qrcode)
    let cases: Vec<(String, u8, u8)> = vec![
        ("tel:+441234567890".into(), 2, 1),
        ("SMSTO:+447700900123:Hello there".into(), 3, 2),
        ("https://example.com/orders/1234567890123456".into(), 4, 3),
        (
            "BEGIN:VCARD\nVERSION:3.0\nN:Doe;Jane\nFN:Jane Doe\nTEL:+441234567890\nEND:VCARD"
                .into(),
            5,
            5,
        ),
        (format!("order-{}", digits(200)), 10, 6),
        (format!("ref:{}", digits(1000)), 26, 16),
        ("WIFI:T:WPA;S:home;P:correct horse;;".into(), 3, 3),
        ("geo:51.5074,-0.1278".into(), 2, 2),
        ("HTTPS://EXAMPLE.COM/A/B".into(), 2, 2),
    ];
    for (data, _before, now) in &cases {
        let ours = encode(data.as_bytes(), &EncodeOptions::default()).unwrap();
        assert_eq!(ours.version().number(), *now, "{data}");
        let reference = reference_auto(data.as_bytes(), EcLevel::M).unwrap();
        assert_same_symbol(&ours, &reference);
    }
}

/// The capacity ceiling for mixed data is `qrcode`'s: a short prefix on
/// 5,000 digits is over the byte-mode limit (2,953 at L) but fits as a
/// byte segment plus a numeric one; at Q it is too long for both, and the
/// error is exact in bits.
#[test]
fn mixed_data_capacity_matches_qrcode() {
    let digits: String = "0123456789".repeat(500);
    let long = format!("ref:{digits}");
    for ec in [EcLevel::L, EcLevel::M] {
        let ours = encode(long.as_bytes(), &EncodeOptions::default().with_ec_level(ec)).unwrap();
        assert_same_symbol(&ours, &reference_auto(long.as_bytes(), ec).unwrap());
    }

    let at_q = EncodeOptions::default().with_ec_level(EcLevel::Q);
    let err = encode(long.as_bytes(), &at_q).unwrap_err();
    assert!(matches!(
        reference_auto(long.as_bytes(), EcLevel::Q),
        Err(qrcode::types::QrError::DataTooLong)
    ));
    let EncodeError::DataTooLong(e) = err else {
        panic!("expected a capacity error, got {err:?}")
    };
    assert_eq!(e.unit, CapacityUnit::Bits);
    // Byte "ref:" at v40: 4 + 16 + 32 bits; 5,000 digits: 4 + 14 + 16,667.
    assert_eq!((e.needed, e.max), (52 + 16_685, 1_666 * 8));
    assert_eq!((e.bytes, e.version), (5_004, Version::MAX));
}

/// The boundary is exact: find the longest digit tail that fits after a
/// byte prefix, and check it fits `qrcode` while one more digit fails both.
#[test]
fn mixed_data_boundary_is_exact() {
    for ec in EcLevel::ALL {
        let make = |n: usize| {
            let mut data = b"id=".to_vec();
            data.extend((0..n).map(|i| b"9876543210"[i % 10]));
            data
        };
        let options = EncodeOptions::default().with_ec_level(ec);
        let fits = |n: usize| encode(&make(n), &options).is_ok();
        let (mut lo, mut hi) = (0, 8_000);
        while lo + 1 < hi {
            let mid = usize::midpoint(lo, hi);
            if fits(mid) { lo = mid } else { hi = mid }
        }
        assert!(reference_auto(&make(lo), ec).is_ok(), "{ec}: {lo} digits");
        assert!(
            reference_auto(&make(lo + 1), ec).is_err(),
            "{ec}: {} digits",
            lo + 1
        );
        match encode(&make(lo + 1), &options) {
            Err(EncodeError::DataTooLong(e)) => {
                assert_eq!(e.unit, CapacityUnit::Bits);
                assert!(e.excess() > 0 && e.excess() <= 4, "{e:?}");
            }
            other => panic!("expected a capacity error, got {other:?}"),
        }
    }
}

/// The automatic mask is `qrcode`'s ISO lowest-penalty choice, on 400
/// seeded pseudo-random byte inputs of any value (0..600 bytes, EC level M,
/// fixed byte mode so `qrcode` sees one byte segment too). Under `fast_qr`
/// this matched in 273/400; now every one must.
#[test]
fn automatic_mask_is_qrcodes_on_random_bytes() {
    let mut state: u64 = 0x5eed_0000_0000_0001;
    let mut next = || {
        // splitmix64: deterministic and dependency-free.
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let options = EncodeOptions::default().with_mode(ModeChoice::Fixed(Mode::Byte));
    for _ in 0..400 {
        let len = usize::try_from(next() % 600).unwrap();
        let data: Vec<u8> = (0..len).map(|_| next().to_le_bytes()[0]).collect();
        let ours = QrcodeEncoder.encode(&data, &options).unwrap();
        let version = qrcode::Version::Normal(i16::from(ours.version().number()));
        let mut bits = qrcode::bits::Bits::new(version);
        bits.push_byte_data(&data).unwrap();
        bits.push_terminator(qrcode::EcLevel::M).unwrap();
        let reference = qrcode::QrCode::with_bits(bits, qrcode::EcLevel::M).unwrap();
        assert_same_symbol(&ours, &reference);
    }
}

/// Decodes a bare module grid with `rqrr`.
fn rqrr_decode(width: usize, modules: &[bool]) -> Result<Vec<u8>, rqrr::DeQRError> {
    let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(width, |x, y| {
        modules[y * width + x]
    }));
    let mut data = Vec::new();
    grid.decode_to(&mut data).map(|_| data)
}

/// Where `qrcode` would use Kanji, `qr_core` keeps UTF-8 in byte mode.
/// `qrcode` packs byte pairs of UTF-8 Japanese that happen to be valid
/// Shift JIS into Kanji mode. `rqrr` hands Kanji segments back as raw bytes,
/// but scanners convert them from Shift JIS to text, which garbles UTF-8;
/// so the symbols must differ.
#[test]
fn kanji_is_never_used() {
    let data = "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}"
        .repeat(4)
        .into_bytes();
    let ours = encode(&data, &EncodeOptions::default()).unwrap();
    assert_eq!(ours.mode(), Mode::Byte);
    let w = ours.matrix().width();
    assert_eq!(rqrr_decode(w, ours.matrix().modules()).unwrap(), data);

    let reference = reference_auto(&data, EcLevel::M).unwrap();
    let modules = dark_modules(&reference);
    assert_ne!(ours.matrix().modules(), modules.as_slice());
}

/// Where `qrcode`'s greedy segmentation loses to one segment in the
/// narrowest mode, `qr_core` picks the smaller version and the symbol
/// still decodes; at full capacity `qrcode` reports `DataTooLong`.
#[test]
fn one_segment_beats_qrcodes_greedy_segmentation() {
    let data = b"XSL790106502KNC+";
    let at_q = EncodeOptions::default().with_ec_level(EcLevel::Q);
    let ours = encode(data, &at_q).unwrap();
    assert_eq!(
        (ours.version().number(), ours.mode()),
        (1, Mode::Alphanumeric)
    );
    assert!(!ours.is_mixed_mode());
    assert_eq!(
        version_number(reference_auto(data, EcLevel::Q).unwrap().version()),
        2
    );
    let w = ours.matrix().width();
    assert_eq!(rqrr_decode(w, ours.matrix().modules()).unwrap(), data);

    let full: Vec<u8> = data.iter().copied().cycle().take(4_296).collect();
    let at_l = EncodeOptions::default().with_ec_level(EcLevel::L);
    assert_eq!(encode(&full, &at_l).unwrap().version(), Version::MAX);
    assert!(matches!(
        reference_auto(&full, EcLevel::L),
        Err(qrcode::types::QrError::DataTooLong)
    ));
}

#[test]
fn exact_version_still_bounds_segmented_data() {
    let data = b"tel:+441234567890";
    let at_least_3 =
        EncodeOptions::default().with_version(VersionChoice::AtLeast(Version::new(3).unwrap()));
    let code = encode(data, &at_least_3).unwrap();
    assert_eq!(code.version().number(), 3);
    let reference =
        qrcode::QrCode::with_version(data, qrcode::Version::Normal(3), qrcode::EcLevel::M).unwrap();
    assert_same_symbol(&code, &reference);
}
