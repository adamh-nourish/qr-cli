//! Text renderers: Unicode half blocks, ASCII art and ANSI terminal output.
//!
//! The Unicode and ANSI renderers pack two module rows into one line of text
//! with the half-block characters `▀`, `▄` and `█`, so a symbol keeps its
//! square shape in a typical terminal cell (about twice as tall as it is
//! wide). The ASCII renderer draws each module as two characters on its own
//! line for the same reason.

use crate::matrix::Matrix;

/// The quiet zone, in modules, that ISO/IEC 18004 requires around a symbol.
pub const DEFAULT_MARGIN: usize = 4;

/// The largest quiet zone the renderers draw, in modules. Larger margins are
/// clamped to it, so a caller cannot make a rendering of unbounded size.
pub const MAX_MARGIN: usize = 100;

/// Selects the SGR colours for the ANSI renderer: black foreground on a
/// bright white background, so the code scans on dark terminal themes.
const ANSI_BLACK_ON_WHITE: &str = "\x1b[30;107m";
/// Resets every SGR attribute at the end of each ANSI line.
const ANSI_RESET: &str = "\x1b[0m";

/// The text flavours a symbol can be drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextFormat {
    /// Unicode half blocks, two module rows per line, with no escape
    /// sequences. Dark modules are drawn with ink, so the code reads
    /// correctly on a light background.
    Unicode,
    /// `##` for a dark module and two spaces for a light one, one module row
    /// per line. The output is pure ASCII.
    Ascii,
    /// Unicode half blocks wrapped in fixed ANSI colour sequences that force
    /// black modules on a white background, whatever the terminal theme.
    Ansi,
}

impl TextFormat {
    /// How many terminal columns each line of the rendering occupies.
    ///
    /// ```
    /// use qr_core::{Matrix, TextFormat, TextOptions};
    /// let m = Matrix::from_modules(1, vec![true]).unwrap();
    /// let options = TextOptions::default().with_margin(1);
    /// assert_eq!(TextFormat::Unicode.columns(&m, &options), 3);
    /// assert_eq!(TextFormat::Ascii.columns(&m, &options), 6);
    /// ```
    pub fn columns(self, matrix: &Matrix, options: &TextOptions) -> usize {
        let side = frame_side(matrix, options);
        match self {
            Self::Ascii => side.saturating_mul(2),
            Self::Unicode | Self::Ansi => side,
        }
    }

    /// How many lines the rendering has.
    ///
    /// ```
    /// use qr_core::{Matrix, TextFormat, TextOptions};
    /// let m = Matrix::from_modules(1, vec![true]).unwrap();
    /// let options = TextOptions::default().with_margin(1);
    /// assert_eq!(TextFormat::Unicode.lines(&m, &options), 2);
    /// assert_eq!(TextFormat::Ascii.lines(&m, &options), 3);
    /// ```
    pub fn lines(self, matrix: &Matrix, options: &TextOptions) -> usize {
        let side = frame_side(matrix, options);
        match self {
            Self::Ascii => side,
            Self::Unicode | Self::Ansi => side.div_ceil(2),
        }
    }
}

/// Options shared by the text renderers.
///
/// ```
/// use qr_core::TextOptions;
/// let options = TextOptions::default().with_margin(2).with_invert(true);
/// assert_eq!((options.margin, options.invert), (2, true));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TextOptions {
    /// Quiet zone around the symbol, in modules. Renderers clamp it to
    /// [`MAX_MARGIN`].
    pub margin: usize,
    /// Swap dark and light, quiet zone included. Useful when plain Unicode
    /// output is shown light-on-dark.
    pub invert: bool,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            margin: DEFAULT_MARGIN,
            invert: false,
        }
    }
}

impl TextOptions {
    /// Returns these options with the given quiet zone, in modules, clamped
    /// to [`MAX_MARGIN`].
    ///
    /// ```
    /// use qr_core::TextOptions;
    /// use qr_core::render::MAX_MARGIN;
    /// assert_eq!(TextOptions::default().with_margin(usize::MAX).margin, MAX_MARGIN);
    /// ```
    #[must_use]
    pub const fn with_margin(mut self, margin: usize) -> Self {
        self.margin = if margin > MAX_MARGIN {
            MAX_MARGIN
        } else {
            margin
        };
        self
    }

    /// The margin the renderers use: [`Self::margin`] clamped to
    /// [`MAX_MARGIN`], since the field is public and may be set directly.
    const fn effective_margin(&self) -> usize {
        if self.margin > MAX_MARGIN {
            MAX_MARGIN
        } else {
            self.margin
        }
    }

