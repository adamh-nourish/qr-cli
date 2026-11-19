//! Turning data into a [`QrCode`].
//!
//! Encoding is split in two so that backends stay interchangeable:
//!
//! 1. [`plan`] validates the data against the requested mode and picks the
//!    version. It owns every user-facing decision, including the exact
//!    capacity error, so all backends report limits identically.
//! 2. A [`QrEncoder`] backend builds the module matrix for that plan.
//!
//! [`encode`] runs both with the default backend, [`FastQrEncoder`].

mod fast;

pub use self::fast::FastQrEncoder;

use crate::capacity::{max_chars, smallest_fitting_version};
use crate::error::{CapacityError, EncodeError};
use crate::matrix::Matrix;
use crate::options::{EcLevel, EncodeOptions, Mask, Mode, ModeChoice, Version};

/// Something that turns bytes into a QR code symbol.
///
/// Implementations must be deterministic: the same data and options always
/// produce the same symbol. `Send + Sync` lets one encoder serve a batch
/// across threads.
pub trait QrEncoder: Send + Sync {
    /// Encodes `data` as a single QR code symbol.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::DataTooLong`] when the data does not fit the
    /// largest allowed version, and [`EncodeError::InvalidCharacter`] when a
    /// fixed mode cannot represent the data.
    fn encode(&self, data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError>;
}

/// The concrete parameters for one symbol, as chosen by [`plan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EncodePlan {
    /// The data mode, never automatic.
    pub mode: Mode,
    /// The symbol version, the smallest that fits within the options.
    pub version: Version,
    /// The error correction level.
    pub ec_level: EcLevel,
    /// A forced mask, or `None` to let the backend choose one.
    ///
    /// The backend's choice is its own penalty heuristic and is not
    /// guaranteed to be the ISO/IEC 18004 minimum: `fast_qr` scores masks
    /// before drawing format information and does not count runs through
    /// function patterns. Every mask decodes; see DECISIONS.md section 1.
    pub mask: Option<Mask>,
}

/// Validates `data` against `options` and chooses the mode and version.
///
/// Automatic mode picks the narrowest single mode that covers all the data
/// ([`Mode::narrowest_for`]); mixed-mode segmentation is not used.
///
/// # Errors
///
/// [`EncodeError::InvalidCharacter`] if a fixed mode cannot represent a
/// byte, and [`EncodeError::DataTooLong`] with the exact limit for the mode,
/// error correction level and largest allowed version.
///
/// ```
/// use qr_core::{EncodeError, EncodeOptions, EcLevel, Mode, plan};
///
/// let p = plan(b"HELLO WORLD", &EncodeOptions::default()).unwrap();
/// assert_eq!((p.mode, p.version.number()), (Mode::Alphanumeric, 1));
///
/// let too_long = vec![b'a'; 2_954];
/// let options = EncodeOptions::default().with_ec_level(EcLevel::L);
/// match plan(&too_long, &options) {
///     Err(EncodeError::DataTooLong(e)) => assert_eq!((e.needed, e.max), (2_954, 2_953)),
///     other => panic!("unexpected {other:?}"),
/// }
/// ```
pub fn plan(data: &[u8], options: &EncodeOptions) -> Result<EncodePlan, EncodeError> {
    let mode = match options.mode {
        ModeChoice::Auto => Mode::narrowest_for(data),
        ModeChoice::Fixed(mode) => {
            if let Some(offset) = data.iter().position(|&b| !mode.accepts(b)) {
                return Err(EncodeError::InvalidCharacter {
                    mode,
                    offset,
                    byte: data[offset],
                });
            }
            mode
        }
    };
    let ec_level = options.ec_level;
    let (min, max) = (options.version.min(), options.version.max());
    let version =
        smallest_fitting_version(mode, ec_level, data.len(), min, max).ok_or(CapacityError {
            needed: data.len(),
            max: max_chars(mode, max, ec_level),
            mode,
            ec_level,
            version: max,
        })?;
    Ok(EncodePlan {
        mode,
        version,
        ec_level,
        mask: options.mask,
    })
}

/// Encodes `data` with the default encoder.
///
/// # Errors
///
/// See [`QrEncoder::encode`].
///
/// ```
/// use qr_core::{EncodeOptions, encode};
/// let code = encode(b"https://example.com", &EncodeOptions::default()).unwrap();
/// assert_eq!(code.version().number(), 2);
/// assert_eq!(code.matrix().width(), 25);
/// ```
pub fn encode(data: &[u8], options: &EncodeOptions) -> Result<QrCode, EncodeError> {
    FastQrEncoder.encode(data, options)
}

/// An encoded symbol: the module matrix plus the parameters it was built
/// with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QrCode {
    matrix: Matrix,
    version: Version,
    ec_level: EcLevel,
    mode: Mode,
    mask: Mask,
}

impl QrCode {
    /// Assembles a symbol from a backend's matrix and the plan it followed.
    ///
    /// # Errors
    ///
    /// [`EncodeError::Internal`] if the matrix width does not match the
    /// planned version, which would mean the backend ignored the plan.
    pub fn from_parts(matrix: Matrix, plan: &EncodePlan, mask: Mask) -> Result<Self, EncodeError> {
        if matrix.width() != plan.version.width() {
            return Err(EncodeError::Internal(format!(
                "backend produced a {w}x{w} matrix for version {v}",
                w = matrix.width(),
                v = plan.version
            )));
        }
        if plan.mask.is_some_and(|forced| forced != mask) {
            return Err(EncodeError::Internal(format!(
                "backend used mask {mask} instead of the forced mask"
            )));
        }
        Ok(Self {
            matrix,
            version: plan.version,
            ec_level: plan.ec_level,
            mode: plan.mode,
            mask,
        })
    }

