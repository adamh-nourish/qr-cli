//! Resolving and reading the data to encode.
//!
//! Exactly one source is used: the `TEXT` argument, stdin (`-` or `-i -`,
//! or implicitly when there is no `TEXT` and stdin is not a terminal) or a
//! file given with `-i`. Clap rejects `TEXT` together with `-i`. Reads from
//! stdin and files go through [`Read::take`], so a huge input is never
//! buffered in full.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::error::CliError;

/// The most data bytes any QR code can hold: 7,089 digits in numeric mode
/// at version 40-L.
pub const MAX_PAYLOAD_BYTES: usize = 7_089;

/// The most bytes read from stdin or a file: the largest payload plus a
/// trailing CRLF, which is stripped before encoding. Anything longer cannot
/// fit in a QR code, so reading stops there.
pub const MAX_READ_BYTES: usize = MAX_PAYLOAD_BYTES + 2;

/// Where the data came from. Used in messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSource {
    /// The `TEXT` argument.
    Argument,
    /// Standard input.
    Stdin,
    /// A file given with `-i`.
    File(PathBuf),
    /// A payload subcommand, such as `wifi`.
    Payload(&'static str),
}

impl fmt::Display for InputSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Argument => f.write_str("the TEXT argument"),
            Self::Stdin => f.write_str("stdin"),
            Self::File(path) => write!(f, "'{}'", path.display()),
            Self::Payload(kind) => write!(f, "the {kind} payload"),
        }
    }
}

/// The data to encode and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// The bytes to encode.
    pub data: Vec<u8>,
    /// Where they came from.
    pub source: InputSource,
}

/// How to read the input, as given on the command line.
#[derive(Debug, Clone, Copy)]
pub struct InputRequest<'a> {
    /// The `TEXT` argument, if any (`-` means stdin).
    pub text: Option<&'a str>,
    /// The `-i FILE` argument, if any (`-` means stdin).
    pub file: Option<&'a Path>,
    /// Keep one trailing newline from stdin or a file.
    pub keep_newline: bool,
}

/// Resolves the input source and reads it.
///
/// `TEXT` is used verbatim. Data from stdin or a file loses one trailing
/// `\n` or `\r\n` unless `keep_newline` is set.
///
/// # Errors
///
/// - [`CliError::StdinIsTerminal`] when stdin would be read but is a
///   terminal, so `qr` never sits waiting for typed input;
/// - [`CliError::ReadInput`] when stdin or the file cannot be read;
/// - [`CliError::InputTooLong`] when more than [`MAX_READ_BYTES`] are
///   available;
/// - [`CliError::EmptyInput`] when there is nothing to encode.
pub fn read_input<R: Read>(
    request: &InputRequest<'_>,
    stdin: R,
    stdin_is_tty: bool,
) -> Result<Input, CliError> {
    let source = match (request.text, request.file) {
        (Some(text), _) if text != "-" => {
            return non_empty(Input {
                data: text.as_bytes().to_vec(),
                source: InputSource::Argument,
            });
        }
        // `-i -` means stdin, as `-` does for TEXT.
        (_, Some(path)) if path != Path::new("-") => InputSource::File(path.to_path_buf()),
        _ => InputSource::Stdin,
    };

    let mut data = match &source {
        InputSource::File(path) => {
            let file = File::open(path).map_err(|error| CliError::ReadInput {
                origin: source.clone(),
                error,
            })?;
            read_bounded(file, &source)?
        }
        _ if stdin_is_tty => return Err(CliError::StdinIsTerminal),
        _ => read_bounded(stdin, &source)?,
    };
    if !request.keep_newline {
        strip_trailing_newline(&mut data);
    }
    non_empty(Input { data, source })
}

fn non_empty(input: Input) -> Result<Input, CliError> {
    if input.data.is_empty() {
        Err(CliError::EmptyInput {
            origin: input.source,
        })
    } else {
        Ok(input)
    }
}

/// Reads at most [`MAX_READ_BYTES`], failing if there is more.
fn read_bounded<R: Read>(reader: R, source: &InputSource) -> Result<Vec<u8>, CliError> {
    let mut data = Vec::new();
    // One extra byte tells "exactly at the limit" from "over the limit".
    let limit = u64::try_from(MAX_READ_BYTES + 1).unwrap_or(u64::MAX);
    reader
        .take(limit)
        .read_to_end(&mut data)
        .map_err(|error| CliError::ReadInput {
            origin: source.clone(),
            error,
        })?;
    if data.len() > MAX_READ_BYTES {
        return Err(CliError::InputTooLong {
            origin: source.clone(),
            max: MAX_PAYLOAD_BYTES,
        });
    }
    Ok(data)
}

/// Opens `path` for reading if it is a regular file, and returns it with
/// its length.
///
/// The type is checked before opening, because opening a FIFO blocks and
/// a device can be endless: images (`--logo`, `qr decode`) are only read
/// from regular files, and the length lets the bounded loader refuse an
/// oversized file before reading it.
///
/// # Errors
///
/// The metadata or open failure; [`io::ErrorKind::IsADirectory`] for a
/// directory; an error reading "not a regular file" for anything else that
/// is not a file.
pub(crate) fn open_regular_file(path: &Path) -> io::Result<(File, u64)> {
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        return Err(io::Error::from(io::ErrorKind::IsADirectory));
    }
    if !metadata.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok((File::open(path)?, metadata.len()))
}

