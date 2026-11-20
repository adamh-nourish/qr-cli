//! Typed errors for the CLI layer and their exit statuses and hints.

use std::io;
use std::path::PathBuf;

use qr_core::payload::PayloadError;
use qr_core::verify::VerifyError;
use qr_core::{EcLevel, EncodeError, RenderError, max_chars};

use crate::cli::OutputFormat;
use crate::exit::ExitStatus;
use crate::input::InputSource;
use crate::output::{WriteError, known_extensions};
pub use crate::verify::Remedy;

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

    /// `--password-stdin` (or another secret read from stdin) and stdin is
    /// a terminal, where the secret would be echoed as it is typed.
    #[error("no password: stdin is a terminal")]
    SecretStdinIsTerminal,

    /// A payload subcommand's fields are invalid.
    #[error(transparent)]
    Payload(#[from] PayloadError),

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

    /// `-o PATH` without `-f`, and the extension names no known format.
    #[error("cannot infer the output format from '{}'", .0.display())]
    UnknownExtension(PathBuf),

    /// Binary output was headed for a terminal.
    #[error("refusing to write binary {} output to a terminal", .0.name())]
    BinaryToTerminal(OutputFormat),

    /// `--data-uri` with a text format.
    #[error("--data-uri needs an image format, not {}", .0.name())]
    DataUriNeedsImage(OutputFormat),

    /// `--json` while the code itself would also go to stdout.
    #[error("--json cannot share stdout with the {} output", .0.name())]
    JsonNeedsOutput(OutputFormat),

    /// The renderer rejected the options (for example, too large).
    #[error(transparent)]
    Render(#[from] RenderError),

    /// The rendered code did not read back as the input, or the check
    /// could not run.
    #[error("{error}")]
    Verify {
        /// What went wrong.
        #[source]
        error: VerifyError,
        /// Whether the check ran automatically, for a styled image, rather
        /// than because of `--verify`.
        automatic: bool,
        /// What is most likely to make the code read back.
        remedy: Remedy,
    },

    /// `--no-clobber` and the output file exists.
    #[error("output file '{}' already exists", .0.display())]
    OutputExists(PathBuf),

    /// The `-o` path is a FIFO, device or socket, not a file to replace.
    #[error("output path '{}' is not a regular file", .0.display())]
    OutputNotAFile(PathBuf),

    /// Creating or writing the output file failed.
    #[error("cannot write '{}': {}", .path.display(), io_reason(.error))]
    WriteFile {
        /// The output path.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        error: io::Error,
    },

    /// A bug: something that should be impossible happened.
    #[error("internal error: {0}")]
    Internal(String),
}

impl CliError {
    /// The exit status that this error maps to.
    pub const fn exit_status(&self) -> ExitStatus {
        match self {
            Self::StdinIsTerminal
            | Self::SecretStdinIsTerminal
            | Self::Payload(_)
            | Self::EmptyInput { .. }
            | Self::Encode(EncodeError::InvalidCharacter { .. })
            | Self::UnknownExtension(_)
            | Self::BinaryToTerminal(_)
            | Self::DataUriNeedsImage(_)
            | Self::JsonNeedsOutput(_) => ExitStatus::Usage,
            Self::Verify { error, .. } if error.is_failure() => ExitStatus::VerificationFailed,
            // An encoder failing on valid options is not the user's fault.
            Self::Render(RenderError::Encoder { .. })
            | Self::Verify {
                error: VerifyError::Render(RenderError::Encoder { .. }),
                ..
            } => ExitStatus::Internal,
            // Every other render error is a size or option limit.
            #[allow(clippy::match_same_arms)]
            Self::Render(_)
            | Self::Verify {
                error: VerifyError::Render(_),
                ..
            } => ExitStatus::Usage,
            Self::InputTooLong { .. } | Self::Encode(EncodeError::DataTooLong(_)) => {
                ExitStatus::DataTooLong
            }
            Self::ReadInput { .. }
            | Self::Output(_)
            | Self::OutputExists(_)
            | Self::OutputNotAFile(_)
            | Self::WriteFile { .. } => ExitStatus::Io,
            // A verify error left here is our own output failing to decode.
            Self::Encode(_) | Self::Internal(_) | Self::Verify { .. } => ExitStatus::Internal,
        }
    }

    /// A stable machine-readable name for the error, used by `--json`.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::StdinIsTerminal | Self::SecretStdinIsTerminal => "no_input",
            Self::Payload(_) => "invalid_payload",
            Self::EmptyInput { .. } => "empty_input",
            Self::InputTooLong { .. } | Self::Encode(EncodeError::DataTooLong(_)) => {
                "data_too_long"
            }
            Self::Encode(EncodeError::InvalidCharacter { .. }) => "invalid_character",
            Self::ReadInput { .. } => "read_failed",
            Self::Output(_) | Self::WriteFile { .. } => "write_failed",
            Self::UnknownExtension(_) => "unknown_extension",
            Self::BinaryToTerminal(_) => "binary_to_terminal",
            Self::DataUriNeedsImage(_) => "data_uri_needs_image",
            Self::JsonNeedsOutput(_) => "json_needs_output",
            Self::Verify { error, .. } if error.is_failure() => "verification_failed",
            Self::Render(_)
            | Self::Verify {
                error: VerifyError::Render(_),
                ..
            } => "render_failed",
            Self::OutputExists(_) => "output_exists",
            Self::OutputNotAFile(_) => "output_not_a_file",
            Self::Encode(_) | Self::Internal(_) | Self::Verify { .. } => "internal",
        }
    }

    /// One actionable suggestion for the user, if there is one.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::StdinIsTerminal | Self::EmptyInput { .. } => {
                Some("pass TEXT, pipe data to stdin or use -i FILE (see 'qr --help')".to_owned())
            }
            Self::SecretStdinIsTerminal => Some(
                "pipe it in, for example: qr wifi --ssid NAME --password-stdin < password.txt"
                    .to_owned(),
            ),
            Self::Payload(PayloadError::ControlCharacter { .. }) => {
                Some("remove the control character from the value".to_owned())
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
            Self::UnknownExtension(_) => Some(format!(
                "pass -f FORMAT or use one of {}",
                known_extensions()
            )),
            Self::BinaryToTerminal(f) => Some(format!(
                "redirect stdout to a file or use -o code.{}",
                f.name()
            )),
            Self::DataUriNeedsImage(_) => Some("use -f svg, -f pdf or -f eps".to_owned()),
            Self::JsonNeedsOutput(_) => Some(
                "write the code to a file with -o PATH, or add --data-uri to put it in the JSON"
                    .to_owned(),
            ),
            Self::OutputExists(_) => {
                Some("remove it, choose another path or drop --no-clobber".to_owned())
            }
            Self::OutputNotAFile(_) => {
                Some("write to a regular file, or use -o - for stdout".to_owned())
            }
            Self::WriteFile { error, .. } if error.kind() == io::ErrorKind::NotFound => {
                Some("check that the directory exists".to_owned())
            }
            Self::WriteFile { error, .. } if error.kind() == io::ErrorKind::PermissionDenied => {
                Some("check that you can write to the directory (use -o - for stdout)".to_owned())
            }
            Self::Verify {
                error,
                automatic,
                remedy,
            } if error.is_failure() => Some(if *automatic {
                format!("{} (or --no-verify to write it anyway)", remedy.hint())
            } else {
                remedy.hint().to_owned()
            }),
            Self::Verify { error, .. } if !matches!(error, VerifyError::Render(_)) => {
                Some("this is a bug; please report it".to_owned())
            }
            // A render or payload error's message already says what to change.
            Self::Verify { .. }
            | Self::ReadInput { .. }
            | Self::Output(_)
            | Self::WriteFile { .. }
            | Self::Render(_)
            | Self::Payload(_) => None,
        }
    }
}

