//! `--name` templates: how each row's file name is built.
//!
//! A template is literal text with fields in braces:
//!
//! - `{n}`: the row number, from 1 (`{n:04}` pads it with zeros to 4
//!   digits);
//! - `{hash}`: 16 hex digits identifying the data (FNV-1a, 64 bits; stable
//!   across runs and platforms, not a security hash);
//! - `{data}`: the data itself;
//! - `{filename}`: the CSV `filename` column (the second column without a
//!   header);
//! - `{NAME}`: any other column of a CSV with a header, by its name.
//!
//! Field names ignore case, and `{{` and `}}` are literal braces. Every
//! field value is cleaned by [`sanitize_field`], so nothing a row holds can
//! add a path separator; the literal text may not contain characters the
//! cleaner would remove. The extension is added afterwards (see
//! [`file_name`](super::name::file_name)).

use std::fmt::Write as _;

use super::name::{is_forbidden, sanitize_field};
use super::source::{Row, Schema};

/// The widest `{n:0W}` padding accepted.
const MAX_WIDTH: usize = 10;

/// Why a `--name` template was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// A `{` without its `}`, or a `}` without its `{`.
    #[error("unbalanced braces in --name '{0}'")]
    Unbalanced(String),
    /// A field the input does not have.
    #[error("unknown field '{{{field}}}' in --name; available: {available}")]
    UnknownField {
        /// The field name as written.
        field: String,
        /// The fields this input offers, as `{n}, {hash}, ...`.
        available: String,
    },
    /// A format other than `{n:W}` / `{n:0W}` with W from 1 to 10.
    #[error("unsupported format '{{{0}}}' in --name; only {{n}} takes a width, such as {{n:04}}")]
    Format(String),
    /// Literal text the sanitiser would remove, such as a `/`.
    #[error("--name cannot contain {0:?}: names are single files inside --out-dir")]
    Forbidden(char),
    /// The template is empty.
    #[error("--name is empty")]
    Empty,
}

/// One piece of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    /// Literal text.
    Literal(String),
    /// `{n}` or `{n:0W}`.
    Number {
        /// The zero-padded width, or 0 for none.
        width: usize,
    },
    /// `{hash}`.
    Hash,
    /// A column of the row: `{data}`, `{filename}` or a named column.
    Column(usize),
}

/// A parsed `--name` template, checked against the input's columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Template {
    parts: Vec<Part>,
}

impl Template {
    /// The template used without `--name`: `{filename}` when the input has
    /// a filename column, otherwise `{n}`.
    pub(crate) fn default_for(schema: &Schema) -> &'static str {
        if schema.filename().is_some() {
            "{filename}"
        } else {
            "{n}"
        }
    }

    /// Parses `text` for an input with `schema`.
    ///
    /// # Errors
    ///
    /// A [`TemplateError`] for unbalanced braces, an unknown field, an
    /// unsupported format, a forbidden literal character or an empty
    /// template.
    pub(crate) fn parse(text: &str, schema: &Schema) -> Result<Self, TemplateError> {
        if text.is_empty() {
            return Err(TemplateError::Empty);
        }
        let unbalanced = || TemplateError::Unbalanced(text.to_owned());
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    literal.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    literal.push('}');
                }
                '}' => return Err(unbalanced()),
                '{' => {
                    let mut field = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some('{') | None => return Err(unbalanced()),
                            Some(c) => field.push(c),
                        }
                    }
                    if !literal.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(field_part(&field, schema)?);
                }
                c if is_forbidden(c) => return Err(TemplateError::Forbidden(c)),
                c => literal.push(c),
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(Self { parts })
    }

    /// The file name stem for `row`, before the extension is added.
    pub(crate) fn render(&self, row: &Row) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(text) => out.push_str(text),
                // Writing to a String cannot fail.
                Part::Number { width } => {
                    let _ = write!(out, "{:0width$}", row.n, width = *width);
                }
                Part::Hash => {
                    let _ = write!(out, "{:016x}", fnv1a(row.data()));
                }
                Part::Column(index) => {
                    let value = String::from_utf8_lossy(row.field(*index));
                    out.push_str(&sanitize_field(&value));
                }
            }
        }
        out
    }
}

/// The part for the field written `{field}`.
fn field_part(field: &str, schema: &Schema) -> Result<Part, TemplateError> {
    let (name, format) = match field.split_once(':') {
        Some((name, format)) => (name, Some(format)),
        None => (field, None),
    };
    let lower = name.trim().to_ascii_lowercase();
    match (lower.as_str(), format) {
        ("n", None) => Ok(Part::Number { width: 0 }),
        ("n", Some(format)) => parse_width(format)
            .map(|width| Part::Number { width })
            .ok_or_else(|| TemplateError::Format(field.to_owned())),
        (_, Some(_)) => Err(TemplateError::Format(field.to_owned())),
        ("hash", None) => Ok(Part::Hash),
        (column, None) => {
            schema
                .column(column)
                .map(Part::Column)
                .ok_or_else(|| TemplateError::UnknownField {
                    field: field.to_owned(),
                    available: available(schema),
                })
        }
    }
}

/// The width in `{n:04}` (`04`) or `{n:4}`.
fn parse_width(format: &str) -> Option<usize> {
    if format.is_empty() || !format.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    format
        .parse::<usize>()
        .ok()
        .filter(|w| (1..=MAX_WIDTH).contains(w))
}

