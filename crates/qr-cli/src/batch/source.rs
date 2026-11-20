//! The batch input: reading `FILE` within the limits, and splitting it
//! into rows.
//!
//! The whole input is read first (at most [`MAX_INPUT_BYTES`]), and every
//! row is counted before anything is written, so a file over
//! [`MAX_ROWS`] rows, or one with no rows, is refused up front. Rows are
//! then produced one at a time from the buffer, so memory beyond the input
//! itself does not grow with the row count.
//!
//! Two layouts are read:
//!
//! - **CSV** (the default): with a header, the column named `data` is
//!   encoded and one named `filename` (if any) feeds `{filename}`; every
//!   other column whose name is letters, digits and `_` is a template
//!   field. The first row is a header when one of its cells is `data`
//!   (ignoring case and surrounding spaces), unless `--no-header` says
//!   otherwise. Without a header, the first column is the data and the
//!   second (if the first row has one) the file name. Rows may have
//!   different lengths; blank lines are skipped.
//! - **Lines** (`--lines`): every line is one item, without its line
//!   ending (`\n` or `\r\n`); blank lines are skipped.
//!
//! A UTF-8 byte order mark at the start is ignored. Cells are bytes, so
//! data that is not UTF-8 is encoded as it is.

use std::io::{self, Read};
use std::path::Path;

use csv::{ByteRecord, ReaderBuilder};

use crate::error::CliError;
use crate::input::{InputSource, open_regular_file};

/// The most rows one batch encodes (DECISIONS section 4.7).
pub const MAX_ROWS: usize = 100_000;

/// The largest batch input, in bytes (DECISIONS section 4.7).
pub const MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;

/// The UTF-8 byte order mark some editors write at the start of a file.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// How the input is split into rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// One item per line (`--lines`).
    Lines,
    /// CSV.
    Csv {
        /// Whether a first row naming a `data` column is a header (false
        /// with `--no-header`).
        detect_header: bool,
    },
}

/// Why the input cannot be used as a batch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    /// Nothing to encode.
    #[error("has no rows to encode")]
    NoRows,
    /// More than [`MAX_ROWS`] rows.
    #[error("has more than {MAX_ROWS} rows, the most one batch encodes")]
    TooManyRows,
    /// The CSV could not be parsed.
    #[error("is not valid CSV (line {line}): {message}")]
    Csv {
        /// The line the problem was found on.
        line: u64,
        /// What the parser reported.
        message: String,
    },
}

/// Reads the batch input from `path` (`-` for stdin), refusing more than
/// [`MAX_INPUT_BYTES`].
///
/// # Errors
///
/// [`CliError::StdinIsTerminal`] for `-` on a terminal, and a
/// [`BatchError`](super::BatchError) for a read failure or an input that
/// is too large.
pub(crate) fn read(
    path: &Path,
    stdin: &mut dyn Read,
    stdin_is_tty: bool,
) -> Result<(InputSource, Vec<u8>), CliError> {
    let too_large = |origin: &InputSource| super::BatchError::InputTooLarge {
        origin: origin.clone(),
        limit: MAX_INPUT_BYTES,
    };
    let read_error = |origin: &InputSource, error| super::BatchError::ReadInput {
        origin: origin.clone(),
        error,
    };
    if path.as_os_str() == "-" {
        let origin = InputSource::Stdin;
        if stdin_is_tty {
            return Err(CliError::StdinIsTerminal);
        }
        let bytes = read_limited(stdin).map_err(|e| read_error(&origin, e))?;
        return match bytes {
            Some(bytes) => Ok((origin, bytes)),
            None => Err(too_large(&origin).into()),
        };
    }
    let origin = InputSource::File(path.to_owned());
    let (file, len) = open_regular_file(path).map_err(|e| read_error(&origin, e))?;
    if len > MAX_INPUT_BYTES {
        return Err(too_large(&origin).into());
    }
    match read_limited(file).map_err(|e| read_error(&origin, e))? {
        Some(bytes) => Ok((origin, bytes)),
        None => Err(too_large(&origin).into()),
    }
}

/// Reads at most [`MAX_INPUT_BYTES`]; `None` if there is more.
fn read_limited(reader: impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= MAX_INPUT_BYTES).then_some(bytes))
}

/// Which columns hold what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Schema {
    /// The column encoded.
    data: usize,
    /// The column `{filename}` reads, if there is one.
    filename: Option<usize>,
    /// Template field names (lower case) and their columns, in column
    /// order. `data` and `filename` are among them.
    columns: Vec<(String, usize)>,
}

