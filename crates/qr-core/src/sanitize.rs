//! Making untrusted text safe to print on a terminal.
//!
//! Anything that echoes user or decoded data (error messages, verbose
//! output, decode results, batch progress) passes it through
//! [`sanitize_for_terminal`] first, so the data cannot move the cursor,
//! change colours, set the window title or reorder text on screen
//! (DECISIONS.md section 4.5).

use std::borrow::Cow;
use std::fmt::Write as _;

/// Replaces characters that a terminal would interpret instead of display
/// with visible `\u{..}` escapes.
///
/// Escaped characters are:
/// - C0 controls (U+0000 to U+001F, including ESC, tab and newline), DEL
///   (U+007F) and C1 controls (U+0080 to U+009F);
/// - the bidirectional formatting characters U+061C, U+200E, U+200F,
///   U+202A to U+202E and U+2066 to U+2069, which can make text appear in
///   a different order from the bytes that make it up.
///
/// Text that needs no escaping is returned borrowed.
///
/// ```
/// use qr_core::sanitize_for_terminal;
/// assert_eq!(sanitize_for_terminal("plain"), "plain");
/// assert_eq!(sanitize_for_terminal("a\x1b[31mb"), "a\\u{1b}[31mb");
/// assert_eq!(sanitize_for_terminal("x\u{202e}y"), "x\\u{202e}y");
/// ```
pub fn sanitize_for_terminal(text: &str) -> Cow<'_, str> {
    sanitize_with(text, needs_escape)
}

/// Like [`sanitize_for_terminal`], but keeps line feeds so that multi-line
/// messages stay readable. A carriage return is still escaped, so a line
/// cannot overwrite the one before it.
///
/// ```
/// use qr_core::sanitize_lines_for_terminal;
/// assert_eq!(sanitize_lines_for_terminal("a\nb\r"), "a\nb\\u{d}");
/// ```
pub fn sanitize_lines_for_terminal(text: &str) -> Cow<'_, str> {
    sanitize_with(text, |c| c != '\n' && needs_escape(c))
}

fn sanitize_with(text: &str, escape: impl Fn(char) -> bool) -> Cow<'_, str> {
    if !text.chars().any(&escape) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len().saturating_add(16));
    for c in text.chars() {
        if escape(c) {
            // Writing to a String cannot fail.
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Whether a terminal would act on `c` rather than just display it.
fn needs_escape(c: char) -> bool {
    // `char::is_control` is the Unicode `Cc` category: C0, DEL and C1.
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn escapes_every_c0_c1_and_del() {
        for c in (0u32..0x20).chain([0x7f]).chain(0x80..0xa0) {
            let c = char::from_u32(c).unwrap();
            let s = c.to_string();
            let out = sanitize_for_terminal(&s);
            assert_eq!(out, format!("\\u{{{:x}}}", u32::from(c)));
        }
    }

    #[test]
    fn escapes_bidi_overrides_and_marks() {
        for c in [
            '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
            '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            let out = sanitize_for_terminal(&c.to_string()).into_owned();
            assert!(out.starts_with("\\u{"), "{c:?} -> {out}");
        }
    }

    #[test]
    fn leaves_printable_text_borrowed() {
        for s in ["", "hello", "héllo wörld", "日本語", "emoji 🎉", "tab-free"] {
            assert!(matches!(sanitize_for_terminal(s), Cow::Borrowed(_)), "{s}");
        }
    }

    #[test]
    fn lines_variant_keeps_only_line_feeds() {
        assert_eq!(sanitize_lines_for_terminal("a\nb"), "a\nb");
        assert!(matches!(
            sanitize_lines_for_terminal("a\nb"),
            Cow::Borrowed(_)
        ));
        assert_eq!(
            sanitize_lines_for_terminal("a\r\n\x1b]0;title\x07"),
            "a\\u{d}\n\\u{1b}]0;title\\u{7}"
        );
        assert_eq!(sanitize_for_terminal("a\nb"), "a\\u{a}b");
    }

    proptest! {
        #[test]
        fn output_never_contains_dangerous_characters(s in any::<String>()) {
            let out = sanitize_for_terminal(&s);
            prop_assert!(!out.chars().any(needs_escape));
            let lines = sanitize_lines_for_terminal(&s);
            prop_assert!(!lines.chars().any(|c| c != '\n' && needs_escape(c)));
        }

        #[test]
        fn sanitizing_is_idempotent(s in any::<String>()) {
            let once = sanitize_for_terminal(&s).into_owned();
            prop_assert_eq!(sanitize_for_terminal(&once), once.as_str());
        }

        #[test]
        fn safe_text_is_unchanged(s in "[a-zA-Z0-9 ./:é日-]{0,64}") {
            prop_assert_eq!(sanitize_for_terminal(&s), s.as_str());
        }
    }
}
