//! Typed errors for the CLI layer and their exit statuses and hints.

use std::io;

use qr_core::{EcLevel, EncodeError, max_chars};

use crate::exit::ExitStatus;
use crate::input::InputSource;

/// Errors the CLI reports to the user before exiting with a non-zero status.
///
/// Messages are lowercase, quote the values involved and contain no stack
/// traces, following the UX rules in the design record. Each error maps to
/// one [`ExitStatus`] and may carry a one-line [`hint`](Self::hint).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CliError {
    /// No `TEXT` or `-i FILE`, and stdin is a terminal.
    #[error("no input: stdin is a terminal")]
    StdinIsTerminal,

    /// The input was empty (after stripping a trailing newline).
    #[error("no data to encode: {origin} is empty")]
    EmptyInput {
        /// Where the empty input came from.
        origin: InputSource,
    },

    /// More data was available than any QR code can hold.
    #[error("data too long: {origin} has more than {max} bytes, the most any QR code can hold")]
    InputTooLong {
        /// Where the input came from.
        origin: InputSource,
        /// The largest payload of any QR code, in bytes.
        max: usize,
    },

    /// Reading stdin or the input file failed.
    #[error("cannot read {origin}: {}", io_reason(.error))]
    ReadInput {
        /// What was being read.
        origin: InputSource,
        /// The underlying failure.
        #[source]
        error: io::Error,
    },

    /// The encoder rejected the data.
    #[error(transparent)]
    Encode(#[from] EncodeError),

    /// Writing to standard output failed (for example, a closed pipe).
    #[error("cannot write output: {}", io_reason(.0))]
    Output(#[source] io::Error),

    /// A bug: something that should be impossible happened.
    #[error("internal error: {0}")]
    Internal(String),
}

impl CliError {
    /// The exit status that this error maps to.
    pub const fn exit_status(&self) -> ExitStatus {
        match self {
            Self::StdinIsTerminal
            | Self::EmptyInput { .. }
            | Self::Encode(EncodeError::InvalidCharacter { .. }) => ExitStatus::Usage,
            Self::InputTooLong { .. } | Self::Encode(EncodeError::DataTooLong(_)) => {
                ExitStatus::DataTooLong
            }
            Self::ReadInput { .. } | Self::Output(_) => ExitStatus::Io,
            Self::Encode(_) | Self::Internal(_) => ExitStatus::Internal,
        }
    }

    /// One actionable suggestion for the user, if there is one.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::StdinIsTerminal | Self::EmptyInput { .. } => {
                Some("pass TEXT, pipe data to stdin or use -i FILE (see 'qr --help')".to_owned())
            }
            Self::Encode(EncodeError::DataTooLong(e)) if e.ec_level != EcLevel::L => Some(format!(
                "shorten the input or lower --ec (now {}; at L it holds up to {} {})",
                e.ec_level,
                max_chars(e.mode, e.version, EcLevel::L),
                e.mode.unit()
            )),
            Self::InputTooLong { .. } | Self::Encode(EncodeError::DataTooLong(_)) => {
                Some("shorten the input".to_owned())
            }
            Self::Encode(EncodeError::InvalidCharacter { .. }) => {
                Some("use --mode auto to pick a mode that fits the data".to_owned())
            }
            Self::Encode(_) | Self::Internal(_) => {
                Some("this is a bug; please report it".to_owned())
            }
            Self::ReadInput {
                origin: InputSource::File(_),
                ..
            } => Some("check the path passed to -i".to_owned()),
            Self::ReadInput { .. } | Self::Output(_) => None,
        }
    }
}