impl From<WriteError> for CliError {
    fn from(err: WriteError) -> Self {
        match err {
            WriteError::Exists(path) => Self::OutputExists(path),
            WriteError::NotAFile(path) => Self::OutputNotAFile(path),
            WriteError::Io { path, error } => Self::WriteFile { path, error },
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
    fn payload_errors_are_usage_errors() {
        let control = CliError::from(PayloadError::ControlCharacter {
            field: "SSID",
            character: '\n',
        });
        assert_eq!(control.exit_status(), ExitStatus::Usage);
        assert_eq!(control.code(), "invalid_payload");
        assert_eq!(
            control.to_string(),
            "the SSID contains the control character U+000A"
        );
        assert!(control.hint().is_some());
        let empty = CliError::from(PayloadError::Empty { field: "SSID" });
        assert_eq!(empty.hint(), None);
        let tty = CliError::SecretStdinIsTerminal;
        assert_eq!(tty.exit_status(), ExitStatus::Usage);
        assert_eq!(tty.code(), "no_input");
        assert!(tty.hint().unwrap().contains("--password-stdin"));
    }

    #[test]
    fn verify_errors_have_statuses_codes_and_hints() {
        let verify = |error, automatic| CliError::Verify {
            error,
            automatic,
            remedy: Remedy::Other { square: false },
        };
        let not_found = || VerifyError::NotFound { format: "png" };
        let explicit = verify(not_found(), false);
        assert_eq!(explicit.exit_status(), ExitStatus::VerificationFailed);
        assert_eq!(explicit.code(), "verification_failed");
        assert_eq!(
            explicit.to_string(),
            "no QR code could be read back from the rendered png"
        );
        assert!(!explicit.hint().unwrap().contains("--no-verify"));
        assert!(explicit.source().is_some());
        let auto = verify(
            VerifyError::Mismatch {
                format: "svg",
                expected: 1,
                decoded: 2,
            },
            true,
        );
        assert_eq!(auto.exit_status(), ExitStatus::VerificationFailed);
        assert!(auto.hint().unwrap().contains("--no-verify"));

        let table = [
            (
                verify(VerifyError::Render(RenderError::ZeroScale), true),
                ExitStatus::Usage,
                "render_failed",
                false,
            ),
            (
                verify(
                    VerifyError::Render(RenderError::Encoder {
                        format: "verify",
                        message: "oom".into(),
                    }),
                    false,
                ),
                ExitStatus::Internal,
                "render_failed",
                false,
            ),
            (
                verify(
                    VerifyError::ReadBack {
                        format: "png",
                        message: "bad".into(),
                    },
                    true,
                ),
                ExitStatus::Internal,
                "internal",
                true,
            ),
        ];
        for (err, status, code, hint) in table {
            assert_eq!(err.exit_status(), status, "{err}");
            assert_eq!(err.code(), code, "{err}");
            assert_eq!(err.hint().is_some(), hint, "{err}");
        }
    }

    #[test]
    fn output_errors_have_statuses_codes_and_hints() {
        use crate::cli::OutputFormat;
        let table = [
            (
                CliError::UnknownExtension("a.gif".into()),
                ExitStatus::Usage,
                "unknown_extension",
            ),
            (
                CliError::OutputNotAFile("p".into()),
                ExitStatus::Io,
                "output_not_a_file",
            ),
            (
                CliError::BinaryToTerminal(OutputFormat::Pdf),
                ExitStatus::Usage,
                "binary_to_terminal",
            ),
            (
                CliError::DataUriNeedsImage(OutputFormat::Text),
                ExitStatus::Usage,
                "data_uri_needs_image",
            ),
            (
                CliError::JsonNeedsOutput(OutputFormat::Text),
                ExitStatus::Usage,
                "json_needs_output",
            ),
            (
                CliError::OutputExists("a".into()),
                ExitStatus::Io,
                "output_exists",
            ),
        ];
        for (err, status, code) in table {
            assert_eq!(err.exit_status(), status, "{err}");
            assert_eq!(err.code(), code, "{err}");
            assert!(err.hint().is_some(), "{err}");
        }
        let render = CliError::Render(RenderError::ZeroScale);
        assert_eq!(
            (render.exit_status(), render.code(), render.hint()),
            (ExitStatus::Usage, "render_failed", None)
        );
        let encoder = CliError::Render(RenderError::Encoder {
            format: "png",
            message: "x".into(),
        });
        assert_eq!(
            (encoder.exit_status(), encoder.code(), encoder.hint()),
            (ExitStatus::Internal, "render_failed", None)
        );
    }

    #[test]
    fn every_error_has_a_code() {
        let codes = [
            (CliError::StdinIsTerminal, "no_input"),
            (
                CliError::EmptyInput {
                    origin: InputSource::Stdin,
                },
                "empty_input",
            ),
            (too_long(EcLevel::M), "data_too_long"),
            (
                CliError::InputTooLong {
                    origin: InputSource::Stdin,
                    max: 1,
                },
                "data_too_long",
            ),
            (
                CliError::Encode(EncodeError::InvalidCharacter {
                    mode: Mode::Numeric,
                    offset: 0,
                    byte: b'x',
                }),
                "invalid_character",
            ),
            (
                CliError::ReadInput {
                    origin: InputSource::Stdin,
                    error: io::Error::other("x"),
                },
                "read_failed",
            ),
            (CliError::Output(io::Error::other("x")), "write_failed"),
            (CliError::Internal("x".into()), "internal"),
            (
                CliError::Encode(EncodeError::Internal("x".into())),
                "internal",
            ),
        ];
        for (err, code) in codes {
            assert_eq!(err.code(), code, "{err}");
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
