//! Where a rendered code goes and in what format.
//!
//! [`Plan::resolve`] runs before any input is read, so every usage error
//! about the output (an unknown extension, binary data for a terminal, a
//! `--no-clobber` path that exists) is reported without consuming stdin or
//! encoding anything. [`Plan::deliver`] then writes the rendered bytes.
//!
//! Format resolution follows DECISIONS section 2: `-f` first, then the `-o`
//! extension, then `terminal` when stdout is a terminal, otherwise `text`.
//! `--data-uri` with nothing else to go on picks `svg`.

mod data_uri;
mod format;
pub mod json;
mod write;

use std::io::Write;
use std::path::{Path, PathBuf};

pub use self::data_uri::{base64, data_uri};
pub use self::format::{Renderer, known_extensions};
pub use self::write::{WriteError, precheck, write_atomic};
use crate::cli::{GenerateArgs, OutputFormat};
use crate::diag::Reporter;
use crate::error::CliError;

/// The format `--data-uri` uses when neither `-f` nor `-o` names one.
pub const DATA_URI_DEFAULT_FORMAT: OutputFormat = OutputFormat::Svg;

/// Where the output goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Standard output (no `-o`, or `-o -`).
    Stdout,
    /// A file, written atomically.
    File(PathBuf),
}

impl Target {
    /// The target selected by `-o`.
    pub fn from_arg(output: Option<&Path>) -> Self {
        match output {
            Some(path) if path.as_os_str() != "-" => Self::File(path.to_owned()),
            Some(_) | None => Self::Stdout,
        }
    }
}

/// The resolved output settings for one code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Where the output goes.
    pub target: Target,
    /// The format to render.
    pub format: OutputFormat,
    /// Wrap the rendered bytes in a `data:` URI.
    pub data_uri: bool,
    /// Print a JSON result object on stdout.
    pub json: bool,
    /// Refuse to replace an existing file.
    pub no_clobber: bool,
}

/// What [`Plan::deliver`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    /// The number of bytes written (or held for the JSON object).
    pub written: usize,
    /// The `data:` URI, when it is to be reported in the JSON object
    /// instead of being printed.
    pub data_uri: Option<String>,
}

impl Plan {
    /// Resolves the output flags, given whether stdout is a terminal.
    ///
    /// # Errors
    ///
    /// A usage error for an unknown `-o` extension, a format without a
    /// renderer, `--data-uri` with a text format, `--json` sharing stdout
    /// with the code, or binary output to a terminal; an I/O error when the
    /// `-o` path is a directory or exists under `--no-clobber`.
    pub fn resolve(
        args: &GenerateArgs,
        stdout_is_tty: bool,
        reporter: &mut Reporter<'_>,
    ) -> Result<Self, CliError> {
        let target = Target::from_arg(args.output.as_deref());
        let format = match (args.format, &target) {
            (Some(format), _) => format,
            (None, Target::File(path)) => OutputFormat::from_path(path)
                .ok_or_else(|| CliError::UnknownExtension(path.clone()))?,
            (None, Target::Stdout) if args.data_uri => DATA_URI_DEFAULT_FORMAT,
            (None, Target::Stdout) if stdout_is_tty => OutputFormat::Terminal,
            (None, Target::Stdout) => OutputFormat::Text,
        };

        if format.renderer() == Renderer::Raster {
            return Err(CliError::UnsupportedFormat(format));
        }
        if args.data_uri && !format.is_image() {
            return Err(CliError::DataUriNeedsImage(format));
        }
        match &target {
            Target::Stdout => {
                if args.json && !args.data_uri {
                    return Err(CliError::JsonNeedsOutput(format));
                }
                if stdout_is_tty && format.is_binary() && !args.data_uri {
                    return Err(CliError::BinaryToTerminal(format));
                }
            }
            Target::File(path) => {
                precheck(path, args.no_clobber)?;
                if args.format.is_some() && !args.data_uri {
                    warn_on_extension_mismatch(path, format, reporter);
                }
            }
        }

        Ok(Self {
            target,
            format,
            data_uri: args.data_uri,
            json: args.json,
            no_clobber: args.no_clobber,
        })
    }

    /// Whether the code is drawn on stdout, so terminal checks such as the
    /// width warning apply.
    pub fn is_stdout(&self) -> bool {
        self.target == Target::Stdout
    }

    /// Writes `rendered` to the target, wrapped in a `data:` URI if asked.
    ///
    /// With `--json` and stdout as the target, nothing is printed here: the
    /// URI is returned for the JSON object instead.
    ///
    /// # Errors
    ///
    /// An I/O error if writing fails; a file target is then left unchanged.
    pub fn deliver(
        &self,
        rendered: Vec<u8>,
        stdout: &mut dyn Write,
    ) -> Result<Delivered, CliError> {
        let bytes = match self.format.media_type() {
            Some(media_type) if self.data_uri => {
                let mut uri = data_uri(media_type, &rendered);
                if self.json && self.target == Target::Stdout {
                    return Ok(Delivered {
                        written: uri.len(),
                        data_uri: Some(uri),
                    });
                }
                uri.push('\n');
                uri.into_bytes()
            }
            Some(_) | None => rendered,
        };
        match &self.target {
            Target::Stdout => stdout.write_all(&bytes).map_err(CliError::Output)?,
            Target::File(path) => write_atomic(path, &bytes, self.no_clobber)?,
        }
        Ok(Delivered {
            written: bytes.len(),
            data_uri: None,
        })
    }
}

/// Warns when `-f` and the `-o` extension disagree, as in `-f svg -o a.pdf`.
/// `-f` wins, but the file will likely be opened with the wrong program.
fn warn_on_extension_mismatch(path: &Path, format: OutputFormat, reporter: &mut Reporter<'_>) {
    let Some(implied) = OutputFormat::from_path(path) else {
        return;
    };
    let same =
        implied == format || (implied == OutputFormat::Text && format.renderer() == Renderer::Text);
    if !same {
        reporter.warn(
            &format!(
                "writing {} to '{}', whose extension suggests {}",
                format.name(),
                path.display(),
                implied.name()
            ),
            Some("drop -f to use the format the extension names"),
        );
    }
}

#[cfg(test)]
mod tests;
