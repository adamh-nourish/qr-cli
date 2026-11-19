//! The `--json` objects: one result object on stdout, and one object per
//! error, warning or note on stderr (one per line).
//!
//! The field names are part of the CLI contract. New fields may be added;
//! existing ones are not renamed or removed.

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
}

/// A diagnostic, as a one-line JSON object keyed by its kind:
/// `{"error":{"code":"...","message":"...","hint":"..."}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic<'a> {
    /// A stable identifier, for errors only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'a str>,
    /// The human-readable message.
    pub message: &'a str,
    /// One actionable suggestion, if there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<&'a str>,
}

/// Serialises `value` as one line of JSON, fields in declaration order.
///
/// Serialising these plain structs cannot fail; if it ever did, an
/// internal-error object takes its place.
pub fn object<T: Serialize + ?Sized>(value: &T) -> String {
    let mut out = serde_json::to_string(value).unwrap_or_else(|e| {
        serde_json::json!({ "code": "internal", "message": e.to_string() }).to_string()
    });
    out.push('\n');
    out
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
            code: Some("usage"),
            message: "bad \"thing\"\n",
            hint: None,
        };
        assert_eq!(
            keyed("error", &d),
            "{\"error\":{\"code\":\"usage\",\"message\":\"bad \\\"thing\\\"\\n\"}}\n"
        );
        let w = Diagnostic {
            code: None,
            message: "m",
            hint: Some("h"),
        };
        assert_eq!(
            keyed("warning", &w),
            "{\"warning\":{\"message\":\"m\",\"hint\":\"h\"}}\n"
        );
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
        };
        let text = object(&g);
        assert!(text.ends_with("}\n") && !text[..text.len() - 1].contains('\n'));
        let r: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(r["output"], "code.svg");
        assert_eq!(r["modules"], 21);
        assert!(r.get("data_uri").is_none());
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
