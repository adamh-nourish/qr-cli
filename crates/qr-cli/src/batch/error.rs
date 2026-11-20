//! Errors specific to `qr batch`, with their exit statuses and hints.

use std::io;
use std::path::PathBuf;

use super::name::NameError;
use super::source::TableError;
use super::template::TemplateError;
use crate::error::io_reason;
use crate::exit::ExitStatus;
use crate::input::InputSource;

/// Something `qr batch` could not do: a problem with the whole batch (the
/// input, `--name`, `--out-dir`) or with one row's file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BatchError {
    /// `-f terminal`, which cannot be written to files.
    #[error("qr batch writes files, and terminal output is not a file format")]
    TerminalFormat,

    /// The input could not be opened or read.
    #[error("cannot read {origin}: {}", io_reason(.error))]
    ReadInput {
        /// What was being read.
        origin: InputSource,
        /// The underlying failure.
        #[source]
        error: io::Error,
    },

    /// The input is over the size limit.
    #[error("{origin} is larger than {} MiB, the batch input limit", .limit / (1024 * 1024))]
    InputTooLarge {
        /// The input.
        origin: InputSource,
        /// The limit in bytes.
        limit: u64,
    },

    /// The input has no rows, too many, or is not valid CSV.
    #[error("{origin} {error}")]
    Table {
        /// The input.
        origin: InputSource,
        /// What is wrong with it.
        #[source]
        error: TableError,
    },

    /// `--name` is not a usable template for this input.
    #[error(transparent)]
    Template(#[from] TemplateError),

    /// `--out-dir` could not be created or is not a directory.
    #[error("cannot use output directory '{}': {}", .path.display(), io_reason(.error))]
    OutDir {
        /// The directory as given.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        error: io::Error,
    },

    /// The worker threads could not be started.
    #[error("cannot start {jobs} worker threads: {message}")]
    Pool {
        /// The number of threads asked for.
        jobs: usize,
        /// What went wrong.
        message: String,
    },

    /// A row's data is empty.
    #[error("the data is empty")]
    EmptyData,

    /// A row's file name is unusable.
    #[error(transparent)]
    Name(#[from] NameError),

    /// Two rows name the same file (names that differ only in case count
    /// as the same, as on macOS and Windows).
    #[error("'{}' is also the file of row {first}", .path.display())]
    DuplicateName {
        /// The file both rows name.
        path: PathBuf,
        /// The earlier row.
        first: usize,
    },

    /// A row's file exists and `--force` was not given.
    #[error("'{}' already exists", .0.display())]
    Exists(PathBuf),

    /// Some rows failed. The exit status is that of the first failure.
    #[error("{failed} of {rows} rows failed{}; {written} written to '{}'",
        skipped_note(*.skipped), .out_dir.display())]
    Incomplete {
        /// Rows in the input.
        rows: usize,
        /// Rows written.
        written: usize,
        /// Rows that failed.
        failed: usize,
        /// Rows not tried after `--fail-fast` stopped the batch.
        skipped: usize,
        /// The output directory as given.
        out_dir: PathBuf,
        /// The exit status of the first failed row.
        status: ExitStatus,
    },
}

/// The `Incomplete` note on rows `--fail-fast` skipped.
fn skipped_note(skipped: usize) -> String {
    if skipped == 0 {
        String::new()
    } else {
        format!(" and {skipped} were skipped (--fail-fast)")
    }
}

impl BatchError {
    /// The exit status this error maps to.
    pub const fn exit_status(&self) -> ExitStatus {
        match self {
            Self::TerminalFormat
            | Self::InputTooLarge { .. }
            | Self::Table { .. }
            | Self::Template(_)
            | Self::EmptyData
            | Self::Name(_)
            | Self::DuplicateName { .. } => ExitStatus::Usage,
            Self::ReadInput { .. } | Self::OutDir { .. } | Self::Exists(_) => ExitStatus::Io,
            Self::Pool { .. } => ExitStatus::Internal,
            Self::Incomplete { status, .. } => *status,
        }
    }