/// Removes one trailing `\n` or `\r\n`.
fn strip_trailing_newline(data: &mut Vec<u8>) {
    if data.ends_with(b"\r\n") {
        data.truncate(data.len() - 2);
    } else if data.ends_with(b"\n") {
        data.truncate(data.len() - 1);
    }
}

/// A reader that always fails, for tests.
#[cfg(test)]
pub(crate) struct FailingReader;

#[cfg(test)]
impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("device on fire"))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write as _};

    use super::*;
    use crate::exit::ExitStatus;

    fn request<'a>(text: Option<&'a str>, file: Option<&'a Path>) -> InputRequest<'a> {
        InputRequest {
            text,
            file,
            keep_newline: false,
        }
    }

    #[test]
    fn text_argument_is_used_verbatim() {
        let input = read_input(&request(Some("hi\n"), None), FailingReader, true).unwrap();
        assert_eq!(input.data, b"hi\n");
        assert_eq!(input.source, InputSource::Argument);
    }

    #[test]
    fn dash_and_no_argument_read_stdin() {
        let dash = Path::new("-");
        for (text, file) in [(Some("-"), None), (None, None), (None, Some(dash))] {
            let input = read_input(&request(text, file), &b"hello\n"[..], false).unwrap();
            assert_eq!(input.data, b"hello");
            assert_eq!(input.source, InputSource::Stdin);
        }
        // `-i -` is still guarded against an interactive stdin.
        let err = read_input(&request(None, Some(dash)), FailingReader, true).unwrap_err();
        assert!(matches!(err, CliError::StdinIsTerminal));
    }

    #[test]
    fn stdin_terminal_is_refused_rather_than_read() {
        for text in [Some("-"), None] {
            let err = read_input(&request(text, None), FailingReader, true).unwrap_err();
            assert!(matches!(err, CliError::StdinIsTerminal));
            assert_eq!(err.exit_status(), ExitStatus::Usage);
        }
    }

    #[test]
    fn exactly_one_trailing_newline_is_stripped_unless_kept() {
        let cases: [(&[u8], &[u8]); 5] = [
            (b"a\n", b"a"),
            (b"a\r\n", b"a"),
            (b"a\n\n", b"a\n"),
            (b"a\r", b"a\r"),
            (b"a", b"a"),
        ];
        for (raw, expected) in cases {
            let input = read_input(&request(None, None), raw, false).unwrap();
            assert_eq!(input.data, expected, "{raw:?}");
            let kept = InputRequest {
                keep_newline: true,
                ..request(None, None)
            };
            assert_eq!(read_input(&kept, raw, false).unwrap().data, raw);
        }
    }

    #[test]
    fn empty_input_is_a_usage_error() {
        for raw in [&b""[..], b"\n", b"\r\n"] {
            let err = read_input(&request(None, None), raw, false).unwrap_err();
            assert!(matches!(
                err,
                CliError::EmptyInput {
                    origin: InputSource::Stdin
                }
            ));
        }
        let err = read_input(&request(Some(""), None), FailingReader, false).unwrap_err();
        assert_eq!(
            err.to_string(),
            "no data to encode: the TEXT argument is empty"
        );
    }

    #[test]
    fn reads_are_bounded() {
        let at_limit = vec![b'1'; MAX_READ_BYTES];
        let input = read_input(&request(None, None), at_limit.as_slice(), false).unwrap();
        assert_eq!(input.data.len(), MAX_READ_BYTES);

        // An endless reader must stop at the limit rather than hang.
        let endless = io::repeat(b'7');
        let err = read_input(&request(None, None), endless, false).unwrap_err();
        assert!(matches!(
            err,
            CliError::InputTooLong {
                max: MAX_PAYLOAD_BYTES,
                ..
            }
        ));
        assert_eq!(err.exit_status(), ExitStatus::DataTooLong);
    }

    #[test]
    fn read_failures_are_io_errors() {
        let err = read_input(&request(None, None), FailingReader, false).unwrap_err();
        assert_eq!(err.exit_status(), ExitStatus::Io);
        assert_eq!(err.to_string(), "cannot read stdin: device on fire");
    }

    #[test]
    fn files_are_read_raw_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.bin");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"\x00\xffbinary\n").unwrap();
        drop(f);
        let input = read_input(&request(None, Some(&path)), FailingReader, true).unwrap();
        assert_eq!(input.data, b"\x00\xffbinary");
        assert_eq!(input.source, InputSource::File(path.clone()));
        fs::remove_file(&path).unwrap();

        let err = read_input(&request(None, Some(&path)), FailingReader, true).unwrap_err();
        assert_eq!(err.exit_status(), ExitStatus::Io);
        assert!(err.to_string().starts_with("cannot read '"), "{err}");
    }

    #[test]
    fn sources_display_for_messages() {
        assert_eq!(InputSource::Argument.to_string(), "the TEXT argument");
        assert_eq!(InputSource::Stdin.to_string(), "stdin");
        assert_eq!(InputSource::File("a b.txt".into()).to_string(), "'a b.txt'");
        assert_eq!(InputSource::Payload("wifi").to_string(), "the wifi payload");
    }
}
