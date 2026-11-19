//! The `--json` objects: one result object on stdout, and one object per
//! error, warning or note on stderr (one per line).
//!
//! The field names are part of the CLI contract. New fields may be added;
//! existing ones are not renamed or removed.
//!
//! Strings such as the `-o` path can hold any character, and stdout or
//! stderr can be a terminal, so [`object`] writes every character a
//! terminal would act on (C0 and C1 controls, DEL and the bidi controls
//! that `sanitize_for_terminal` escapes) as a `\uXXXX` escape (DECISIONS
//! section 4.5). The result is still plain JSON and decodes to the exact
//! original text.

use std::fmt::Write as _;

use serde::Serialize;

/// The result of generating one code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Generated {
    /// The file written, or `None` when the code went to stdout.
    pub output: Option<String>,
    /// The output format name, as accepted by `-f`.
    pub format: &'static str,
    /// The symbol version, 1 to 40.
    pub version: u8,
    /// The error correction level: L, M, Q or H.
    pub ec: String,
    /// The data mode: numeric, alphanumeric or byte.
    pub mode: String,
    /// The mask pattern, 0 to 7.
    pub mask: u8,
    /// The symbol width in modules, without the quiet zone.
    pub modules: usize,
    /// The number of data bytes encoded.
    pub bytes: usize,
    /// The number of bytes written to the output.
    pub written: usize,
    /// The `data:` URI, when `--data-uri` sent it to stdout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_uri: Option<String>,
    /// Whether the rendered code read back as the input, when it was
    /// checked (`--verify`, or automatically for a styled image).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
}

/// A diagnostic, as a one-line JSON object keyed by its kind:
/// `{"error":{"code":"...","message":"...","hint":"..."}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic<'a> {
    /// A stable identifier, such as `unknown_extension` for an error or
    /// `extension_mismatch` for a warning.
    pub code: &'a str,
    /// The human-readable message.
    pub message: &'a str,
    /// One actionable suggestion, if there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<&'a str>,
}

/// Serialises `value` as one line of JSON, fields in declaration order,
/// with terminal controls escaped (see the module documentation).
///
/// Serialising these plain structs cannot fail; if it ever did, an
/// internal-error object takes its place.
pub fn object<T: Serialize + ?Sized>(value: &T) -> String {
    let text = serde_json::to_string(value).unwrap_or_else(|e| {
        serde_json::json!({ "code": "internal", "message": e.to_string() }).to_string()
    });
    let mut out = escape_controls(&text);
    out.push('\n');
    out
}

/// Rewrites the characters `serde_json` leaves raw but a terminal acts on
/// as `\uXXXX` escapes. JSON syntax outside strings is ASCII, so any such
/// character is inside a string, where the escape means the same character.
/// All of them are in the Basic Multilingual Plane, so one escape suffices.
fn escape_controls(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if is_terminal_control(c) {
            // Writing to a String cannot fail.
            let _ = write!(out, "\\u{:04x}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// The characters `qr_core::sanitize_for_terminal` escapes (a test keeps
/// the two in step): C0, DEL, C1 and the bidi controls.
fn is_terminal_control(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// Serialises `value` as one line of JSON under the key `kind`.
pub fn keyed<T: Serialize + ?Sized>(kind: &str, value: &T) -> String {
    object(&Keyed { kind, value })
}

/// `{kind: value}`, keeping `value`'s field order (a `serde_json::Map`
/// would sort the keys).
struct Keyed<'a, T: ?Sized> {
    kind: &'a str,
    value: &'a T,
}

impl<T: Serialize + ?Sized> Serialize for Keyed<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.kind, self.value)?;
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_omit_empty_fields() {
        let d = Diagnostic {
            code: "usage",
            message: "bad \"thing\"\n",
            hint: None,
        };
        assert_eq!(
            keyed("error", &d),
            "{\"error\":{\"code\":\"usage\",\"message\":\"bad \\\"thing\\\"\\n\"}}\n"
        );
        let w = Diagnostic {
            code: "w",
            message: "m",
            hint: Some("h"),
        };
        assert_eq!(
            keyed("warning", &w),
            "{\"warning\":{\"code\":\"w\",\"message\":\"m\",\"hint\":\"h\"}}\n"
        );
    }

    #[test]
    fn terminal_controls_are_escaped_losslessly() {
        let nasty =
            "a\u{1b}[2J\u{7f}\u{80}\u{9b}31m\u{9f}\u{202e}gpj.svg\u{2066}\u{61c}\u{200f}\n\té";
        let g = Generated {
            output: Some(nasty.into()),
            format: "svg",
            version: 1,
            ec: "M".into(),
            mode: "byte".into(),
            mask: 0,
            modules: 21,
            bytes: 1,
            written: 1,
            data_uri: None,
            verified: None,
        };
        let text = object(&g);
        assert!(
            !text[..text.len() - 1].chars().any(is_terminal_control),
            "{text:?}"
        );
        for escape in [
            "\\u007f", "\\u0080", "\\u009b", "\\u009f", "\\u202e", "\\u2066",
        ] {
            assert!(text.contains(escape), "{escape} in {text}");
        }
        // Ordinary non-ASCII text is left alone.
        assert!(text.contains('é'));
        let back: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["output"], nasty);
    }

    #[test]
    fn the_escaped_set_matches_the_terminal_sanitiser() {
        for c in (0..=0x3000_u32).filter_map(char::from_u32) {
            let s = c.to_string();
            let sanitised = qr_core::sanitize_for_terminal(&s) != s;
            assert_eq!(is_terminal_control(c), sanitised, "U+{:04X}", u32::from(c));
        }
    }

    #[test]
    fn result_object_shape() {
        let g = Generated {
            output: Some("code.svg".into()),
            format: "svg",
            version: 1,
            ec: "M".into(),
            mode: "byte".into(),
            mask: 2,
            modules: 21,
            bytes: 5,
            written: 100,
            data_uri: None,
            verified: None,
        };
        let text = object(&g);
        assert!(text.ends_with("}\n") && !text[..text.len() - 1].contains('\n'));
        let r: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(r["output"], "code.svg");
        assert_eq!(r["modules"], 21);
        assert!(r.get("data_uri").is_none());
        assert!(r.get("verified").is_none());
        let checked = object(&Generated {
            verified: Some(true),
            ..g
        });
        assert!(checked.ends_with(",\"verified\":true}\n"), "{checked}");
    }

    #[test]
    fn a_failing_value_still_produces_an_object() {
        struct Fails;
        impl Serialize for Fails {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("nope"))
            }
        }
        assert_eq!(
            object(&Fails),
            "{\"code\":\"internal\",\"message\":\"nope\"}\n"
        );
    }
}
