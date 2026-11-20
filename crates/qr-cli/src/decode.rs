//! `qr decode`: reading the QR codes in images and printing their data.
//!
//! Each `IMAGE` is read through `qr_core`'s bounded loader (regular files
//! only; `-` is stdin) and searched with [`qr_core::decode`]. An image
//! that cannot be read (including one refused for holding too many
//! finder-like patterns, DECISIONS section 4.3), or holds no readable
//! code, is reported on stderr and the remaining images are still read;
//! the exit status then says what went wrong (DECISIONS section 2): 4 if
//! any image could not be read, otherwise 5 if any held no code,
//! otherwise 0.
//!
//! Decoded data is untrusted, so it reaches stdout in one of three ways
//! (DECISIONS section 4.5):
//! - by default, escaped with `sanitize_for_terminal`, with bytes that are
//!   not UTF-8 written as `\x{..}`, one line per code;
//! - with `--raw`, unescaped, one code per line, which is refused when
//!   stdout is a terminal;
//! - with `--json`, in one JSON object whose strings escape every control
//!   character, with base64 for data that is not UTF-8.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::Path;

use qr_core::decode::{DecodedCode, Scan, decode_bitmap};
use qr_core::loader::{LoadError, load_image};
use qr_core::render::base64;
use qr_core::sanitize_for_terminal;

use crate::cli::DecodeArgs;
use crate::diag::Reporter;
use crate::env::Env;
use crate::error::CliError;
use crate::exit::ExitStatus;
use crate::input::{InputSource, open_regular_file};
use crate::output::json::{
    self, DecodeReport, DecodeSummary, DecodedImage, DecodedPayload, ImageError,
};

/// Streams and settings a decode run needs.
pub(crate) struct Decode<'a, 'r> {
    pub(crate) args: &'a DecodeArgs,
    pub(crate) env: &'a Env,
    pub(crate) stdin: &'a mut dyn Read,
    pub(crate) stdout: &'a mut dyn Write,
    pub(crate) reporter: &'a mut Reporter<'r>,
}

impl Decode<'_, '_> {
    /// Runs the command and returns the exit status that sums up every
    /// image.
    ///
    /// # Errors
    ///
    /// A usage error found before any image is read ([`check`]), or
    /// [`CliError::Output`] if stdout cannot be written. Problems with
    /// single images are reported and reflected in the status instead.
    pub(crate) fn run(self) -> Result<ExitStatus, CliError> {
        let Self {
            args,
            env,
            stdin,
            stdout,
            reporter,
        } = self;
        check(args, env)?;

        let mode = if args.json {
            Mode::Json
        } else if args.raw {
            Mode::Raw
        } else {
            Mode::Text {
                prefix: args.images.len() > 1,
            }
        };
        let mut report = DecodeReport::default();
        for path in &args.images {
            let origin = origin(path);
            let entry = match read(path, &origin, &mut *stdin) {
                Ok(scan) if !scan.is_empty() => {
                    for code in &scan.codes {
                        reporter.note("decoded", &describe(&origin, code));
                    }
                    mode.write(path, &scan, &mut *stdout)
                        .map_err(CliError::Output)?;
                    report.summary.codes += scan.codes.len();
                    image_entry(path, &scan.codes, None)
                }
                Ok(scan) => {
                    report.summary.no_code += 1;
                    let err = CliError::NoCodeFound {
                        origin,
                        undecodable: scan.undecodable,
                    };
                    failed(path, &err, reporter)
                }
                Err(err) => {
                    report.summary.unreadable += 1;
                    failed(path, &err, reporter)
                }
            };
            report.images.push(entry);
        }
        report.summary.images = args.images.len();

        if args.json {
            stdout
                .write_all(json::object(&report).as_bytes())
                .map_err(CliError::Output)?;
        }
        let summary = report.summary;
        if summary.images > 1 && summary.unreadable + summary.no_code > 0 {
            reporter.error("decode_incomplete", &summarise(&summary), None);
        }
        Ok(status(&summary))
    }
}

/// Usage errors found before anything is read: `--raw` to a terminal,
/// `-` twice, or `-` while stdin is a terminal.
fn check(args: &DecodeArgs, env: &Env) -> Result<(), CliError> {
    if args.raw && env.stdout_is_tty {
        return Err(CliError::RawToTerminal);
    }
    let dashes = args.images.iter().filter(|p| is_stdin(p)).count();
    if dashes > 1 {
        return Err(CliError::StdinTwice);
    }
    if dashes == 1 && env.stdin_is_tty {
        return Err(CliError::ImageStdinIsTerminal);
    }
    Ok(())
}

fn is_stdin(path: &Path) -> bool {
    path == Path::new("-")
}

fn origin(path: &Path) -> InputSource {
    if is_stdin(path) {
        InputSource::Stdin
    } else {
        InputSource::File(path.to_path_buf())
    }
}

