//! Escaping for every string the vector renderers write: XML text and
//! attribute values, PDF text strings and PostScript strings.
//!
//! Each function maps arbitrary input to a form that cannot leave the
//! surrounding syntax, so no user text can open an element, close a string
//! or start a new line of PostScript or a DSC comment.

use std::fmt::Write as _;

/// Whether `c` is allowed in an XML 1.0 document (the `Char` production).
/// Rust `char`s are never surrogates, so only the low controls and the two
/// non-characters at the end of the Basic Multilingual Plane are excluded.
const fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | ' '..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// Escapes `s` for XML text content or a double- or single-quoted attribute
/// value. The five markup characters become entities and characters that
/// XML 1.0 forbids are dropped.
pub(crate) fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().filter(|&c| is_xml_char(c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Attribute-value normalisation would turn these into spaces.
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

/// Writes `s` as a PDF text string: a literal string `( … )` when it is
/// printable ASCII, otherwise a UTF-16BE hex string with a byte order mark,
/// as the PDF specification requires for text outside `PDFDocEncoding`.
pub(crate) fn pdf_text(s: &str) -> String {
    if s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('(');
        for c in s.chars() {
            if matches!(c, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for unit in s.encode_utf16() {
            let _ = write!(out, "{unit:04X}");
        }
        out.push('>');
        out
    }
}

/// Writes `s` as a PostScript string `( … )` of at most `max_len` bytes,
/// including the parentheses. Printable ASCII is kept, `(`, `)` and `\` are
/// backslash-escaped and every other byte of the UTF-8 encoding becomes an
/// octal escape, so the result is one line of ASCII. Input that does not fit
/// is cut at an escape boundary.
pub(crate) fn ps_string(s: &str, max_len: usize) -> String {
    let mut out = String::from("(");
    let budget = max_len.saturating_sub(1);
    let mut piece = String::with_capacity(4);
    for b in s.bytes() {
        piece.clear();
        match b {
            b'(' | b')' | b'\\' => {
                piece.push('\\');
                piece.push(char::from(b));
            }
            0x20..=0x7e => piece.push(char::from(b)),
            _ => {
                let _ = write!(piece, "\\{b:03o}");
            }
        }
        if out.len() + piece.len() > budget {
            break;
        }
        out.push_str(&piece);
    }
    out.push(')');
    out
}

/// Formats a channel value as a fraction of 255 with at most three decimals
/// and no trailing zeros (`0`, `0.5`, `1`). Three decimals keep all 256
/// values distinct.
pub(crate) fn unit(value: u8) -> String {
    let text = format!("{:.3}", f64::from(value) / 255.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn xml_escapes_markup_and_drops_invalid_characters() {
        assert_eq!(
            xml(r#"<a href="x">'&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&apos;&amp;&apos;&lt;/a&gt;"
        );
        assert_eq!(xml("a\u{0}b\u{1b}c\u{FFFE}\u{FFFF}d"), "abcd");
        assert_eq!(xml("t\tn\nr\r"), "t&#9;n&#10;r&#13;");
        assert_eq!(
            xml("héllo 😀 \u{D7FF}\u{E000}"),
            "héllo 😀 \u{D7FF}\u{E000}"
        );
    }

    #[test]
    fn pdf_text_uses_literal_or_utf16_strings() {
        assert_eq!(pdf_text("hello"), "(hello)");
        assert_eq!(pdf_text(r"a(b)c\d"), r"(a\(b\)c\\d)");
        assert_eq!(pdf_text("é"), "<FEFF00E9>");
        assert_eq!(pdf_text("a\nb"), "<FEFF0061000A0062>");
        assert_eq!(pdf_text("😀"), "<FEFFD83DDE00>");
        assert_eq!(pdf_text(""), "()");
    }

    #[test]
    fn ps_string_escapes_and_truncates() {
        assert_eq!(ps_string("abc", 255), "(abc)");
        assert_eq!(ps_string(r"(x)\", 255), r"(\(x\)\\)");
        assert_eq!(ps_string("a\nb", 255), r"(a\012b)");
        assert_eq!(ps_string("é", 255), r"(\303\251)");
        assert_eq!(ps_string("abcdef", 5), "(abc)");
        // An escape is never split.
        assert_eq!(ps_string("ab\n", 6), "(ab)");
        assert_eq!(ps_string("abc", 0), "()");
    }

    #[test]
    fn unit_is_short_and_exact_at_the_ends() {
        assert_eq!(unit(0), "0");
        assert_eq!(unit(255), "1");
        assert_eq!(unit(128), "0.502");
        assert_eq!(unit(51), "0.2");
        let all: std::collections::HashSet<String> = (0..=255).map(unit).collect();
        assert_eq!(all.len(), 256);
    }

    proptest! {
        #[test]
        fn xml_output_has_no_raw_markup(s in any::<String>()) {
            let escaped = xml(&s);
            prop_assert!(!escaped.contains(['<', '>', '"', '\'']));
            prop_assert!(escaped.chars().all(|c| is_xml_char(c) && !matches!(c, '\t' | '\n' | '\r')));
        }

        #[test]
        fn ps_string_is_one_balanced_ascii_line(s in any::<String>(), max in 2usize..300) {
            let escaped = ps_string(&s, max);
            prop_assert!(escaped.len() <= max);
            prop_assert!(escaped.bytes().all(|b| (0x20..0x7f).contains(&b)));
            let inner = &escaped[1..escaped.len() - 1];
            // Every parenthesis and backslash inside is escaped.
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                prop_assert!(c != '(' && c != ')');
                if c == '\\' {
                    let next = chars.next();
                    prop_assert!(next.is_some());
                }
            }
        }

        #[test]
        fn pdf_text_is_ascii(s in any::<String>()) {
            let escaped = pdf_text(&s);
            prop_assert!(escaped.bytes().all(|b| (0x20..0x7f).contains(&b)));
        }
    }
}