    /// A stable machine-readable name for the error, used by `--json`.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TerminalFormat => "terminal_not_a_file",
            Self::ReadInput { .. } => "read_failed",
            Self::InputTooLarge { .. } => "input_too_large",
            Self::Table {
                error: TableError::NoRows,
                ..
            } => "no_rows",
            Self::Table {
                error: TableError::TooManyRows,
                ..
            } => "too_many_rows",
            Self::Table {
                error: TableError::Csv { .. },
                ..
            } => "invalid_csv",
            Self::Template(_) => "invalid_template",
            Self::OutDir { .. } => "out_dir_failed",
            Self::Pool { .. } => "internal",
            Self::EmptyData => "empty_input",
            Self::Name(_) => "invalid_name",
            Self::DuplicateName { .. } => "duplicate_name",
            Self::Exists(_) => "output_exists",
            Self::Incomplete { .. } => "batch_incomplete",
        }
    }

    /// One actionable suggestion, if there is one.
    pub fn hint(&self) -> Option<String> {
        let hint = match self {
            Self::TerminalFormat => "use -f png, svg, text or another file format",
            Self::ReadInput { .. } => "check the FILE path",
            Self::InputTooLarge { .. } => "split the input into smaller files",
            Self::Table {
                error: TableError::NoRows,
                ..
            } => "put one item per row (CSV) or per line (--lines)",
            Self::Table {
                error: TableError::TooManyRows,
                ..
            } => "split the input into batches of at most 100000 rows",
            Self::Table {
                error: TableError::Csv { .. },
                ..
            } => "check the quoting, or use --lines for one item per line",
            Self::Template(TemplateError::UnknownField { .. }) => {
                "a CSV header names the columns; without one only {n}, {hash}, {data} and {filename} exist"
            }
            Self::Template(_) => "for example: --name 'code-{n:04}'",
            Self::OutDir { .. } => "check that --out-dir is a directory you can write to",
            Self::Pool { .. } => "try a smaller --jobs",
            Self::EmptyData => "remove the row or give it data",
            Self::Name(NameError::Reserved(_)) => {
                "rename the row, or add text around the field in --name"
            }
            Self::Name(_) => "fill in the field --name uses, or use --name '{n}'",
            Self::DuplicateName { .. } => {
                "make the names unique, for example --name '{n}-{filename}'"
            }
            Self::Exists(_) => "pass --force to replace existing files",
            Self::Incomplete { .. } => return None,
        };
        Some(hint.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::super::source::MAX_INPUT_BYTES;
    use super::*;

    #[test]
    #[allow(clippy::too_many_lines)] // A table of cases.
    fn statuses_codes_and_hints() {
        let origin = InputSource::File("rows.csv".into());
        let table = |error| BatchError::Table {
            origin: origin.clone(),
            error,
        };
        let cases: Vec<(BatchError, ExitStatus, &str)> = vec![
            (
                BatchError::TerminalFormat,
                ExitStatus::Usage,
                "terminal_not_a_file",
            ),
            (
                BatchError::ReadInput {
                    origin: origin.clone(),
                    error: io::ErrorKind::NotFound.into(),
                },
                ExitStatus::Io,
                "read_failed",
            ),
            (
                BatchError::InputTooLarge {
                    origin: origin.clone(),
                    limit: MAX_INPUT_BYTES,
                },
                ExitStatus::Usage,
                "input_too_large",
            ),
            (table(TableError::NoRows), ExitStatus::Usage, "no_rows"),
            (
                table(TableError::TooManyRows),
                ExitStatus::Usage,
                "too_many_rows",
            ),
            (
                table(TableError::Csv {
                    line: 3,
                    message: "bad".into(),
                }),
                ExitStatus::Usage,
                "invalid_csv",
            ),
            (
                BatchError::Template(TemplateError::Empty),
                ExitStatus::Usage,
                "invalid_template",
            ),
            (
                BatchError::Template(TemplateError::UnknownField {
                    field: "x".into(),
                    available: "{n}".into(),
                }),
                ExitStatus::Usage,
                "invalid_template",
            ),
            (
                BatchError::OutDir {
                    path: "out".into(),
                    error: io::ErrorKind::PermissionDenied.into(),
                },
                ExitStatus::Io,
                "out_dir_failed",
            ),
            (
                BatchError::Pool {
                    jobs: 4,
                    message: "no".into(),
                },
                ExitStatus::Internal,
                "internal",
            ),
            (BatchError::EmptyData, ExitStatus::Usage, "empty_input"),
            (
                BatchError::Name(NameError::Empty),
                ExitStatus::Usage,
                "invalid_name",
            ),
            (
                BatchError::Name(NameError::Reserved("con.png".into())),
                ExitStatus::Usage,
                "invalid_name",
            ),
            (
                BatchError::DuplicateName {
                    path: "out/a.png".into(),
                    first: 1,
                },
                ExitStatus::Usage,
                "duplicate_name",
            ),
            (
                BatchError::Exists("out/a.png".into()),
                ExitStatus::Io,
                "output_exists",
            ),
        ];
        for (err, status, code) in cases {
            assert_eq!(err.exit_status(), status, "{err}");
            assert_eq!(err.code(), code, "{err}");
            assert!(err.hint().is_some(), "{err}");
            assert_ne!(err.to_string(), "");
        }
    }

    #[test]
    fn incomplete_takes_the_first_failure_status() {
        let err = BatchError::Incomplete {
            rows: 10,
            written: 7,
            failed: 2,
            skipped: 1,
            out_dir: "out".into(),
            status: ExitStatus::DataTooLong,
        };
        assert_eq!(err.exit_status(), ExitStatus::DataTooLong);
        assert_eq!(err.code(), "batch_incomplete");
        assert_eq!(err.hint(), None);
        assert_eq!(
            err.to_string(),
            "2 of 10 rows failed and 1 were skipped (--fail-fast); 7 written to 'out'"
        );
        let err = BatchError::Incomplete {
            rows: 3,
            written: 2,
            failed: 1,
            skipped: 0,
            out_dir: "o".into(),
            status: ExitStatus::Io,
        };
        assert_eq!(err.to_string(), "1 of 3 rows failed; 2 written to 'o'");
    }
}
