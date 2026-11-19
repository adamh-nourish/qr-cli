//! Exact symbol capacities (ISO/IEC 18004 section 7.4 and table 7).
//!
//! Capacity depends on the version, the error correction level and the data
//! mode. Everything here is computed from the number of data codewords per
//! version and level, so it is exact rather than an approximation, and it is
//! independent of the encoder backend.

use crate::options::{EcLevel, Mode, Version};

/// Data codewords per version (row, version 1 first) and error correction
/// level (column, `L, M, Q, H`). ISO/IEC 18004 table 7.
const DATA_CODEWORDS: [[u16; 4]; 40] = [
    [19, 16, 13, 9],
    [34, 28, 22, 16],
    [55, 44, 34, 26],
    [80, 64, 48, 36],
    [108, 86, 62, 46],
    [136, 108, 76, 60],
    [156, 124, 88, 66],
    [194, 154, 110, 86],
    [232, 182, 132, 100],
    [274, 216, 154, 122],
    [324, 254, 180, 140],
    [370, 290, 206, 158],
    [428, 334, 244, 180],
    [461, 365, 261, 197],
    [523, 415, 295, 223],
    [589, 453, 325, 253],
    [647, 507, 367, 283],
    [721, 563, 397, 313],
    [795, 627, 445, 341],
    [861, 669, 485, 385],
    [932, 714, 512, 406],
    [1006, 782, 568, 442],
    [1094, 860, 614, 464],
    [1174, 914, 664, 514],
    [1276, 1000, 718, 538],
    [1370, 1062, 754, 596],
    [1468, 1128, 808, 628],
    [1531, 1193, 871, 661],
    [1631, 1267, 911, 701],
    [1735, 1373, 985, 745],
    [1843, 1455, 1033, 793],
    [1955, 1541, 1115, 845],
    [2071, 1631, 1171, 901],
    [2191, 1725, 1231, 961],
    [2306, 1812, 1286, 986],
    [2434, 1914, 1354, 1054],
    [2566, 1992, 1426, 1096],
    [2702, 2102, 1502, 1142],
    [2812, 2216, 1582, 1222],
    [2956, 2334, 1666, 1276],
];

/// Bits in a mode indicator.
const MODE_INDICATOR_BITS: usize = 4;

/// Number of data bits available in a symbol of `version` at `ec`.
pub fn data_bits(version: Version, ec: EcLevel) -> usize {
    let row = DATA_CODEWORDS[usize::from(version.number() - 1)];
    usize::from(row[ec.index()]) * 8
}

/// Width of the character count indicator (ISO/IEC 18004 table 3).
pub fn count_indicator_bits(mode: Mode, version: Version) -> usize {
    let class = match version.number() {
        1..=9 => 0,
        10..=26 => 1,
        _ => 2,
    };
    let widths = match mode {
        Mode::Numeric => [10, 12, 14],
        Mode::Alphanumeric => [9, 11, 13],
        Mode::Byte => [8, 16, 16],
    };
    widths[class]
}

/// Bits needed for a single segment of `len` characters in `mode`, including
/// the mode indicator and character count indicator.
pub fn segment_bits(mode: Mode, version: Version, len: usize) -> usize {
    let payload = match mode {
        Mode::Numeric => 10 * (len / 3) + [0, 4, 7][len % 3],
        Mode::Alphanumeric => 11 * (len / 2) + 6 * (len % 2),
        Mode::Byte => 8 * len,
    };
    MODE_INDICATOR_BITS + count_indicator_bits(mode, version) + payload
}

/// Whether `len` characters in `mode` fit a symbol of `version` at `ec`.
pub fn fits(mode: Mode, version: Version, ec: EcLevel, len: usize) -> bool {
    len < (1 << count_indicator_bits(mode, version))
        && segment_bits(mode, version, len) <= data_bits(version, ec)
}

/// The largest number of characters (digits, alphanumeric characters or
/// bytes, depending on `mode`) that fit a symbol of `version` at `ec`.
///
/// ```
/// use qr_core::{EcLevel, Mode, Version, max_chars};
/// // The well-known upper limits of version 40-L.
/// assert_eq!(max_chars(Mode::Numeric, Version::MAX, EcLevel::L), 7089);
/// assert_eq!(max_chars(Mode::Alphanumeric, Version::MAX, EcLevel::L), 4296);
/// assert_eq!(max_chars(Mode::Byte, Version::MAX, EcLevel::L), 2953);
/// ```
pub fn max_chars(mode: Mode, version: Version, ec: EcLevel) -> usize {
    let available = data_bits(version, ec)
        .saturating_sub(MODE_INDICATOR_BITS + count_indicator_bits(mode, version));
    let by_bits = match mode {
        Mode::Numeric => {
            let groups = available / 10;
            let rest = available % 10;
            groups * 3 + usize::from(rest >= 4) + usize::from(rest >= 7)
        }
        Mode::Alphanumeric => {
            let pairs = available / 11;
            pairs * 2 + usize::from(available % 11 >= 6)
        }
        Mode::Byte => available / 8,
    };
    let by_count_field = (1 << count_indicator_bits(mode, version)) - 1;
    by_bits.min(by_count_field)
}