/// A lowercase description of an I/O failure.
///
/// The standard library's text comes from the OS ("No such file or
/// directory (os error 2)"), which breaks the lowercase message style, so
/// common kinds get fixed wording. Anything else keeps its own text with
/// the first letter lowered.
fn io_reason(error: &io::Error) -> String {
    let fixed = match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory",
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::IsADirectory => "it is a directory",
        io::ErrorKind::BrokenPipe => "the pipe was closed",
        io::ErrorKind::InvalidData => "the data is invalid",
        io::ErrorKind::Interrupted => "the operation was interrupted",
        io::ErrorKind::OutOfMemory => "out of memory",
        io::ErrorKind::StorageFull => "no space left on the device",
        _ => {
            let text = error.to_string();
            let mut chars = text.chars();
            return chars.next().map_or_else(String::new, |first| {
                first.to_lowercase().chain(chars).collect()
            });
        }
    };
    fixed.to_owned()
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;
    use std::io;

    use qr_core::{CapacityError, EcLevel, Mode, Version};

    use super::*;

    fn too_long(ec_level: EcLevel) -> CliError {
        CliError::Encode(EncodeError::DataTooLong(CapacityError {
            needed: 3_000,
            max: 2_953,
            mode: Mode::Byte,
            ec_level,
            version: Version::MAX,
        }))
    }

    #[test]
    fn output_error_maps_to_io_status_and_keeps_source() {
        let err = CliError::Output(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"));
        assert_eq!(err.exit_status(), ExitStatus::Io);
        assert_eq!(err.to_string(), "cannot write output: the pipe was closed");
        assert!(err.source().is_some());
        assert_eq!(err.hint(), None);
    }

    #[test]
    fn every_variant_maps_to_its_documented_status() {
        let invalid = CliError::Encode(EncodeError::InvalidCharacter {
            mode: Mode::Numeric,
            offset: 0,
            byte: b'x',
        });
        let table = [
            (CliError::StdinIsTerminal, ExitStatus::Usage),
            (
                CliError::EmptyInput {
                    origin: InputSource::Stdin,
                },
                ExitStatus::Usage,
            ),
            (invalid, ExitStatus::Usage),
            (
                CliError::InputTooLong {
                    origin: InputSource::Stdin,
                    max: 7_089,
                },
                ExitStatus::DataTooLong,
            ),
            (too_long(EcLevel::M), ExitStatus::DataTooLong),
            (
                CliError::ReadInput {
                    origin: InputSource::Stdin,
                    error: io::Error::other("x"),
                },
                ExitStatus::Io,
            ),
            (
                CliError::Encode(EncodeError::Internal("x".into())),
                ExitStatus::Internal,
            ),
            (CliError::Internal("x".into()), ExitStatus::Internal),
        ];
        for (err, status) in table {
            assert_eq!(err.exit_status(), status, "{err}");
            if status != ExitStatus::Io {
                assert!(err.hint().is_some(), "{err}");
            }
        }
    }

    #[test]
    fn io_reasons_are_lowercase() {
        let kinds = [
            (io::ErrorKind::NotFound, "no such file or directory"),
            (io::ErrorKind::PermissionDenied, "permission denied"),
            (io::ErrorKind::IsADirectory, "it is a directory"),
            (io::ErrorKind::BrokenPipe, "the pipe was closed"),
            (io::ErrorKind::InvalidData, "the data is invalid"),
            (io::ErrorKind::Interrupted, "the operation was interrupted"),
            (io::ErrorKind::OutOfMemory, "out of memory"),
            (io::ErrorKind::StorageFull, "no space left on the device"),
        ];
        for (kind, text) in kinds {
            assert_eq!(io_reason(&io::Error::new(kind, "Raw OS Text")), text);
        }
        assert_eq!(
            io_reason(&io::Error::other("Device on fire")),
            "device on fire"
        );
        assert_eq!(io_reason(&io::Error::other("")), "");
        assert_eq!(
            io_reason(&io::Error::from_raw_os_error(2)),
            "no such file or directory"
        );
    }

    #[test]
    fn read_errors_hint_at_the_path_only_for_files() {
        let file = CliError::ReadInput {
            origin: InputSource::File("x".into()),
            error: io::ErrorKind::NotFound.into(),
        };
        assert_eq!(
            file.to_string(),
            "cannot read 'x': no such file or directory"
        );
        assert_eq!(file.hint().unwrap(), "check the path passed to -i");
        let stdin = CliError::ReadInput {
            origin: InputSource::Stdin,
            error: io::Error::other("x"),
        };
        assert_eq!(stdin.hint(), None);
    }

    #[test]
    fn data_too_long_hint_suggests_lowering_ec_only_when_possible() {
        let m = too_long(EcLevel::M).hint().unwrap();
        assert_eq!(
            m,
            "shorten the input or lower --ec (now M; at L it holds up to 2953 bytes)"
        );
        assert_eq!(too_long(EcLevel::L).hint().unwrap(), "shorten the input");
        assert_eq!(
            too_long(EcLevel::L).to_string(),
            "data too long: 3000 bytes exceeds the maximum of 2953 for byte mode at error correction level L (version 40)"
        );
    }

    #[test]
    fn messages_quote_the_source() {
        let err = CliError::InputTooLong {
            origin: InputSource::File("big.txt".into()),
            max: 7_089,
        };
        assert_eq!(
            err.to_string(),
            "data too long: 'big.txt' has more than 7089 bytes, the most any QR code can hold"
        );
        assert_eq!(err.hint().unwrap(), "shorten the input");
        assert_eq!(
            CliError::Internal("boom".into()).to_string(),
            "internal error: boom"
        );
    }
}