/// The fields an input offers, for the unknown-field message.
fn available(schema: &Schema) -> String {
    let mut fields = vec!["{n}".to_owned(), "{hash}".to_owned()];
    fields.extend(schema.column_names().map(|name| format!("{{{name}}}")));
    fields.join(", ")
}

/// The 64-bit FNV-1a hash of `bytes`: small, fast and stable, which is all
/// a file name needs.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use super::super::source::{Layout, Table};
    use super::*;

    fn table(csv: &str, layout: Layout) -> Table<'_> {
        Table::parse(csv.as_bytes(), layout).unwrap()
    }

    fn names(text: &str, csv: &str, layout: Layout) -> Result<Vec<String>, TemplateError> {
        let table = table(csv, layout);
        let template = Template::parse(text, table.schema())?;
        Ok(table
            .rows()
            .map(|row| template.render(&row.unwrap()))
            .collect())
    }

    #[test]
    fn number_hash_and_data_fields() {
        let csv = "hello\nworld\n";
        assert_eq!(names("{n}", csv, Layout::Lines).unwrap(), ["1", "2"]);
        assert_eq!(
            names("code-{n:04}", csv, Layout::Lines).unwrap(),
            ["code-0001", "code-0002"]
        );
        assert_eq!(names("{N:3}", csv, Layout::Lines).unwrap(), ["001", "002"]);
        assert_eq!(
            names("{data}_{hash}", csv, Layout::Lines).unwrap(),
            ["hello_a430d84680aabd0b", "world_4f59ff5e730c8af3"]
        );
        assert_eq!(
            names("{{{n}}}", csv, Layout::Lines).unwrap(),
            ["{1}", "{2}"]
        );
    }

    #[test]
    fn csv_columns_by_name() {
        let csv = "id,Data,filename,Team Name,team\n7,x,../a,eng,ops\n";
        let detect = Layout::Csv {
            detect_header: true,
        };
        assert_eq!(names("{id}-{filename}", csv, detect).unwrap(), ["7-a"]);
        assert_eq!(names("{TEAM}", csv, detect).unwrap(), ["ops"]);
        let err = names("{team name}", csv, detect).unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown field '{team name}' in --name; available: \
             {n}, {hash}, {id}, {data}, {filename}, {team}"
        );
        // Without a header: data, then filename.
        let plain = Layout::Csv {
            detect_header: false,
        };
        assert_eq!(names("{filename}", "x,..\\b\n", plain).unwrap(), ["b"]);
        assert!(matches!(
            names("{filename}", "x\n", plain),
            Err(TemplateError::UnknownField { .. })
        ));
        assert!(matches!(
            names("{filename}", "x\n", Layout::Lines),
            Err(TemplateError::UnknownField { .. })
        ));
    }

    #[test]
    fn field_values_are_sanitised() {
        let csv = "data,filename\n\"a/b\",\"../../etc/passwd\"\n";
        let layout = Layout::Csv {
            detect_header: true,
        };
        assert_eq!(
            names("{data}-{filename}", csv, layout).unwrap(),
            ["ab-etcpasswd"]
        );
        // Bytes that are not UTF-8 are replaced, not passed through.
        let table = Table::parse(b"\xff/x\n", Layout::Lines).unwrap();
        let template = Template::parse("{data}", table.schema()).unwrap();
        let row = table.rows().next().unwrap().unwrap();
        assert_eq!(template.render(&row), "\u{fffd}x");
    }

    #[test]
    fn bad_templates_are_refused() {
        let t = table("a\n", Layout::Lines);
        let parse = |text: &str| Template::parse(text, t.schema()).unwrap_err();
        assert_eq!(parse(""), TemplateError::Empty);
        assert_eq!(parse("{n"), TemplateError::Unbalanced("{n".into()));
        assert_eq!(parse("n}"), TemplateError::Unbalanced("n}".into()));
        assert_eq!(parse("{a{n}"), TemplateError::Unbalanced("{a{n}".into()));
        assert_eq!(parse("sub/{n}"), TemplateError::Forbidden('/'));
        assert_eq!(parse("c:{n}"), TemplateError::Forbidden(':'));
        assert_eq!(parse("{n}\u{2215}"), TemplateError::Forbidden('\u{2215}'));
        for format in ["{n:}", "{n:x}", "{n:0}", "{n:11}", "{n:-4}", "{hash:4}"] {
            assert!(
                matches!(parse(format), TemplateError::Format(_)),
                "{format}"
            );
        }
        assert_eq!(
            parse("{n:x}").to_string(),
            "unsupported format '{n:x}' in --name; only {n} takes a width, such as {n:04}"
        );
        assert_eq!(
            parse("{nope}").to_string(),
            "unknown field '{nope}' in --name; available: {n}, {hash}, {data}"
        );
        assert_eq!(
            parse("a\\b").to_string(),
            "--name cannot contain '\\\\': names are single files inside --out-dir"
        );
    }

    #[test]
    fn default_template_follows_the_filename_column() {
        let with = table(
            "data,filename\nx,y\n",
            Layout::Csv {
                detect_header: true,
            },
        );
        assert_eq!(Template::default_for(with.schema()), "{filename}");
        let without = table(
            "x\n",
            Layout::Csv {
                detect_header: true,
            },
        );
        assert_eq!(Template::default_for(without.schema()), "{n}");
        assert_eq!(
            Template::default_for(table("x\n", Layout::Lines).schema()),
            "{n}"
        );
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
