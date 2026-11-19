//! Differential test against the `qrcode` crate, implemented as a second
//! [`QrEncoder`] backend. With the same plan and a forced mask, the two
//! independent encoders must produce bit-identical matrices. This also
//! proves the trait is enough to plug in an alternative backend.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point in this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use qr_core::{
    EcLevel, EncodeError, EncodeOptions, EncodePlan, FastQrEncoder, Mask, Matrix, Mode, QrCode,
    QrEncoder, plan,
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
