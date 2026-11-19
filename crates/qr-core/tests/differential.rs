//! Differential test against the `qrcode` crate, implemented as a second
//! [`QrEncoder`] backend. With the same plan and a forced mask, the two
//! independent encoders must produce bit-identical matrices. This also
//! proves the trait is enough to plug in an alternative backend.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point in this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use qr_core::{
    EcLevel, EncodeError, EncodeOptions, EncodePlan, FastQrEncoder, Mask, Matrix, Mode, ModeChoice,
    QrCode, QrEncoder, plan,
};
use qrcode::canvas::{Canvas, MaskPattern};

/// Reference backend over `qrcode`'s low-level API. Only usable with a forced
/// mask, because `qrcode` does not report the mask it picks automatically.
struct QrcodeEncoder;

impl QrEncoder for QrcodeEncoder {
    fn encode(&self, data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError> {
        let plan: EncodePlan = plan(data, options)?;
        let mask = plan
            .mask
            .ok_or_else(|| EncodeError::Internal("reference needs a forced mask".into()))?;
        let version = qrcode::Version::Normal(i16::from(plan.version.number()));
        let ec = match plan.ec_level {
            EcLevel::L => qrcode::EcLevel::L,
            EcLevel::M => qrcode::EcLevel::M,
            EcLevel::Q => qrcode::EcLevel::Q,
            EcLevel::H => qrcode::EcLevel::H,
        };
        let internal = |e: qrcode::types::QrError| EncodeError::Internal(e.to_string());

        let mut bits = qrcode::bits::Bits::new(version);
        match plan.mode {
            Mode::Numeric => bits.push_numeric_data(data),
            Mode::Alphanumeric => bits.push_alphanumeric_data(data),
            Mode::Byte => bits.push_byte_data(data),
        }
        .map_err(internal)?;
        bits.push_terminator(ec).map_err(internal)?;
        let (codewords, ec_codewords) =
            qrcode::ec::construct_codewords(&bits.into_bytes(), version, ec).map_err(internal)?;

        let mut canvas = Canvas::new(version, ec);
        canvas.draw_all_functional_patterns();
        canvas.draw_data(&codewords, &ec_codewords);
        canvas.apply_mask(pattern(mask));
        let modules = canvas
            .into_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect();
        let matrix = Matrix::from_modules(plan.version.width(), modules)
            .map_err(|e| EncodeError::Internal(e.to_string()))?;
        QrCode::from_parts(matrix, &plan, mask)
    }
}

fn pattern(mask: Mask) -> MaskPattern {
    [
        MaskPattern::Checkerboard,
        MaskPattern::HorizontalLines,
        MaskPattern::VerticalLines,
        MaskPattern::DiagonalLines,
        MaskPattern::LargeCheckerboard,
        MaskPattern::Fields,
        MaskPattern::Diamonds,
        MaskPattern::Meadow,
    ][usize::from(mask.number())]
}

fn assert_backends_agree(data: &[u8], options: EncodeOptions) {
    let ours = FastQrEncoder.encode(data, &options).unwrap();
    let reference = QrcodeEncoder.encode(data, &options).unwrap();
    assert_eq!(ours, reference, "matrices differ for {} bytes", data.len());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn backends_agree_on_arbitrary_input(
        data in prop::collection::vec(any::<u8>(), 0..400),
        ec in prop::sample::select(EcLevel::ALL.to_vec()),
        mask in 0u8..=7,
    ) {
        let options = EncodeOptions::default().with_ec_level(ec).with_mask(Mask::new(mask));
        assert_backends_agree(&data, options);
    }

    #[test]
    fn backends_agree_on_numeric_and_alphanumeric(
        digits in "[0-9]{1,800}",
        alnum in "[0-9A-Z $%*+./:-]{1,500}",
        mask in 0u8..=7,
    ) {
        let options = EncodeOptions::default().with_mask(Mask::new(mask));
        assert_backends_agree(digits.as_bytes(), options);
        assert_backends_agree(alnum.as_bytes(), options);
    }
}

#[test]
fn backends_agree_at_version_40() {
    let data: Vec<u8> = (0..2_300u32)
        .map(|i| b"qr-cli/0123:"[(i % 12) as usize])
        .collect();
    for mask in Mask::all() {
        assert_backends_agree(&data, EncodeOptions::default().with_mask(Some(mask)));
    }
}

#[test]
fn reference_backend_reports_errors_through_the_trait() {
    let err = QrcodeEncoder
        .encode(b"abc", &EncodeOptions::default())
        .unwrap_err();
    assert!(matches!(err, EncodeError::Internal(_)));
}

/// The smallest version `qrcode` picks with its optimal mixed-mode
/// segmentation, at the same error correction level.
fn qrcode_segmented_version(data: &[u8], ec: qrcode::EcLevel) -> u8 {
    match qrcode::QrCode::with_error_correction_level(data, ec)
        .unwrap()
        .version()
    {
        qrcode::Version::Normal(v) => u8::try_from(v).unwrap(),
        qrcode::Version::Micro(_) => unreachable!("Micro QR is never chosen automatically"),
    }
}

/// Known-answer table for the cost of single-mode auto (`plan()` picks one
/// mode for the whole input) against `qrcode`'s mixed-mode segmentation, at
/// EC level M. This is the symbol-size trade-off recorded in DECISIONS.md
/// section 1. A change to segmentation or to the backend must update this
/// table deliberately.
#[test]
fn segmentation_gap_against_qrcode_is_pinned() {
    let digits = |n: usize| "0123456789".repeat(n / 10 + 1)[..n].to_owned();
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
    let mut actual = Vec::new();
    for (data, _, _) in &cases {
        let ours = qr_core::encode(data.as_bytes(), &EncodeOptions::default())
            .unwrap()
            .version()
            .number();
        let reference = qrcode_segmented_version(data.as_bytes(), qrcode::EcLevel::M);
        actual.push((data.clone(), ours, reference));
    }
    assert_eq!(actual, cases);

    // The capacity ceiling drops for mixed data too: a short prefix on 5,000
    // digits is over the byte limit (2,953 at L) but fits qrcode's numeric
    // segment at version 40.
    let long = format!("ref:{}", digits(5_000));
    let options = EncodeOptions::default().with_ec_level(EcLevel::L);
    assert!(matches!(
        qr_core::encode(long.as_bytes(), &options),
        Err(EncodeError::DataTooLong(_))
    ));
    assert!(qrcode_segmented_version(long.as_bytes(), qrcode::EcLevel::L) <= 40);
}

/// `qrcode`'s automatic mask on the same plan (same version and byte mode),
/// for comparison with the production backend's automatic mask.
fn qrcode_best_mask_matrix(data: &[u8], plan: EncodePlan) -> Vec<bool> {
    let version = qrcode::Version::Normal(i16::from(plan.version.number()));
    let ec = qrcode::EcLevel::M;
    let mut bits = qrcode::bits::Bits::new(version);
    bits.push_byte_data(data).unwrap();
    bits.push_terminator(ec).unwrap();
    let (codewords, ec_codewords) =
        qrcode::ec::construct_codewords(&bits.into_bytes(), version, ec).unwrap();
    let mut canvas = Canvas::new(version, ec);
    canvas.draw_all_functional_patterns();
    canvas.draw_data(&codewords, &ec_codewords);
    canvas
        .apply_best_mask()
        .into_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect()
}

/// How often the production backend's automatic mask matches `qrcode`'s, on
/// 400 seeded pseudo-random byte inputs (0..600 bytes, EC level M, byte mode).
/// Both choices decode; they differ because `fast_qr` scores masks before the
/// format information is drawn and resets its run penalty at function
/// modules. Pinned so that a backend change shows up as a deliberate diff.
#[test]
fn automatic_mask_agreement_with_qrcode_is_pinned() {
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
    let mut same = 0;
    for _ in 0..400 {
        let len = usize::try_from(next() % 600).unwrap();
        let data: Vec<u8> = (0..len).map(|_| next().to_le_bytes()[0]).collect();
        let plan = plan(&data, &options).unwrap();
        let ours = FastQrEncoder.encode(&data, &options).unwrap();
        if ours.matrix().modules() == qrcode_best_mask_matrix(&data, plan).as_slice() {
            same += 1;
        }
    }
    eprintln!("auto mask agreement: {same}/400");
    assert_eq!(same, 273, "automatic-mask agreement with qrcode changed");
}
