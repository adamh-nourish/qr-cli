//! Errors returned by encoders.

use thiserror::Error;

use crate::options::{EcLevel, Mode, Version};

/// Why data could not be encoded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// The data does not fit in the largest allowed symbol.
    #[error(transparent)]
    DataTooLong(#[from] CapacityError),
    /// A byte cannot be represented in the requested mode.
    #[error("byte 0x{byte:02x} at offset {offset} cannot be encoded in {mode} mode")]
    InvalidCharacter {
        /// The mode that was requested.
        mode: Mode,
        /// Offset of the first offending byte in the input.
        offset: usize,
        /// The offending byte.
        byte: u8,
    },
    /// The encoder backend produced something unusable. This is a bug.
    #[error("internal encoder error: {0}")]
    Internal(String),
}

/// The data is longer than the largest symbol allowed by the options.
///
/// Lengths are counted in the unit of [`CapacityError::mode`]: digits for
/// numeric, characters for alphanumeric and bytes for byte mode. Every
/// supported character is one input byte, so `needed` is also the input
/// length in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error(
    "data too long: {needed} {unit} exceeds the maximum of {max} for {mode} mode at error correction level {ec_level} (version {version})",
    unit = mode.unit()
)]
pub struct CapacityError {
    /// Characters in the input.
    pub needed: usize,
    /// The most characters that fit.
    pub max: usize,
    /// The mode the limit applies to.
    pub mode: Mode,
    /// The error correction level the limit applies to.
    pub ec_level: EcLevel,
    /// The largest version that was allowed.
    pub version: Version,
}

impl CapacityError {
    /// How many characters too many the input has.
    pub fn excess(&self) -> usize {
        self.needed.saturating_sub(self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_error_message_names_every_dimension() {
        let err = CapacityError {
            needed: 3_214,
            max: 2_953,
            mode: Mode::Byte,
            ec_level: EcLevel::L,
            version: Version::MAX,
        };
        assert_eq!(
            err.to_string(),
            "data too long: 3214 bytes exceeds the maximum of 2953 for byte mode at error correction level L (version 40)"
        );
        assert_eq!(err.excess(), 261);
        let wrapped = EncodeError::from(err);
        assert_eq!(wrapped.to_string(), err.to_string());
    }

    #[test]
    fn invalid_character_message_shows_offset_and_byte() {
        let err = EncodeError::InvalidCharacter {
            mode: Mode::Numeric,
            offset: 3,
            byte: b'x',
        };
        assert_eq!(
            err.to_string(),
            "byte 0x78 at offset 3 cannot be encoded in numeric mode"
        );
        assert_eq!(
            EncodeError::Internal("boom".into()).to_string(),
            "internal encoder error: boom"
        );
    }
}
