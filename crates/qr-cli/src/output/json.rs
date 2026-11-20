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

/// The result of `qr decode --json`: every image in command-line order,
/// then the totals.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DecodeReport {
    /// One entry per `IMAGE` argument.
    pub images: Vec<DecodedImage>,
    /// Totals over all images.
    pub summary: DecodeSummary,
}

/// What one image held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecodedImage {
    /// The path as given, or `-` for stdin.
    pub path: String,
    /// The codes read, top to bottom, then left to right. Empty when the
    /// image could not be read or held no readable code.
    pub payloads: Vec<DecodedPayload>,
    /// Why nothing was read, when that is an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ImageError>,
}

/// One decoded code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecodedPayload {
    /// The data as text. Bytes that are not UTF-8 are replaced with
    /// U+FFFD; `base64` then holds the exact bytes.
    pub text: String,
    /// The exact data bytes in standard base64, only when they are not
    /// valid UTF-8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    /// The number of data bytes.
    pub bytes: usize,
    /// The symbol version, 1 to 40.
    pub version: u8,
    /// The error correction level: L, M, Q or H.
    pub ec: String,
    /// The mask pattern, 0 to 7.
    pub mask: u8,
    /// Whether the code is light on dark.
    pub inverted: bool,
    /// The corners in image pixels as `[x, y]`: top-left, top-right,
    /// bottom-right, bottom-left (as the code is oriented).
    pub bounds: [[i32; 2]; 4],
}

/// Why an image (`qr decode`) or a row (`qr batch`) yielded nothing: the
/// same code and message as the error reported on stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageError {
    /// A stable identifier, such as `no_code_found` or `invalid_image`.
    pub code: &'static str,
    /// The human-readable message.
    pub message: String,
}

/// Totals for `qr decode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DecodeSummary {
    /// The number of images given.
    pub images: usize,
    /// The number of codes decoded, over all images.
    pub codes: usize,
    /// Images that could not be read or are not acceptable images.
    pub unreadable: usize,
    /// Images that were read but held no readable code.
    pub no_code: usize,
}

/// One `qr batch --json` line: what happened to one row.
///
/// A written row has the code's details; a failed one has `error` (and
/// `output` when its file name was settled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchRow {
    /// The row number, from 1, not counting a header or blank lines.
    pub row: usize,
    /// The input line the row starts on, from 1.
    pub line: u64,
    /// The file written (or that would have been), `--out-dir` joined
    /// with the name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// The code written, for a row that succeeded.
    #[serde(flatten)]
    pub code: Option<BatchCode>,
    /// Why the row failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ImageError>,
}

/// The code written for one `qr batch` row; the fields mean what they do
/// in [`Generated`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchCode {
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
    /// The number of bytes written to the file.
    pub written: usize,
    /// Whether the rendered code read back as the data, when it was
    /// checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
}

/// The last `qr batch --json` line, under the key `summary`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BatchSummary {
    /// Rows in the input.
    pub rows: usize,
    /// Rows written.
    pub written: usize,
    /// Rows that failed.
    pub failed: usize,
    /// Rows not tried because `--fail-fast` stopped the batch.
    pub skipped: usize,
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
    fn batch_rows_are_flat_objects() {
        let ok = BatchRow {
            row: 1,
            line: 2,
            output: Some("out/1.png".into()),
            code: Some(BatchCode {
                format: "png",
                version: 1,
                ec: "M".into(),
                mode: "byte".into(),
                mask: 3,
                modules: 21,
                bytes: 5,
                written: 99,
                verified: None,
            }),
            error: None,
        };
        assert_eq!(
            object(&ok),
            "{\"row\":1,\"line\":2,\"output\":\"out/1.png\",\"format\":\"png\",\
             \"version\":1,\"ec\":\"M\",\"mode\":\"byte\",\"mask\":3,\"modules\":21,\
             \"bytes\":5,\"written\":99}\n"
        );
        let failed = BatchRow {
            row: 2,
            line: 3,
            output: None,
            code: None,
            error: Some(ImageError {
                code: "empty_input",
                message: "the data is empty".into(),
            }),
        };
        assert_eq!(
            object(&failed),
            "{\"row\":2,\"line\":3,\"error\":{\"code\":\"empty_input\",\
             \"message\":\"the data is empty\"}}\n"
        );
        assert_eq!(
            keyed("summary", &BatchSummary::default()),
            "{\"summary\":{\"rows\":0,\"written\":0,\"failed\":0,\"skipped\":0}}\n"
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