    /// Returns these options with dark and light swapped (or not).
    #[must_use]
    pub const fn with_invert(mut self, invert: bool) -> Self {
        self.invert = invert;
        self
    }
}

/// Renders `matrix` as text. Every line, including the last, ends in `\n`.
///
/// The output contains no user data. The only escape sequences are the
/// fixed colour codes of [`TextFormat::Ansi`].
///
/// ```
/// use qr_core::{Matrix, TextFormat, TextOptions, render_text};
///
/// let m = Matrix::from_modules(2, vec![true, false, false, true]).unwrap();
/// let options = TextOptions::default().with_margin(0);
/// assert_eq!(render_text(&m, TextFormat::Unicode, &options), "▀▄\n");
/// assert_eq!(render_text(&m, TextFormat::Ascii, &options), "##  \n  ##\n");
/// ```
pub fn render_text(matrix: &Matrix, format: TextFormat, options: &TextOptions) -> String {
    let side = frame_side(matrix, options);
    let margin = options.effective_margin();
    let dark = |x: usize, y: usize| matrix.is_dark_with_margin(x, y, margin) != options.invert;
    // Half blocks are 3 bytes in UTF-8; ASCII modules are 2 bytes.
    let line_bytes = side
        .saturating_mul(3)
        .saturating_add(ANSI_BLACK_ON_WHITE.len() + ANSI_RESET.len() + 1);
    let mut out = String::with_capacity(line_bytes.saturating_mul(format.lines(matrix, options)));

    match format {
        TextFormat::Ascii => {
            for y in 0..side {
                for x in 0..side {
                    out.push_str(if dark(x, y) { "##" } else { "  " });
                }
                out.push('\n');
            }
        }
        TextFormat::Unicode | TextFormat::Ansi => {
            let ansi = format == TextFormat::Ansi;
            for y in (0..side).step_by(2) {
                if ansi {
                    out.push_str(ANSI_BLACK_ON_WHITE);
                }
                for x in 0..side {
                    // When the frame has an odd number of rows, the lower half
                    // of the last line lies outside the frame. It reads as
                    // quiet zone, which is what `dark` returns there.
                    out.push(half_block(dark(x, y), dark(x, y + 1)));
                }
                if ansi {
                    out.push_str(ANSI_RESET);
                }
                out.push('\n');
            }
        }
    }
    out
}

/// The side of the symbol plus its quiet zone, in modules.
fn frame_side(matrix: &Matrix, options: &TextOptions) -> usize {
    matrix
        .width()
        .saturating_add(options.effective_margin().saturating_mul(2))
}

