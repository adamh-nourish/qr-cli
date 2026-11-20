//! Field checks and the escaping rules the payload formats share.
//!
//! - [`backslash`]: the WiFi and MeCard convention (ZXing's
//!   `WIFI:`/`MECARD:` grammar), where `\ ; , : "` are prefixed with `\`.
//! - [`text`]: the vCard 3.0 (RFC 2426 section 4) and iCalendar (RFC 5545
//!   section 3.3.11) `TEXT` value, where `\ ; ,` are prefixed with `\` and
//!   a line break becomes the two characters `\n`.
//! - [`percent`]: URI percent-encoding (RFC 3986 section 2.1) of UTF-8.

use std::fmt::Write as _;

use super::PayloadError;
use crate::sanitize::is_bidi_format;

/// The characters [`backslash`] escapes in WiFi and MeCard values.
pub(crate) const BACKSLASH_SPECIALS: [char; 5] = ['\\', ';', ',', ':', '"'];

/// Which control characters a field may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Controls {
    /// None at all: the format has no way to carry them.
    Forbid,
    /// Tab, line feed and carriage return; every other control is refused.
    AllowLineBreaks,
}

/// Fails unless `value` has a non-blank character.
pub(crate) fn require(field: &'static str, value: &str) -> Result<(), PayloadError> {
    if value.trim().is_empty() {
        Err(PayloadError::Empty { field })
    } else {
        Ok(())
    }
}

/// Fails if `value` holds a control character `controls` does not allow.
pub(crate) fn check_controls(
    field: &'static str,
    value: &str,
    controls: Controls,
) -> Result<(), PayloadError> {
    let allowed =
        |c: char| controls == Controls::AllowLineBreaks && matches!(c, '\t' | '\n' | '\r');
    if let Some(character) = value.chars().find(|&c| c.is_control() && !allowed(c)) {
        return Err(PayloadError::ControlCharacter { field, character });
    }
    match value.chars().find(|&c| is_bidi_format(c)) {
        Some(character) => Err(PayloadError::BidiCharacter { field, character }),
        None => Ok(()),
    }
}

/// [`require`] and then [`check_controls`].
pub(crate) fn check_required(
    field: &'static str,
    value: &str,
    controls: Controls,
) -> Result<(), PayloadError> {
    require(field, value)?;
    check_controls(field, value, controls)
}

/// Prefixes each of [`BACKSLASH_SPECIALS`] with a backslash.
pub(crate) fn backslash(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if BACKSLASH_SPECIALS.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escapes a vCard or iCalendar `TEXT` value: `\`, `;` and `,` get a
/// backslash and each line break (`\r\n`, `\r` or `\n`) becomes `\n`.
pub(crate) fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' | ';' | ',' => {
                out.push('\\');
                out.push(c);
            }
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encodes every byte of `value`'s UTF-8 except the RFC 3986
/// unreserved characters (`A-Z a-z 0-9 - . _ ~`) and the ASCII bytes in
/// `keep`.
pub(crate) fn percent(value: &str, keep: &[u8]) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || keep.contains(&b)
        {
            out.push(char::from(b));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Rewrites every line break (`\r\n`, `\r` or `\n`) as `\r\n`.
pub(crate) fn crlf(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn backslash_escapes_exactly_the_wifi_specials() {
        assert_eq!(backslash(r#"a\b;c,d:e"f'g"#), r#"a\\b\;c\,d\:e\"f'g"#);
        assert_eq!(backslash("plain"), "plain");
    }

    #[test]
    fn text_escapes_separators_and_line_breaks() {
        assert_eq!(text("a\\b;c,d:e"), "a\\\\b\\;c\\,d:e");
        assert_eq!(text("1\r\n2\r3\n4"), "1\\n2\\n3\\n4");
    }

    #[test]
    fn percent_keeps_unreserved_and_extras() {
        assert_eq!(percent("a b/é~", b""), "a%20b%2F%C3%A9~");
        assert_eq!(percent("a@b+c", b"@"), "a@b%2Bc");
    }

    #[test]
    fn crlf_normalises_every_line_break() {
        assert_eq!(crlf("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn field_checks() {
        assert_eq!(
            require("SSID", " \t"),
            Err(PayloadError::Empty { field: "SSID" })
        );
        assert!(require("SSID", "x").is_ok());
        assert!(check_controls("note", "a\nb\tc\r", Controls::AllowLineBreaks).is_ok());
        assert_eq!(
            check_controls("note", "a\nb", Controls::Forbid),
            Err(PayloadError::ControlCharacter {
                field: "note",
                character: '\n'
            })
        );
        assert!(check_controls("note", "a\u{0}", Controls::AllowLineBreaks).is_err());
        assert!(check_controls("note", "a\u{85}", Controls::AllowLineBreaks).is_err());
        for bidi in ['\u{202E}', '\u{2066}', '\u{200F}', '\u{061C}'] {
            let err = check_controls("name", &format!("a{bidi}b"), Controls::AllowLineBreaks)
                .unwrap_err();
            assert_eq!(
                err,
                PayloadError::BidiCharacter {
                    field: "name",
                    character: bidi
                }
            );
            assert!(err.hint().is_some());
        }
        // Right-to-left text itself is fine.
        assert!(check_controls("name", "שלום مرحبا", Controls::Forbid).is_ok());
        assert!(check_required("note", "", Controls::Forbid).is_err());
    }

    /// Undoes [`backslash`].
    fn unbackslash(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            out.push(if c == '\\' { chars.next().unwrap() } else { c });
        }
        out
    }

    proptest! {
        #[test]
        fn backslash_round_trips_and_leaves_no_bare_special(s in "\\PC*") {
            let escaped = backslash(&s);
            prop_assert_eq!(unbackslash(&escaped), s);
            // Every special is preceded by an odd run of backslashes.
            let mut run = 0usize;
            for c in escaped.chars() {
                if c == '\\' {
                    run += 1;
                    continue;
                }
                if BACKSLASH_SPECIALS.contains(&c) {
                    prop_assert!(run % 2 == 1);
                }
                run = 0;
            }
            prop_assert!(run % 2 == 0);
        }

        #[test]
        fn percent_output_is_ascii_and_decodes(s in "\\PC*") {
            let encoded = percent(&s, b"");
            prop_assert!(encoded.bytes().all(|b| b.is_ascii_graphic()));
            let mut bytes = Vec::new();
            let raw = encoded.as_bytes();
            let mut i = 0;
            while i < raw.len() {
                if raw[i] == b'%' {
                    let hex = std::str::from_utf8(&raw[i + 1..i + 3]).unwrap();
                    bytes.push(u8::from_str_radix(hex, 16).unwrap());
                    i += 3;
                } else {
                    bytes.push(raw[i]);
                    i += 1;
                }
            }
            prop_assert_eq!(String::from_utf8(bytes).unwrap(), s);
        }

        #[test]
        fn text_output_has_no_line_breaks(s in "(\\PC|[\r\n])*") {
            let escaped = text(&s);
            prop_assert!(!escaped.contains(['\r', '\n']));
        }
    }
}
