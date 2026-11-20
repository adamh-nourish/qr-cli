//! Format information (ISO/IEC 18004 section 7.9): the 15-bit codeword that
//! records the error correction level and mask pattern twice in every
//! symbol.
//!
//! The `qrcode` crate picks the lowest-penalty mask itself but does not say
//! which one it chose. Every symbol carries that choice in its format
//! information, so it is recovered from the finished matrix by
//! [`read`], the same way a scanner learns it.

use crate::matrix::Matrix;
use crate::options::{EcLevel, Mask};

/// The BCH(15, 5) generator polynomial `x^10 + x^8 + x^5 + x^4 + x^2 + x + 1`.
const GENERATOR: u16 = 0b101_0011_0111;

/// XOR pattern that keeps the codeword from being all light.
const XOR_MASK: u16 = 0b101_0100_0001_0010;

/// The two-bit error correction indicator (ISO/IEC 18004 table 12).
const fn ec_bits(level: EcLevel) -> u16 {
    match level {
        EcLevel::L => 0b01,
        EcLevel::M => 0b00,
        EcLevel::Q => 0b11,
        EcLevel::H => 0b10,
    }
}

/// The masked 15-bit format information codeword for `level` and `mask`.
pub(crate) const fn codeword(level: EcLevel, mask: Mask) -> u16 {
    let data = (ec_bits(level) << 3) | mask.number() as u16;
    let mut remainder = data << 10;
    let mut bit = 14;
    while bit >= 10 {
        if remainder & (1 << bit) != 0 {
            remainder ^= GENERATOR << (bit - 10);
        }
        bit -= 1;
    }
    ((data << 10) | remainder) ^ XOR_MASK
}

/// Module positions `(x, y)` of the first copy, most significant bit
/// first: along row 8 beside the top-left finder pattern, skipping the
/// timing pattern, then up column 8.
const TOP_LEFT: [(usize, usize); 15] = [
    (0, 8),
    (1, 8),
    (2, 8),
    (3, 8),
    (4, 8),
    (5, 8),
    (7, 8),
    (8, 8),
    (8, 7),
    (8, 5),
    (8, 4),
    (8, 3),
    (8, 2),
    (8, 1),
    (8, 0),
];

/// Reads the 15 bits of one copy, most significant bit first.
fn read_bits(matrix: &Matrix, at: impl Fn(usize) -> (usize, usize)) -> u16 {
    (0..15).fold(0, |acc, i| {
        let (x, y) = at(i);
        (acc << 1) | u16::from(matrix.is_dark(x, y))
    })
}

/// Recovers the error correction level and mask from the format information
/// of `matrix`.
///
/// Both copies must hold the same, exactly valid codeword: the matrix comes
/// straight from an encoder, so any disagreement is a bug, not damage to
/// correct. Returns `None` for a matrix too small to be a QR code symbol or
/// without valid format information.
pub(crate) fn read(matrix: &Matrix) -> Option<(EcLevel, Mask)> {
    let w = matrix.width();
    if w < 21 {
        return None;
    }
    let first = read_bits(matrix, |i| TOP_LEFT[i]);
    // The second copy runs up column 8 from the bottom edge (7 bits, beside
    // the bottom-left finder), then along row 8 to the right edge (8 bits,
    // under the top-right finder).
    let second = read_bits(matrix, |i| {
        if i < 7 {
            (8, w - 1 - i)
        } else {
            (w - 15 + i, 8)
        }
    });
    if first != second {
        return None;
    }
    EcLevel::ALL.into_iter().find_map(|level| {
        Mask::all()
            .find(|&mask| codeword(level, mask) == first)
            .map(|mask| (level, mask))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the format information for `level` and `mask` into a blank
    /// matrix of `width`, both copies.
    fn blank_with_format(width: usize, level: EcLevel, mask: Mask) -> Matrix {
        let word = codeword(level, mask);
        let mut modules = vec![false; width * width];
        for (i, &(x, y)) in TOP_LEFT.iter().enumerate() {
            let dark = word & (1 << (14 - i)) != 0;
            modules[y * width + x] = dark;
            let (x, y) = if i < 7 {
                (8, width - 1 - i)
            } else {
                (width - 15 + i, 8)
            };
            modules[y * width + x] = dark;
        }
        Matrix::from_modules(width, modules).unwrap()
    }

    #[test]
    fn codewords_match_the_standard() {
        // ISO/IEC 18004 table C.1, first entry of each level (mask 0) and
        // the last entry (H, mask 7).
        let m0 = Mask::new(0).unwrap();
        assert_eq!(codeword(EcLevel::L, m0), 0x77c4);
        assert_eq!(codeword(EcLevel::M, m0), 0x5412);
        assert_eq!(codeword(EcLevel::Q, m0), 0x355f);
        assert_eq!(codeword(EcLevel::H, m0), 0x1689);
        assert_eq!(codeword(EcLevel::H, Mask::new(7).unwrap()), 0x083b);
    }

    #[test]
    fn all_32_codewords_are_distinct() {
        let mut words: Vec<u16> = EcLevel::ALL
            .into_iter()
            .flat_map(|l| Mask::all().map(move |m| codeword(l, m)))
            .collect();
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), 32);
    }

    #[test]
    fn read_recovers_every_level_and_mask() {
        for width in [21, 177] {
            for level in EcLevel::ALL {
                for mask in Mask::all() {
                    let m = blank_with_format(width, level, mask);
                    assert_eq!(read(&m), Some((level, mask)), "{width} {level} {mask}");
                }
            }
        }
    }

    #[test]
    fn read_rejects_inconsistent_or_invalid_format_information() {
        let mask = Mask::new(3).unwrap();
        let good = blank_with_format(21, EcLevel::Q, mask);

        // Flip one bit of the second copy: the copies disagree.
        let mut modules = good.modules().to_vec();
        modules[8 * 21 + 20] = !modules[8 * 21 + 20];
        assert_eq!(read(&Matrix::from_modules(21, modules).unwrap()), None);

        // Flip the same bit in both copies: consistent, but not a codeword.
        let mut modules = good.modules().to_vec();
        modules[8 * 21] = !modules[8 * 21];
        modules[20 * 21 + 8] = !modules[20 * 21 + 8];
        assert_eq!(read(&Matrix::from_modules(21, modules).unwrap()), None);

        // Too small to be a symbol.
        assert_eq!(read(&Matrix::from_modules(1, vec![true]).unwrap()), None);
    }
}