impl Schema {
    /// The schema for lines, or for CSV without a header whose first row
    /// has `width` cells.
    fn positional(width: usize) -> Self {
        let filename = (width > 1).then_some(1);
        let mut columns = vec![("data".to_owned(), 0)];
        columns.extend(filename.map(|i| ("filename".to_owned(), i)));
        Self {
            data: 0,
            filename,
            columns,
        }
    }

    /// The schema a header row describes, if it names a `data` column.
    fn from_header(header: &ByteRecord) -> Option<Self> {
        let mut columns: Vec<(String, usize)> = Vec::new();
        for (index, cell) in header.iter().enumerate() {
            let name = String::from_utf8_lossy(cell).trim().to_ascii_lowercase();
            let usable = !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && name != "n"
                && name != "hash";
            if usable && !columns.iter().any(|(seen, _)| *seen == name) {
                columns.push((name, index));
            }
        }
        let find = |wanted: &str| {
            columns
                .iter()
                .find(|(name, _)| name == wanted)
                .map(|&(_, i)| i)
        };
        Some(Self {
            data: find("data")?,
            filename: find("filename"),
            columns,
        })
    }

    /// The column `{filename}` reads, if any.
    pub(crate) fn filename(&self) -> Option<usize> {
        self.filename
    }

    /// The column a template field (lower case) names.
    pub(crate) fn column(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .find(|(column, _)| column == name)
            .map(|&(_, i)| i)
    }

    /// The template field names, in column order.
    pub(crate) fn column_names(&self) -> impl Iterator<Item = &str> {
        self.columns.iter().map(|(name, _)| name.as_str())
    }
}

/// One row to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    /// The row number, from 1, not counting a header or blank lines.
    pub(crate) n: usize,
    /// The input line the row starts on, from 1.
    pub(crate) line: u64,
    /// The cells (one for `--lines`).
    fields: Vec<Vec<u8>>,
    /// Which cell is the data.
    data: usize,
}

impl Row {
    /// The bytes to encode (empty if the row has no such cell).
    pub(crate) fn data(&self) -> &[u8] {
        self.field(self.data)
    }

    /// Cell `index`, or nothing if this row is shorter.
    pub(crate) fn field(&self, index: usize) -> &[u8] {
        self.fields.get(index).map_or(&[], Vec::as_slice)
    }
}

/// The input, split into rows.
#[derive(Debug, Clone)]
pub(crate) struct Table<'a> {
    bytes: &'a [u8],
    layout: Layout,
    schema: Schema,
    header: bool,
    len: usize,
}

impl<'a> Table<'a> {
    /// Splits `bytes` into rows and counts them.
    ///
    /// # Errors
    ///
    /// [`TableError::NoRows`], [`TableError::TooManyRows`] or
    /// [`TableError::Csv`].
    pub(crate) fn parse(bytes: &'a [u8], layout: Layout) -> Result<Self, TableError> {
        let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
        let (schema, header) = match layout {
            Layout::Lines => (Schema::positional(1), false),
            Layout::Csv { detect_header } => {
                let mut reader = csv_reader(bytes);
                let mut first = ByteRecord::new();
                if !reader
                    .read_byte_record(&mut first)
                    .map_err(|e| csv_error(&e))?
                {
                    return Err(TableError::NoRows);
                }
                match Schema::from_header(&first).filter(|_| detect_header) {
                    Some(schema) => (schema, true),
                    None => (Schema::positional(first.len()), false),
                }
            }
        };
        let mut table = Self {
            bytes,
            layout,
            schema,
            header,
            len: 0,
        };
        let mut rows = table.rows();
        let mut len = 0;
        while rows.next_row()?.is_some() {
            len += 1;
            if len > MAX_ROWS {
                return Err(TableError::TooManyRows);
            }
        }
        if len == 0 {
            return Err(TableError::NoRows);
        }
        table.len = len;
        Ok(table)
    }

    /// Which columns hold what.
    pub(crate) fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The number of rows.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The rows, in order.
    pub(crate) fn rows(&self) -> Rows<'a> {
        let inner = match self.layout {
            Layout::Lines => Inner::Lines {
                rest: self.bytes,
                line: 0,
            },
            Layout::Csv { .. } => {
                let mut reader = csv_reader(self.bytes);
                let mut record = ByteRecord::new();
                // The header was read once already, so it parses again.
                if self.header {
                    let _ = reader.read_byte_record(&mut record);
                }
                Inner::Csv {
                    reader,
                    record,
                    bytes: self.bytes,
                    counted: 0,
                    newlines: 0,
                }
            }
        };
        Rows {
            inner,
            n: 0,
            data: self.schema.data,
        }
    }
}

