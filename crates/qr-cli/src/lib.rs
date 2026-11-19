//! Library half of the `qr` command-line tool.
//!
//! The binary is a thin wrapper around [`run`], which takes the arguments and
//! output streams explicitly so that the whole CLI can be tested in-process.
//! Argument parsing lives in [`cli`], process exit statuses in [`exit`] and
//! user-facing errors in [`error`].

#![forbid(unsafe_code)]

pub mod cli;
pub mod error;
pub mod exit;

use std::ffi::OsString;
use std::io::Write;

use clap::Parser as _;

pub use crate::cli::Cli;
pub use crate::error::CliError;
pub use crate::exit::ExitStatus;

/// Runs the CLI with `args` (including the program name as the first item),
/// writing normal output to `stdout` and diagnostics to `stderr`.
///
/// Errors are reported on `stderr` and converted into the matching
/// [`ExitStatus`]; this function never panics on bad input.
pub fn run<I, T, O, E>(args: I, stdout: &mut O, stderr: &mut E) -> ExitStatus
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
    O: Write + ?Sized,
    E: Write + ?Sized,
{
    match try_run(args, stdout, stderr) {
        Ok(status) => status,
        Err(err) => {
            // If stderr itself is unwritable there is nowhere left to report.
            let _ = writeln!(stderr, "error: {err}");
            err.exit_status()
        }
    }
}

fn try_run<I, T, O, E>(args: I, stdout: &mut O, stderr: &mut E) -> Result<ExitStatus, CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
    O: Write + ?Sized,
    E: Write + ?Sized,
{
    let status = match Cli::try_parse_from(args) {
        Ok(_cli) => ExitStatus::Success,
        Err(err) => {
            let rendered = err.render().to_string();
            if err.use_stderr() {
                // Best effort: a usage error is still a usage error even if
                // it cannot be printed.
                let _ = stderr.write_all(rendered.as_bytes());
                ExitStatus::Usage
            } else {
                stdout
                    .write_all(rendered.as_bytes())
                    .map_err(CliError::Output)?;
                ExitStatus::Success
            }
        }
    };
    stdout.flush().map_err(CliError::Output)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{ExitStatus, run};

    /// A writer that fails every operation, standing in for a closed pipe.
    struct BrokenPipe;

    impl Write for BrokenPipe {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
        }
    }

    fn run_capture(args: &[&str]) -> (ExitStatus, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run(args.iter().copied(), &mut out, &mut err);
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn version_flags_print_name_and_version() {
        let expected = format!("qr {}\n", env!("CARGO_PKG_VERSION"));
        for flag in ["--version", "-V"] {
            let (status, out, err) = run_capture(&["qr", flag]);
            assert_eq!(status, ExitStatus::Success);
            assert_eq!(out, expected);
            assert_eq!(err, "");
        }
    }

    #[test]
    fn help_goes_to_stdout_and_succeeds() {
        let (status, out, err) = run_capture(&["qr", "--help"]);
        assert_eq!(status, ExitStatus::Success);
        assert!(out.contains("Usage: qr"), "{out}");
        assert_eq!(err, "");
    }

    #[test]
    fn no_arguments_is_a_usage_error_with_help() {
        let (status, out, err) = run_capture(&["qr"]);
        assert_eq!(status, ExitStatus::Usage);
        assert_eq!(out, "");
        assert!(err.contains("Usage: qr"), "{err}");
    }

    #[test]
    fn unknown_flag_is_a_usage_error() {
        let (status, out, err) = run_capture(&["qr", "--definitely-not-a-flag"]);
        assert_eq!(status, ExitStatus::Usage);
        assert_eq!(out, "");
        assert!(err.contains("--definitely-not-a-flag"), "{err}");
    }

    #[test]
    fn unwritable_stdout_is_an_io_error() {
        let mut err = Vec::new();
        let status = run(["qr", "--version"], &mut BrokenPipe, &mut err);
        assert_eq!(status, ExitStatus::Io);
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err, "error: cannot write output: pipe closed\n");
    }

    #[test]
    fn unwritable_stderr_still_reports_usage_status() {
        let mut out = Vec::new();
        let status = run(["qr", "--nope"], &mut out, &mut BrokenPipe);
        assert_eq!(status, ExitStatus::Usage);
        assert_eq!(out, b"");
    }

    #[test]
    fn core_and_cli_versions_move_in_lockstep() {
        assert_eq!(qr_core::VERSION, env!("CARGO_PKG_VERSION"));
    }
}