/// The smallest version in `min..=max` that fits `len` characters in `mode`.
pub fn smallest_fitting_version(
    mode: Mode,
    ec: EcLevel,
    len: usize,
    min: Version,
    max: Version,
) -> Option<Version> {
    min.up_to_max()
        .take_while(|v| *v <= max)
        .find(|v| fits(mode, *v, ec, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(n: u8) -> Version {
        Version::new(n).unwrap()
    }

    #[test]
    fn known_capacities_from_the_standard() {
        // (version, ec, numeric, alphanumeric, byte) from ISO/IEC 18004 table 7.
        let rows = [
            (1, EcLevel::L, 41, 25, 17),
            (1, EcLevel::M, 34, 20, 14),
            (1, EcLevel::Q, 27, 16, 11),
            (1, EcLevel::H, 17, 10, 7),
            (10, EcLevel::M, 513, 311, 213),
            (40, EcLevel::L, 7_089, 4_296, 2_953),
            (40, EcLevel::M, 5_596, 3_391, 2_331),
            (40, EcLevel::Q, 3_993, 2_420, 1_663),
            (40, EcLevel::H, 3_057, 1_852, 1_273),
        ];
        for (n, ec, numeric, alnum, byte) in rows {
            assert_eq!(max_chars(Mode::Numeric, v(n), ec), numeric, "v{n}-{ec}");
            assert_eq!(max_chars(Mode::Alphanumeric, v(n), ec), alnum, "v{n}-{ec}");
            assert_eq!(max_chars(Mode::Byte, v(n), ec), byte, "v{n}-{ec}");
        }
    }

    #[test]
    fn max_chars_is_the_exact_boundary_of_fits() {
        for version in Version::MIN.up_to_max() {
            for ec in EcLevel::ALL {
                for mode in [Mode::Numeric, Mode::Alphanumeric, Mode::Byte] {
                    let max = max_chars(mode, version, ec);
                    assert!(fits(mode, version, ec, max), "{mode} v{version}-{ec}");
                    assert!(!fits(mode, version, ec, max + 1), "{mode} v{version}-{ec}");
                }
            }
        }
    }

    #[test]
    fn count_indicator_widths_change_at_versions_10_and_27() {
        assert_eq!(count_indicator_bits(Mode::Numeric, v(9)), 10);
        assert_eq!(count_indicator_bits(Mode::Numeric, v(10)), 12);
        assert_eq!(count_indicator_bits(Mode::Numeric, v(27)), 14);
        assert_eq!(count_indicator_bits(Mode::Alphanumeric, v(26)), 11);
        assert_eq!(count_indicator_bits(Mode::Byte, v(9)), 8);
        assert_eq!(count_indicator_bits(Mode::Byte, v(40)), 16);
    }

    #[test]
    fn segment_bits_matches_hand_computed_values() {
        // "01234567" in numeric at v1: 4 + 10 + 10*2 + 7 = 41 bits.
        assert_eq!(segment_bits(Mode::Numeric, v(1), 8), 41);
        // "AC-42" in alphanumeric at v1: 4 + 9 + 11*2 + 6 = 41 bits.
        assert_eq!(segment_bits(Mode::Alphanumeric, v(1), 5), 41);
        assert_eq!(segment_bits(Mode::Byte, v(1), 0), 12);
    }

    #[test]
    fn smallest_fitting_version_respects_bounds() {
        assert_eq!(
            smallest_fitting_version(Mode::Byte, EcLevel::M, 14, Version::MIN, Version::MAX),
            Some(v(1))
        );
        assert_eq!(
            smallest_fitting_version(Mode::Byte, EcLevel::M, 15, Version::MIN, Version::MAX),
            Some(v(2))
        );
        assert_eq!(
            smallest_fitting_version(Mode::Byte, EcLevel::M, 1, v(7), Version::MAX),
            Some(v(7))
        );
        assert_eq!(
            smallest_fitting_version(Mode::Byte, EcLevel::M, 15, v(1), v(1)),
            None
        );
        assert_eq!(
            smallest_fitting_version(Mode::Byte, EcLevel::L, 2_954, Version::MIN, Version::MAX),
            None
        );
    }
}