    /// The module grid, without a quiet zone.
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }

    /// Consumes the symbol, returning its module grid.
    pub fn into_matrix(self) -> Matrix {
        self.matrix
    }

    /// The symbol version.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The error correction level.
    pub fn ec_level(&self) -> EcLevel {
        self.ec_level
    }

    /// The data mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The mask pattern that was applied.
    pub fn mask(&self) -> Mask {
        self.mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::VersionChoice;

    fn v(n: u8) -> Version {
        Version::new(n).unwrap()
    }

    #[test]
    fn auto_mode_picks_the_narrowest_mode() {
        let o = EncodeOptions::default();
        assert_eq!(plan(b"12345", &o).unwrap().mode, Mode::Numeric);
        assert_eq!(plan(b"AB-12", &o).unwrap().mode, Mode::Alphanumeric);
        assert_eq!(plan(b"ab-12", &o).unwrap().mode, Mode::Byte);
    }

    #[test]
    fn fixed_mode_rejects_the_first_bad_byte() {
        let o = EncodeOptions::default().with_mode(ModeChoice::Fixed(Mode::Numeric));
        assert_eq!(
            plan(b"12a4b", &o),
            Err(EncodeError::InvalidCharacter {
                mode: Mode::Numeric,
                offset: 2,
                byte: b'a'
            })
        );
        let o = o.with_mode(ModeChoice::Fixed(Mode::Byte));
        assert_eq!(plan(b"12a4b", &o).unwrap().mode, Mode::Byte);
    }

    #[test]
    fn version_choice_is_honoured() {
        let base = EncodeOptions::default();
        assert_eq!(plan(b"x", &base).unwrap().version, v(1));
        let at_least = base.with_version(VersionChoice::AtLeast(v(10)));
        assert_eq!(plan(b"x", &at_least).unwrap().version, v(10));
        let exact = base.with_version(VersionChoice::Exact(v(3)));
        assert_eq!(plan(b"x", &exact).unwrap().version, v(3));
        // AtLeast still grows past the minimum when the data needs it.
        let grows = base.with_version(VersionChoice::AtLeast(v(2)));
        assert_eq!(plan(&[b'x'; 100], &grows).unwrap().version, v(6));
    }

    #[test]
    fn exact_version_capacity_error_reports_that_version() {
        let o = EncodeOptions::default().with_version(VersionChoice::Exact(v(1)));
        let err = plan(&[b'x'; 15], &o).unwrap_err();
        assert_eq!(
            err,
            EncodeError::DataTooLong(CapacityError {
                needed: 15,
                max: 14,
                mode: Mode::Byte,
                ec_level: EcLevel::M,
                version: v(1),
            })
        );
    }

    #[test]
    fn from_parts_rejects_a_backend_that_ignored_the_plan() {
        let plan = EncodePlan {
            mode: Mode::Byte,
            version: v(1),
            ec_level: EcLevel::M,
            mask: Some(Mask::new(1).unwrap()),
        };
        let small = Matrix::from_modules(2, vec![false; 4]).unwrap();
        assert!(matches!(
            QrCode::from_parts(small, &plan, Mask::new(1).unwrap()),
            Err(EncodeError::Internal(_))
        ));
        let right_size = Matrix::from_modules(21, vec![false; 441]).unwrap();
        assert!(matches!(
            QrCode::from_parts(right_size.clone(), &plan, Mask::new(2).unwrap()),
            Err(EncodeError::Internal(_))
        ));
        let code = QrCode::from_parts(right_size, &plan, Mask::new(1).unwrap()).unwrap();
        assert_eq!(code.into_matrix().width(), 21);
    }

    #[test]
    fn encode_reports_the_parameters_used() {
        let o = EncodeOptions::default()
            .with_ec_level(EcLevel::Q)
            .with_mask(Mask::new(5));
        let code = encode(b"HELLO", &o).unwrap();
        assert_eq!(code.ec_level(), EcLevel::Q);
        assert_eq!(code.mode(), Mode::Alphanumeric);
        assert_eq!(code.mask(), Mask::new(5).unwrap());
        assert_eq!(code.version(), v(1));
        assert_eq!(code.matrix().width(), 21);
    }

    /// Single-mode auto costs a version on mixed data: `qrcode`'s segmented
    /// encoding fits this in version 1 (see `tests/differential.rs`). Pinned
    /// so a change to segmentation or the backend is a deliberate diff.
    #[test]
    fn mixed_data_is_encoded_in_a_single_mode() {
        let code = encode(b"tel:+441234567890", &EncodeOptions::default()).unwrap();
        assert_eq!((code.mode(), code.version()), (Mode::Byte, v(2)));
    }

    /// Pins the backend's automatic mask for empty data at version 40, the
    /// case where it differed from `qrcode` in the PR 2 bake-off.
    #[test]
    fn automatic_mask_for_empty_data_at_version_40_is_pinned() {
        let o = EncodeOptions::default().with_version(VersionChoice::Exact(v(40)));
        let code = encode(b"", &o).unwrap();
        assert_eq!(code.mask().number(), 4);
    }
}
