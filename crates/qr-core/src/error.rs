//! Errors returned by encoders.

use std::fmt;

use thiserror::Error;

use crate::capacity::{data_bits, max_chars};
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
/// Single-mode data (a fixed `--mode`, or automatic data that is one
/// segment) is measured in characters of [`CapacityError::mode`]: digits for
/// numeric, characters for alphanumeric and bytes for byte mode. Every
/// supported character is one input byte, so `needed` is then also the
/// input length in bytes.
///
/// Mixed data, which automatic mode splits into several segments, has no
/// single per-mode limit, so it is measured exactly in data bits: the bits
/// its segments take, headers included, against the bits the symbol holds.
/// See [`CapacityUnit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    /// Size of the input, in [`CapacityError::unit`].
    pub needed: usize,
    /// The most that fits, in [`CapacityError::unit`].
    pub max: usize,
    /// What `needed` and `max` count.
    pub unit: CapacityUnit,
    /// The mode the limit applies to; for mixed data, the widest segment
    /// mode.
    pub mode: Mode,
    /// The error correction level the limit applies to.
    pub ec_level: EcLevel,
    /// The largest version that was allowed.
    pub version: Version,
    /// The input length in bytes.
    pub bytes: usize,
}

/// What a [`CapacityError`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CapacityUnit {
    /// Characters of the error's single mode.
    Characters,
    /// Data bits of a segmented (mixed-mode) encoding, including mode and
    /// character count headers but not the terminator.
    Bits,
}

impl CapacityError {
    /// How much too long the input is, in [`CapacityError::unit`].
    pub fn excess(&self) -> usize {
        self.needed.saturating_sub(self.max)
    }

    /// The name of the unit `needed` and `max` are counted in, such as
    /// `"bytes"` or `"bits"`.
    pub const fn unit_name(&self) -> &'static str {
        match self.unit {
            CapacityUnit::Characters => self.mode.unit(),
            CapacityUnit::Bits => "bits",
        }
    }

    /// The limit, in the same unit, at the same version but error correction
    /// level `ec_level`. Useful for suggesting a lower level.
    ///
    /// ```
    /// use qr_core::{EcLevel, EncodeError, EncodeOptions, encode};
    ///
    /// let options = EncodeOptions::default().with_ec_level(EcLevel::H);
    /// let Err(EncodeError::DataTooLong(e)) = encode(&[b'a'; 1_300], &options) else {
    ///     unreachable!()
    /// };
    /// assert_eq!((e.max, e.max_at(EcLevel::L)), (1_273, 2_953));
    /// ```
    pub fn max_at(&self, ec_level: EcLevel) -> usize {
        match self.unit {
            CapacityUnit::Characters => max_chars(self.mode, self.version, ec_level),
            CapacityUnit::Bits => data_bits(self.version, ec_level),
        }
    }
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.unit {
            CapacityUnit::Characters => write!(
                f,
                "data too long: {needed} {unit} exceeds the maximum of {max} for {mode} mode \
                 at error correction level {ec} (version {version})",
                needed = self.needed,
                unit = self.unit_name(),
                max = self.max,
                mode = self.mode,
                ec = self.ec_level,
                version = self.version,
            ),
            CapacityUnit::Bits => write!(
                f,
                "data too long: {bytes} bytes of mixed data need {needed} bits, which exceeds \
                 the maximum of {max} bits at error correction level {ec} (version {version})",
                bytes = self.bytes,
                needed = self.needed,
                max = self.max,
                ec = self.ec_level,
                version = self.version,
            ),
        }
    }
}

impl std::error::Error for CapacityError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_error_message_names_every_dimension() {
        let err = CapacityError {
            needed: 3_214,
            max: 2_953,
            unit: CapacityUnit::Characters,
            mode: Mode::Byte,
            ec_level: EcLevel::L,
            version: Version::MAX,
            bytes: 3_214,
        };
        assert_eq!(
            err.to_string(),
            "data too long: 3214 bytes exceeds the maximum of 2953 for byte mode at error correction level L (version 40)"
        );
        assert_eq!(err.excess(), 261);
        assert_eq!(err.unit_name(), "bytes");
        assert_eq!(err.max_at(EcLevel::H), 1_273);
        let wrapped = EncodeError::from(err);
        assert_eq!(wrapped.to_string(), err.to_string());
    }

    #[test]
    fn mixed_data_capacity_error_is_in_bits() {
        let err = CapacityError {
            needed: 23_700,
            max: 23_648,
            unit: CapacityUnit::Bits,
            mode: Mode::Byte,
            ec_level: EcLevel::L,
            version: Version::MAX,
            bytes: 7_000,
        };
        assert_eq!(
            err.to_string(),
            "data too long: 7000 bytes of mixed data need 23700 bits, which exceeds the maximum of 23648 bits at error correction level L (version 40)"
        );
        assert_eq!((err.excess(), err.unit_name()), (52, "bits"));
        assert_eq!(err.max_at(EcLevel::M), 18_672);
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