/// A CSV reader for `bytes`: no header handling (done here), rows of any
/// length.
fn csv_reader(bytes: &[u8]) -> csv::Reader<&[u8]> {
    ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes)
}

fn csv_error(error: &csv::Error) -> TableError {
    TableError::Csv {
        line: error.position().map_or(0, csv::Position::line),
        message: error.to_string(),
    }
}

/// The rows of a [`Table`], in order.
pub(crate) struct Rows<'a> {
    inner: Inner<'a>,
    n: usize,
    data: usize,
}

enum Inner<'a> {
    Lines {
        rest: &'a [u8],
        line: u64,
    },
    Csv {
        reader: csv::Reader<&'a [u8]>,
        record: ByteRecord,
        /// The input, to number lines: the parser does not count the
        /// blank lines it skips.
        bytes: &'a [u8],
        /// How far into `bytes` newlines have been counted.
        counted: usize,
        /// The newlines in `bytes[..counted]`.
        newlines: u64,
    },
}

impl Rows<'_> {
    fn next_row(&mut self) -> Result<Option<Row>, TableError> {
        let next = match &mut self.inner {
            Inner::Lines { rest, line } => loop {
                if rest.is_empty() {
                    break None;
                }
                let end = rest.iter().position(|&b| b == b'\n');
                let (text, tail) = match end {
                    Some(end) => (&rest[..end], &rest[end + 1..]),
                    None => (*rest, &rest[rest.len()..]),
                };
                *rest = tail;
                *line += 1;
                let text = text.strip_suffix(b"\r").unwrap_or(text);
                if !text.is_empty() {
                    break Some((*line, vec![text.to_vec()]));
                }
            },
            Inner::Csv {
                reader,
                record,
                bytes,
                counted,
                newlines,
            } => {
                if reader.read_byte_record(record).map_err(|e| csv_error(&e))? {
                    // The reported start is before any blank lines the
                    // parser skipped; no record starts with a line ending.
                    let mut start = record
                        .position()
                        .and_then(|p| usize::try_from(p.byte()).ok())
                        .unwrap_or(*counted)
                        .clamp(*counted, bytes.len());
                    while matches!(bytes.get(start), Some(b'\r' | b'\n')) {
                        start += 1;
                    }
                    let skipped = bytes[*counted..start].iter().filter(|&&b| b == b'\n');
                    *newlines += skipped.count() as u64;
                    *counted = start;
                    Some((*newlines + 1, record.iter().map(<[u8]>::to_vec).collect()))
                } else {
                    None
                }
            }
        };
        Ok(next.map(|(line, fields)| {
            self.n += 1;
            Row {
                n: self.n,
                line,
                fields,
                data: self.data,
            }
        }))
    }
}