/// Loads one image within the loader's limits and reads every code in it.
fn read(path: &Path, origin: &InputSource, stdin: &mut dyn Read) -> Result<Scan, CliError> {
    let loaded = if is_stdin(path) {
        load_image(stdin, None)
    } else {
        let (file, len) = open_regular_file(path).map_err(|error| CliError::ReadImage {
            origin: origin.clone(),
            error,
        })?;
        load_image(file, Some(len))
    };
    let bitmap = loaded.map_err(|error| match error {
        LoadError::Read(error) => CliError::ReadImage {
            origin: origin.clone(),
            error,
        },
        error => CliError::Image {
            origin: origin.clone(),
            error,
        },
    })?;
    decode_bitmap(bitmap).map_err(|error| CliError::Search {
        origin: origin.clone(),
        error,
    })
}

/// Reports `err` for one image and returns its JSON entry.
fn failed(path: &Path, err: &CliError, reporter: &mut Reporter<'_>) -> DecodedImage {
    let message = err.to_string();
    reporter.error(err.code(), &message, err.hint().as_deref());
    image_entry(
        path,
        &[],
        Some(ImageError {
            code: err.code(),
            message,
        }),
    )
}

/// How decoded data is written to stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Escaped text, one line per code, prefixed with the path when there
    /// are several images.
    Text {
        /// Whether to start each line with the image's path.
        prefix: bool,
    },
    /// The data unescaped, one code per line (each followed by `\n`).
    Raw,
    /// Nothing until the end, when the JSON report is written.
    Json,
}

impl Mode {
    /// Writes the codes `scan` found in the image at `path`.
    fn write(self, path: &Path, scan: &Scan, out: &mut dyn Write) -> std::io::Result<()> {
        for code in &scan.codes {
            match self {
                Self::Text { prefix } => {
                    let mut line = String::new();
                    if prefix {
                        line.push_str(&sanitize_for_terminal(&path.to_string_lossy()));
                        line.push_str(": ");
                    }
                    line.push_str(&escape_payload(&code.payload));
                    line.push('\n');
                    out.write_all(line.as_bytes())?;
                }
                Self::Raw => {
                    out.write_all(&code.payload)?;
                    out.write_all(b"\n")?;
                }
                Self::Json => {}
            }
        }
        Ok(())
    }
}

/// `payload` as one line of terminal-safe text: valid UTF-8 through
/// [`sanitize_for_terminal`], every other byte as `\x{..}`.
fn escape_payload(payload: &[u8]) -> String {
    let mut out = String::with_capacity(payload.len());
    for chunk in payload.utf8_chunks() {
        out.push_str(&sanitize_for_terminal(chunk.valid()));
        for byte in chunk.invalid() {
            // Writing to a String cannot fail.
            let _ = write!(out, "\\x{{{byte:02x}}}");
        }
    }
    out
}

/// The `--json` entry for one image.
fn image_entry(path: &Path, codes: &[DecodedCode], error: Option<ImageError>) -> DecodedImage {
    DecodedImage {
        path: path.to_string_lossy().into_owned(),
        payloads: codes.iter().map(payload_entry).collect(),
        error,
    }
}

fn payload_entry(code: &DecodedCode) -> DecodedPayload {
    let (text, base64) = match std::str::from_utf8(&code.payload) {
        Ok(text) => (text.to_owned(), None),
        Err(_) => (
            String::from_utf8_lossy(&code.payload).into_owned(),
            Some(base64(&code.payload)),
        ),
    };
    DecodedPayload {
        text,
        base64,
        bytes: code.payload.len(),
        version: code.version.number(),
        ec: code.ec_level.to_string(),
        mask: code.mask.number(),
        inverted: code.inverted,
        bounds: code.bounds.map(|p| [p.x, p.y]),
    }
}

/// The `-v` note for one decoded code.
fn describe(origin: &InputSource, code: &DecodedCode) -> String {
    let [top_left, _, bottom_right, _] = code.bounds;
    format!(
        "{origin}: version {}, error correction {}, mask {}, {} bytes{}, at ({}, {})-({}, {})",
        code.version,
        code.ec_level,
        code.mask,
        code.payload.len(),
        if code.inverted { ", light on dark" } else { "" },
        top_left.x,
        top_left.y,
        bottom_right.x,
        bottom_right.y,
    )
}

/// The closing error line when some of several images failed.
fn summarise(summary: &DecodeSummary) -> String {
    let failed = summary.unreadable + summary.no_code;
    let mut parts = Vec::new();
    if summary.unreadable > 0 {
        parts.push(format!("{} unreadable", summary.unreadable));
    }
    if summary.no_code > 0 {
        parts.push(format!("{} without a readable code", summary.no_code));
    }
    format!(
        "{failed} of {} images failed ({}); {} {} decoded",
        summary.images,
        parts.join(", "),
        summary.codes,
        if summary.codes == 1 { "code" } else { "codes" },
    )
}

/// The exit status for the whole run: an unreadable image (4) outranks one
/// without a code (5).
const fn status(summary: &DecodeSummary) -> ExitStatus {
    if summary.unreadable > 0 {
        ExitStatus::Io
    } else if summary.no_code > 0 {
        ExitStatus::NoCodeFound
    } else {
        ExitStatus::Success
    }
}

#[cfg(test)]
mod tests;
