//! The production backend, built on the `qrcode` crate.
//!
//! The owner chose `qrcode` over `fast_qr` after the PR 2 bake-off (see
//! DECISIONS.md section 1): its lowest-penalty mask follows ISO/IEC 18004,
//! and the segment-aware [`plan`] it can be fed gives smaller symbols than
//! single-mode encoding for mixed data such as phone numbers and digit-heavy
//! references. `fast_qr` was about 12x faster to encode, which only matters
//! for large batches of version 20+ codes.
//!
//! The backend encodes exactly the segments and version the plan chose, so
//! sizing and capacity errors never depend on the crate. `qrcode` does not
//! report the mask it picks, so the mask is read back from the format
//! information in the finished matrix ([`format_info::read`]).

use qrcode::bits::Bits;
use qrcode::canvas::{Canvas, MaskPattern};
use qrcode::types::QrError;

use crate::encode::{EncodePlan, QrCode, QrEncoder, format_info, plan};
use crate::error::EncodeError;
use crate::matrix::Matrix;
use crate::options::{EcLevel, EncodeOptions, Mask, Mode};

/// Encoder backed by the `qrcode` crate. This is the default encoder.
#[derive(Debug, Clone, Copy, Default)]
pub struct QrcodeEncoder;

impl QrEncoder for QrcodeEncoder {
    fn encode(&self, data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError> {
        let plan = plan(data, options)?;
        encode_plan(data, &plan)
    }
}

fn internal(err: QrError) -> EncodeError {
    EncodeError::Internal(format!("qrcode rejected a planned symbol: {err}"))
}

/// Builds the symbol `plan` describes for `data`.
fn encode_plan(data: &[u8], plan: &EncodePlan) -> Result<QrCode, EncodeError> {
    let version = qrcode::Version::Normal(i16::from(plan.version.number()));
    let ec = ec_level(plan.ec_level);

    let mut bits = Bits::new(version);
    for segment in &plan.segments {
        let chunk = data
            .get(segment.range())
            .filter(|chunk| chunk.iter().all(|&b| segment.mode.accepts(b)))
            .ok_or_else(|| {
                EncodeError::Internal(format!("segment {segment:?} does not fit the data"))
            })?;
        match segment.mode {
            Mode::Numeric => bits.push_numeric_data(chunk),
            Mode::Alphanumeric => bits.push_alphanumeric_data(chunk),
            Mode::Byte => bits.push_byte_data(chunk),
        }
        .map_err(internal)?;
    }
    bits.push_terminator(ec).map_err(internal)?;

    let colors = match plan.mask {
        // `with_bits` runs `qrcode`'s lowest-penalty mask search.
        None => qrcode::QrCode::with_bits(bits, ec)
            .map_err(internal)?
            .into_colors(),
        Some(mask) => {
            let (data_codewords, ec_codewords) =
                qrcode::ec::construct_codewords(&bits.into_bytes(), version, ec)
                    .map_err(internal)?;
            let mut canvas = Canvas::new(version, ec);
            canvas.draw_all_functional_patterns();
            canvas.draw_data(&data_codewords, &ec_codewords);
            canvas.apply_mask(pattern(mask));
            canvas.into_colors()
        }
    };
    let modules = colors
        .into_iter()
        .map(|color| color == qrcode::Color::Dark)
        .collect();
    let matrix = Matrix::from_modules(plan.version.width(), modules)
        .map_err(|err| EncodeError::Internal(err.to_string()))?;

    let (level, mask) = format_info::read(&matrix)
        .ok_or_else(|| EncodeError::Internal("qrcode drew no valid format information".into()))?;
    if level != plan.ec_level {
        return Err(EncodeError::Internal(format!(
            "qrcode drew error correction level {level} instead of {}",
            plan.ec_level
        )));
    }
    QrCode::from_parts(matrix, plan, mask)
}

const fn ec_level(level: EcLevel) -> qrcode::EcLevel {
    match level {
        EcLevel::L => qrcode::EcLevel::L,
        EcLevel::M => qrcode::EcLevel::M,
        EcLevel::Q => qrcode::EcLevel::Q,
        EcLevel::H => qrcode::EcLevel::H,
    }
}

/// `qrcode`'s mask patterns, in ISO/IEC 18004 number order.
const PATTERNS: [MaskPattern; 8] = [
    MaskPattern::Checkerboard,
    MaskPattern::HorizontalLines,
    MaskPattern::VerticalLines,
    MaskPattern::DiagonalLines,
    MaskPattern::LargeCheckerboard,
    MaskPattern::Fields,
    MaskPattern::Diamonds,
    MaskPattern::Meadow,
];

fn pattern(mask: Mask) -> MaskPattern {
    PATTERNS[usize::from(mask.number())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::Segment;
    use crate::options::Version;

    #[test]
    fn mask_table_is_in_number_order() {
        for mask in Mask::all() {
            assert_eq!(pattern(mask) as u8, mask.number());
        }
    }

    fn bogus_plan(segments: Vec<Segment>, version: u8, mask: Option<Mask>) -> EncodePlan {
        EncodePlan {
            segments,
            version: Version::new(version).unwrap(),
            ec_level: EcLevel::H,
            mask,
        }
    }

    #[test]
    fn a_plan_that_does_not_fit_is_an_internal_error_not_a_panic() {
        let data = [b'x'; 100];
        let segments = vec![Segment {
            mode: Mode::Byte,
            start: 0,
            end: 100,
        }];
        for mask in [None, Mask::new(2)] {
            let err = encode_plan(&data, &bogus_plan(segments.clone(), 1, mask)).unwrap_err();
            assert!(matches!(err, EncodeError::Internal(_)), "{err:?}");
        }
    }

    #[test]
    fn segments_that_do_not_match_the_data_are_rejected() {
        // Out of range.
        let out_of_range = vec![Segment {
            mode: Mode::Byte,
            start: 0,
            end: 5,
        }];
        let err = encode_plan(b"abc", &bogus_plan(out_of_range, 5, None)).unwrap_err();
        assert!(matches!(err, EncodeError::Internal(_)), "{err:?}");
        // A letter in a numeric segment would underflow inside `qrcode`.
        let wrong_mode = vec![Segment {
            mode: Mode::Numeric,
            start: 0,
            end: 3,
        }];
        let err = encode_plan(b"1a3", &bogus_plan(wrong_mode, 5, None)).unwrap_err();
        assert!(matches!(err, EncodeError::Internal(_)), "{err:?}");
    }

    #[test]
    fn the_reported_mask_is_the_one_drawn() {
        for ec in EcLevel::ALL {
            for mask in Mask::all() {
                let options = EncodeOptions::default()
                    .with_ec_level(ec)
                    .with_mask(Some(mask));
                let code = QrcodeEncoder.encode(b"mask", &options).unwrap();
                assert_eq!((code.ec_level(), code.mask()), (ec, mask));
                assert_eq!(format_info::read(code.matrix()), Some((ec, mask)));
            }
            // The automatic mask is read back at every level too.
            let auto = QrcodeEncoder
                .encode(b"mask", &EncodeOptions::default().with_ec_level(ec))
                .unwrap();
            assert_eq!(format_info::read(auto.matrix()), Some((ec, auto.mask())));
        }
    }
}