impl Iterator for Rows<'_> {
    type Item = Result<Row, TableError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_row().transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::FailingReader;

    const CSV: Layout = Layout::Csv {
        detect_header: true,
    };
    const NO_HEADER: Layout = Layout::Csv {
        detect_header: false,
    };

    fn rows(bytes: &[u8], layout: Layout) -> Vec<(usize, u64, Vec<u8>)> {
        let table = Table::parse(bytes, layout).unwrap();
        let rows: Vec<_> = table
            .rows()
            .map(|r| {
                let r = r.unwrap();
                (r.n, r.line, r.data().to_vec())
            })
            .collect();
        assert_eq!(rows.len(), table.len());
        rows
    }

    #[test]
    fn lines_skip_blanks_and_line_endings() {
        let got = rows(b"\xEF\xBB\xBFone\r\n\n two \nthree", Layout::Lines);
        assert_eq!(
            got,
            [
                (1, 1, b"one".to_vec()),
                (2, 3, b" two ".to_vec()),
                (3, 4, b"three".to_vec())
            ]
        );
        // Commas are data in --lines mode.
        assert_eq!(rows(b"a,b\n", Layout::Lines)[0].2, b"a,b");
    }

    #[test]
    fn csv_header_is_detected_by_a_data_column() {
        let table = Table::parse(b"Filename, DATA \nf1,d1\n\nf2,d2\n", CSV).unwrap();
        assert_eq!(table.schema().filename(), Some(0));
        assert_eq!(table.schema().column("data"), Some(1));
        let got: Vec<_> = table.rows().map(Result::unwrap).collect();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].n, got[0].line, got[0].data()), (1, 2, &b"d1"[..]));
        assert_eq!((got[1].n, got[1].line, got[1].data()), (2, 4, &b"d2"[..]));
        assert_eq!(got[1].field(0), b"f2");
        assert_eq!(got[1].field(9), b"");
    }

    #[test]
    fn without_a_data_column_the_first_row_is_data() {
        let table = Table::parse(b"hello,world.png\nbye\n", CSV).unwrap();
        assert_eq!(table.schema().filename(), Some(1));
        let got: Vec<_> = table.rows().map(Result::unwrap).collect();
        assert_eq!(got[0].data(), b"hello");
        assert_eq!(got[0].field(1), b"world.png");
        assert_eq!(got[1].field(1), b"");
        // --no-header keeps a row that looks like a header.
        let got = rows(b"data\nx\n", NO_HEADER);
        assert_eq!(got[0].2, b"data");
        assert_eq!(got.len(), 2);
        let one = Table::parse(b"a\n", CSV).unwrap();
        assert_eq!(one.schema().filename(), None);
    }

    #[test]
    fn csv_quoting_keeps_commas_newlines_and_bytes() {
        let got = rows(b"data\n\"a,b\"\n\"multi\nline\"\nnext\n\xff\xfe\n", CSV);
        assert_eq!(got[0].2, b"a,b");
        assert_eq!((got[1].1, got[1].2.as_slice()), (3, &b"multi\nline"[..]));
        assert_eq!((got[2].0, got[2].1), (3, 5));
        assert_eq!(got[3].2, b"\xff\xfe");
    }

    #[test]
    fn header_columns_become_fields() {
        let table = Table::parse(b"id,data,n,hash,bad name,ID,x-y,\nx,y\n", CSV).unwrap();
        let names: Vec<_> = table.schema().column_names().collect();
        assert_eq!(names, ["id", "data"]);
        assert_eq!(table.schema().column("id"), Some(0));
    }

    #[test]
    fn empty_and_header_only_inputs_have_no_rows() {
        for (bytes, layout) in [
            (&b""[..], CSV),
            (b"\n\n", CSV),
            (b"data\n", CSV),
            (b"\xEF\xBB\xBF", Layout::Lines),
            (b"\r\n\n", Layout::Lines),
        ] {
            let err = Table::parse(bytes, layout).unwrap_err();
            assert_eq!(err, TableError::NoRows, "{bytes:?}");
        }
        assert_eq!(TableError::NoRows.to_string(), "has no rows to encode");
    }

    #[test]
    fn row_limit_is_enforced_before_any_row_is_used() {
        let mut ok = b"x\n".repeat(MAX_ROWS);
        assert_eq!(Table::parse(&ok, Layout::Lines).unwrap().len(), MAX_ROWS);
        ok.extend_from_slice(b"x\n");
        let err = Table::parse(&ok, Layout::Lines).unwrap_err();
        assert_eq!(err, TableError::TooManyRows);
        assert_eq!(
            err.to_string(),
            "has more than 100000 rows, the most one batch encodes"
        );
        // A header does not count.
        let mut csv = b"data\n".to_vec();
        csv.extend(b"x\n".repeat(MAX_ROWS));
        assert_eq!(Table::parse(&csv, CSV).unwrap().len(), MAX_ROWS);
    }

    #[test]
    fn reads_are_bounded() {
        let big = vec![b'x'; usize::try_from(MAX_INPUT_BYTES).unwrap() + 1];
        let err = read(Path::new("-"), &mut big.as_slice(), false).unwrap_err();
        assert!(matches!(
            err,
            CliError::Batch(super::super::BatchError::InputTooLarge { .. })
        ));
        let exact = &big[1..];
        let (origin, bytes) = read(Path::new("-"), &mut &exact[..], false).unwrap();
        assert_eq!((origin, bytes.len()), (InputSource::Stdin, exact.len()));
        let err = read(Path::new("-"), &mut &b""[..], true).unwrap_err();
        assert!(matches!(err, CliError::StdinIsTerminal));
        let err = read(Path::new("-"), &mut FailingReader, false).unwrap_err();
        assert!(matches!(
            err,
            CliError::Batch(super::super::BatchError::ReadInput { .. })
        ));
    }

    #[test]
    fn files_are_checked_before_they_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.csv");
        std::fs::write(&path, b"a\n").unwrap();
        let (origin, bytes) = read(&path, &mut &b""[..], true).unwrap();
        assert_eq!(
            (origin, bytes),
            (InputSource::File(path.clone()), b"a\n".to_vec())
        );
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_INPUT_BYTES + 1).unwrap();
        let err = read(&path, &mut &b""[..], true).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "'{}' is larger than 50 MiB, the batch input limit",
                path.display()
            )
        );
        let err = read(dir.path(), &mut &b""[..], true).unwrap_err();
        assert!(err.to_string().contains("it is a directory"), "{err}");
    }
}