/// The character that draws a cell whose upper half is `top` and lower half
/// is `bottom` (`true` meaning ink).
const fn half_block(top: bool, bottom: bool) -> char {
    match (top, bottom) {
        (true, true) => '█',
        (true, false) => '▀',
        (false, true) => '▄',
        (false, false) => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EncodeOptions, encode};

    /// Reads a Unicode or ANSI rendering back into rows of modules.
    fn parse_half_blocks(text: &str) -> Vec<Vec<bool>> {
        let mut rows = Vec::new();
        for line in text.lines() {
            let line = line
                .strip_prefix(ANSI_BLACK_ON_WHITE)
                .and_then(|l| l.strip_suffix(ANSI_RESET))
                .unwrap_or(line);
            let (mut top, mut bottom) = (Vec::new(), Vec::new());
            for c in line.chars() {
                let (t, b) = match c {
                    '█' => (true, true),
                    '▀' => (true, false),
                    '▄' => (false, true),
                    ' ' => (false, false),
                    other => panic!("unexpected character {other:?}"),
                };
                top.push(t);
                bottom.push(b);
            }
            rows.push(top);
            rows.push(bottom);
        }
        rows
    }

    fn parse_ascii(text: &str) -> Vec<Vec<bool>> {
        text.lines()
            .map(|line| {
                line.as_bytes()
                    .chunks(2)
                    .map(|cell| match cell {
                        b"##" => true,
                        b"  " => false,
                        other => panic!("unexpected cell {other:?}"),
                    })
                    .collect()
            })
            .collect()
    }

    fn expected_frame(m: &Matrix, options: &TextOptions) -> Vec<Vec<bool>> {
        let side = frame_side(m, options);
        (0..side)
            .map(|y| {
                (0..side)
                    .map(|x| m.is_dark_with_margin(x, y, options.margin) != options.invert)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn half_block_covers_all_four_cells() {
        assert_eq!(half_block(true, true), '█');
        assert_eq!(half_block(true, false), '▀');
        assert_eq!(half_block(false, true), '▄');
        assert_eq!(half_block(false, false), ' ');
    }

    #[test]
    fn every_format_round_trips_to_the_module_frame() {
        let code = encode(b"https://example.com/round-trip", &EncodeOptions::default()).unwrap();
        let m = code.matrix();
        for margin in [0, 1, 4] {
            for invert in [false, true] {
                let options = TextOptions::default()
                    .with_margin(margin)
                    .with_invert(invert);
                let expected = expected_frame(m, &options);
                let side = expected.len();

                let ascii = render_text(m, TextFormat::Ascii, &options);
                assert_eq!(parse_ascii(&ascii), expected, "ascii {margin} {invert}");

                for format in [TextFormat::Unicode, TextFormat::Ansi] {
                    let text = render_text(m, format, &options);
                    let mut rows = parse_half_blocks(&text);
                    // An odd frame has one phantom half row, which must read
                    // as quiet zone.
                    assert_eq!(rows.len(), side + side % 2);
                    if side % 2 == 1 {
                        let phantom = rows.pop().unwrap();
                        assert!(phantom.iter().all(|&d| d == invert), "{format:?}");
                    }
                    assert_eq!(rows, expected, "{format:?} {margin} {invert}");
                    assert_eq!(text.lines().count(), format.lines(m, &options));
                }
            }
        }
    }

    #[test]
    fn dimensions_match_the_rendering() {
        let code = encode(b"dims", &EncodeOptions::default()).unwrap();
        let m = code.matrix();
        let options = TextOptions::default();
        assert_eq!(TextFormat::Unicode.columns(m, &options), 29);
        assert_eq!(TextFormat::Ansi.columns(m, &options), 29);
        assert_eq!(TextFormat::Ascii.columns(m, &options), 58);
        assert_eq!(TextFormat::Unicode.lines(m, &options), 15);
        assert_eq!(TextFormat::Ascii.lines(m, &options), 29);
        for format in [TextFormat::Unicode, TextFormat::Ascii] {
            let text = render_text(m, format, &options);
            for line in text.lines() {
                assert_eq!(line.chars().count(), format.columns(m, &options));
            }
        }
    }

    #[test]
    fn ansi_lines_are_wrapped_in_fixed_sequences_only() {
        let m = Matrix::from_modules(1, vec![true]).unwrap();
        let options = TextOptions::default().with_margin(1);
        let text = render_text(&m, TextFormat::Ansi, &options);
        assert_eq!(text, "\x1b[30;107m ▄ \x1b[0m\n\x1b[30;107m   \x1b[0m\n");
        let plain = render_text(&m, TextFormat::Unicode, &options);
        assert!(!plain.contains('\x1b'));
        assert!(render_text(&m, TextFormat::Ascii, &options).is_ascii());
    }

    #[test]
    fn huge_margins_are_clamped() {
        let m = Matrix::from_modules(1, vec![true]).unwrap();
        let side = 1 + 2 * MAX_MARGIN;
        let via_builder = TextOptions::default().with_margin(usize::MAX);
        assert_eq!(via_builder.margin, MAX_MARGIN);
        // The field is public, so a direct assignment must be clamped too.
        let direct = TextOptions {
            margin: usize::MAX,
            ..TextOptions::default()
        };
        for options in [via_builder, direct] {
            assert_eq!(TextFormat::Ascii.columns(&m, &options), 2 * side);
            assert_eq!(TextFormat::Unicode.lines(&m, &options), side.div_ceil(2));
            let text = render_text(&m, TextFormat::Ascii, &options);
            assert_eq!(text.lines().count(), side);
        }
    }

    #[test]
    fn default_options_use_the_iso_quiet_zone() {
        assert_eq!(TextOptions::default().margin, 4);
        assert!(!TextOptions::default().invert);
    }

    #[test]
    fn snapshots_of_a_real_code() {
        let code = encode(b"qr-cli", &EncodeOptions::default()).unwrap();
        let options = TextOptions::default().with_margin(2);
        insta::assert_snapshot!(
            "unicode_margin_2",
            render_text(code.matrix(), TextFormat::Unicode, &options)
        );
        insta::assert_snapshot!(
            "ascii_margin_2",
            render_text(code.matrix(), TextFormat::Ascii, &options)
        );
        insta::assert_snapshot!(
            "unicode_margin_2_inverted",
            render_text(
                code.matrix(),
                TextFormat::Unicode,
                &options.with_invert(true)
            )
        );
    }
}
