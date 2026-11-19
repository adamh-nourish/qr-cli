//! The production backend, built on the `fast_qr` crate.
//!
//! `fast_qr` was chosen over `qrcode` by the PR 2 bake-off: on the default
//! path (automatic mask) it is about 11x faster at versions 20 and 40, all of
//! its output round-trips through `rqrr`, and with a forced mask its matrices
//! are bit-identical to `qrcode`'s. With the automatic mask the two crates
//! often pick different (equally decodable) masks, and auto mode here does not
//! segment mixed data, so some payloads need a larger version. Both
//! trade-offs are measured in `tests/differential.rs` and recorded in
//! `DECISIONS.md` section 1.

use fast_qr::{ECL, QRBuilder};

use crate::encode::{EncodePlan, QrCode, QrEncoder, plan};
use crate::error::EncodeError;
use crate::matrix::Matrix;
use crate::options::{EcLevel, EncodeOptions, Mask, Mode, Version};

/// Encoder backed by the `fast_qr` crate. This is the default encoder.
#[derive(Debug, Clone, Copy, Default)]
pub struct FastQrEncoder;

impl QrEncoder for FastQrEncoder {
    fn encode(&self, data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError> {
        let plan = plan(data, options)?;
        encode_plan(data, plan)
    }
}

fn encode_plan(data: &[u8], plan: EncodePlan) -> Result<QrCode, EncodeError> {
    let mut builder = QRBuilder::new(data);
    builder
        .ecl(ecl(plan.ec_level))
        .mode(mode(plan.mode))
        .version(version(plan.version));
    if let Some(mask) = plan.mask {
        builder.mask(fast_mask(mask));
    }
    let code = builder.build().map_err(|err| {
        EncodeError::Internal(format!("fast_qr rejected a planned symbol: {err}"))
    })?;

    let width = code.size;
    let modules = code
        .data
        .get(..width.saturating_mul(width))
        .ok_or_else(|| EncodeError::Internal(format!("fast_qr returned an invalid width {width}")))?
        .iter()
        .map(|module| module.value())
        .collect();
    let matrix = Matrix::from_modules(width, modules)
        .map_err(|err| EncodeError::Internal(err.to_string()))?;
    let mask = code
        .mask
        .and_then(|m| Mask::new(m as u8))
        .ok_or_else(|| EncodeError::Internal("fast_qr did not report a mask".into()))?;
    QrCode::from_parts(matrix, &plan, mask)
}

const fn ecl(level: EcLevel) -> ECL {
    match level {
        EcLevel::L => ECL::L,
        EcLevel::M => ECL::M,
        EcLevel::Q => ECL::Q,
        EcLevel::H => ECL::H,
    }
}

const fn mode(mode: Mode) -> fast_qr::Mode {
    match mode {
        Mode::Numeric => fast_qr::Mode::Numeric,
        Mode::Alphanumeric => fast_qr::Mode::Alphanumeric,
        Mode::Byte => fast_qr::Mode::Byte,
    }
}

fn fast_mask(mask: Mask) -> fast_qr::Mask {
    use fast_qr::Mask as M;
    const MASKS: [M; 8] = [
        M::Checkerboard,
        M::HorizontalLines,
        M::VerticalLines,
        M::DiagonalLines,
        M::LargeCheckerboard,
        M::Fields,
        M::Diamonds,
        M::Meadow,
    ];
    MASKS[usize::from(mask.number())]
}

fn version(version: Version) -> fast_qr::Version {
    use fast_qr::Version as V;
    const VERSIONS: [V; 40] = [
        V::V01,
        V::V02,
        V::V03,
        V::V04,
        V::V05,
        V::V06,
        V::V07,
        V::V08,
        V::V09,
        V::V10,
        V::V11,
        V::V12,
        V::V13,
        V::V14,
        V::V15,
        V::V16,
        V::V17,
        V::V18,
        V::V19,
        V::V20,
        V::V21,
        V::V22,
        V::V23,
        V::V24,
        V::V25,
        V::V26,
        V::V27,
        V::V28,
        V::V29,
        V::V30,
        V::V31,
        V::V32,
        V::V33,
        V::V34,
        V::V35,
        V::V36,
        V::V37,
        V::V38,
        V::V39,
        V::V40,
    ];
    VERSIONS[usize::from(version.number() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_tables_are_in_number_order() {
        for mask in Mask::all() {
            assert_eq!(fast_mask(mask) as u8, mask.number());
        }
        for v in Version::MIN.up_to_max() {
            assert_eq!(version(v) as usize + 1, usize::from(v.number()));
        }
    }

    #[test]
    fn mismatched_plan_is_an_internal_error_not_a_panic() {
        // A plan that skips `plan()` and asks for too small a version.
        let bogus = EncodePlan {
            mode: Mode::Byte,
            version: Version::MIN,
            ec_level: EcLevel::H,
            mask: None,
        };
        let err = encode_plan(&[b'x'; 100], bogus).unwrap_err();
        assert!(matches!(err, EncodeError::Internal(_)), "{err:?}");
    }
}
