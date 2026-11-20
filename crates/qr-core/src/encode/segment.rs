//! Mixed-mode segmentation: splitting data into runs that are each packed in
//! the narrowest mode that represents them.
//!
//! The algorithm is the one the `qrcode` crate uses (`optimize::Parser` and
//! `optimize::Optimizer`), so automatic sizing matches `qrcode` exactly:
//!
//! 1. Split the data into maximal runs of one character class: digits,
//!    other alphanumeric characters, and everything else.
//! 2. Walk the runs from left to right and merge each run into the previous
//!    segment (widening its mode) whenever the merged segment is no longer
//!    than the two separately. Mode and count headers make short runs
//!    expensive, so this folds them into their neighbours.
//!
//! The merge decision depends on the width of the character count field,
//! which changes at versions 10 and 27, so segmentation is computed per
//! version class.
//!
//! One deliberate difference from `qrcode`: Kanji mode is never used.
//! `qrcode` packs any byte pair that looks like Shift JIS into Kanji mode,
//! which scanners then decode as Shift JIS, so UTF-8 text such as Japanese
//! would not round-trip. Here every byte outside the alphanumeric set is in
//! byte mode.

use std::ops::Range;

use crate::capacity::segment_bits;
use crate::options::{Mode, Version};

/// A run of input bytes packed in one [`Mode`].
///
/// Segments produced by [`crate::plan`] are contiguous, in input order and
/// cover the whole input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Segment {
    /// The mode the bytes are packed in.
    pub mode: Mode,
    /// Offset of the first byte in the input.
    pub start: usize,
    /// Offset one past the last byte in the input.
    pub end: usize,
}

impl Segment {
    /// The input byte range this segment covers.
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    /// Number of characters (bytes) in the segment.
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the segment holds no characters.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bits this segment takes in a symbol of `version`, including its mode
    /// indicator and character count indicator.
    pub fn bits(&self, version: Version) -> usize {
        segment_bits(self.mode, version, self.len())
    }
}

/// The character class of a byte: the narrowest mode that can hold it.
fn class(byte: u8) -> Mode {
    if Mode::Numeric.accepts(byte) {
        Mode::Numeric
    } else if Mode::Alphanumeric.accepts(byte) {
        Mode::Alphanumeric
    } else {
        Mode::Byte
    }
}

/// Splits `data` into maximal runs of a single character class.
pub(crate) fn runs(data: &[u8]) -> Vec<Segment> {
    let mut runs: Vec<Segment> = Vec::new();
    for (offset, &byte) in data.iter().enumerate() {
        let mode = class(byte);
        match runs.last_mut() {
            Some(last) if last.mode == mode => last.end = offset + 1,
            _ => runs.push(Segment {
                mode,
                start: offset,
                end: offset + 1,
            }),
        }
    }
    runs
}

/// Greedily merges adjacent `runs` for a symbol of `version` (see the
/// module documentation). Only the version class matters.
pub(crate) fn optimize(runs: &[Segment], version: Version) -> Vec<Segment> {
    let mut iter = runs.iter().copied();
    let Some(mut last) = iter.next() else {
        return Vec::new();
    };
    let mut last_bits = last.bits(version);
    let mut out = Vec::new();
    for segment in iter {
        let bits = segment.bits(version);
        let merged = Segment {
            mode: last.mode.max(segment.mode),
            start: last.start,
            end: segment.end,
        };
        let merged_bits = merged.bits(version);
        if last_bits + bits >= merged_bits {
            last = merged;
            last_bits = merged_bits;
        } else {
            out.push(last);
            last = segment;
            last_bits = bits;
        }
    }
    out.push(last);
    out
}

/// Total bits of `segments` in a symbol of `version`, excluding the
/// terminator and padding.
pub(crate) fn total_bits(segments: &[Segment], version: Version) -> usize {
    segments.iter().map(|s| s.bits(version)).sum()
}

/// The widest mode among `segments`, or [`Mode::Numeric`] if there are
/// none (empty input), matching [`Mode::narrowest_for`].
pub(crate) fn widest_mode(segments: &[Segment]) -> Mode {
    segments
        .iter()
        .map(|s| s.mode)
        .max()
        .unwrap_or(Mode::Numeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(mode: Mode, start: usize, end: usize) -> Segment {
        Segment { mode, start, end }
    }

    fn v(n: u8) -> Version {
        Version::new(n).unwrap()
    }

    #[test]
    fn runs_split_on_class_changes() {
        assert_eq!(runs(b""), vec![]);
        assert_eq!(
            runs(b"ABC123abcd"),
            vec![
                seg(Mode::Alphanumeric, 0, 3),
                seg(Mode::Numeric, 3, 6),
                seg(Mode::Byte, 6, 10),
            ]
        );
        // Non-ASCII bytes are byte mode, never Kanji.
        assert_eq!(
            runs("\u{3042}1".as_bytes()),
            vec![seg(Mode::Byte, 0, 3), seg(Mode::Numeric, 3, 4)]
        );
    }

    /// The `qrcode` crate's own parser test (`test_parse_1`), which has no
    /// Kanji, gives the same runs.
    #[test]
    fn runs_match_the_qrcode_parser_example() {
        assert_eq!(
            runs(b"01049123451234591597033130128%10ABC123"),
            vec![
                seg(Mode::Numeric, 0, 29),
                seg(Mode::Alphanumeric, 29, 30),
                seg(Mode::Numeric, 30, 32),
                seg(Mode::Alphanumeric, 32, 35),
                seg(Mode::Numeric, 35, 38),
            ]
        );
    }

    #[test]
    fn short_runs_are_merged_into_their_neighbours() {
        // A two-digit run inside text costs more as its own segment.
        let data = b"abc12def";
        assert_eq!(optimize(&runs(data), v(1)), vec![seg(Mode::Byte, 0, 8)]);
        // A long digit run is worth its own numeric segment.
        let data = b"tel:+441234567890";
        assert_eq!(
            optimize(&runs(data), v(1)),
            vec![seg(Mode::Byte, 0, 5), seg(Mode::Numeric, 5, 17)]
        );
    }

    #[test]
    fn optimize_of_nothing_is_nothing() {
        assert_eq!(optimize(&[], v(1)), vec![]);
        assert_eq!(widest_mode(&[]), Mode::Numeric);
    }

    #[test]
    fn segment_accessors_and_bits() {
        let s = seg(Mode::Numeric, 2, 10);
        assert_eq!((s.range(), s.len(), s.is_empty()), (2..10, 8, false));
        // 4 + 10 + 2 * 10 + 7 bits, as in `capacity::segment_bits`.
        assert_eq!(s.bits(v(1)), 41);
        assert!(seg(Mode::Byte, 3, 3).is_empty());
        let both = [s, seg(Mode::Byte, 10, 11)];
        assert_eq!(total_bits(&both, v(1)), 41 + 20);
        assert_eq!(widest_mode(&both), Mode::Byte);
    }

    #[test]
    fn merging_depends_on_the_version_class() {
        // Count fields widen at versions 10 and 27, which changes when a
        // run is worth its own segment.
        let a = b"a123456789b";
        assert_eq!(
            optimize(&runs(a), v(1)),
            vec![
                seg(Mode::Byte, 0, 1),
                seg(Mode::Numeric, 1, 10),
                seg(Mode::Byte, 10, 11)
            ]
        );
        let differs = (1..40).any(|n| {
            let data = format!("x{}Y{}", "7".repeat(n), "Z".repeat(n));
            optimize(&runs(data.as_bytes()), v(1)) != optimize(&runs(data.as_bytes()), v(27))
        });
        assert!(differs);
    }
}
