//! Library half of the `qr` command-line tool.
//!
//! The binary is a thin wrapper around [`run`], which takes the arguments,
//! the standard streams and the detected [`Env`] explicitly, so the whole
//! CLI (terminal detection and colour rules included) can be tested
//! in-process.
//!
//! - [`cli`]: the clap definition, the single source for help, completions
//!   and the man page;
//! - [`input`]: input resolution and bounded reads;
//! - [`diag`]: errors, warnings and notes on stderr;
//! - [`env`](mod@env): terminal and environment detection;
//! - [`error`] and [`exit`]: typed errors and the exit status contract.

#![forbid(unsafe_code)]

pub mod cli;
pub mod diag;
pub mod env;
pub mod error;
pub mod exit;
mod generate;
pub mod input;
mod tooling;

use std::ffi::OsString;
use std::io::{Read, Write};

use clap::Parser as _;
use qr_core::sanitize_lines_for_terminal;

pub use crate::cli::Cli;
use crate::cli::Command;
use crate::diag::{Reporter, Verbosity};
pub use crate::env::Env;
pub use crate::error::CliError;
pub use crate::exit::ExitStatus;
use crate::generate::Generate;

/// The streams and environment one invocation runs against.
pub struct Context<'a> {
    /// Where input is read from when stdin is selected.
    pub stdin: &'a mut dyn Read,
    /// Where codes and other primary output go.
    pub stdout: &'a mut dyn Write,
    /// Where diagnostics go.
    pub stderr: &'a mut dyn Write,
    /// Terminal and environment facts.
    pub env: Env,
}

/// Runs the CLI with `args` (including the program name as the first item).
///
/// Errors are reported on stderr and converted into the matching
/// [`ExitStatus`]; this function never panics on bad input.
pub fn run<I, T>(args: I, ctx: &mut Context<'_>) -> ExitStatus
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => return report_parse_error(&err, ctx),
    };

    let Context {
        stdin,
        stdout,
        stderr,
        env,
    } = ctx;
    let global = cli.global;
    let mut reporter = Reporter::new(
        &mut **stderr,
        global.color.enabled(env.stderr_is_tty, env),
        Verbosity::from_flags(global.quiet, global.verbose),
    );

    let result = match cli.command {
        Some(Command::Completions { shell }) => tooling::completions(shell, &mut **stdout),
        Some(Command::Man) => tooling::man(&mut **stdout),
        None => Generate {
            args: &cli.generate,
            color: global.color,
            env,
            stdin: &mut **stdin,
            stdout: &mut **stdout,
            reporter: &mut reporter,
        }
        .run(),
    }
    .and_then(|()| stdout.flush().map_err(CliError::Output));

    match result {
        Ok(()) => ExitStatus::Success,
        Err(err) => {
            reporter.error(&err.to_string(), err.hint().as_deref());
            err.exit_status()
        }
    }
}

/// Prints a clap error (or `--help`/`--version` output) and picks the status.
fn report_parse_error(err: &clap::Error, ctx: &mut Context<'_>) -> ExitStatus {
    let rendered = err.render().to_string();
    if err.use_stderr() {
        // Clap quotes the offending argument, which is user data.
        // Best effort: a usage error is still a usage error even if it
        // cannot be printed.
        let _ = ctx
            .stderr
            .write_all(sanitize_lines_for_terminal(&rendered).as_bytes());
        ExitStatus::Usage
    } else {
        let written = ctx
            .stdout
            .write_all(rendered.as_bytes())
            .and_then(|()| ctx.stdout.flush());
        match written {
            Ok(()) => ExitStatus::Success,
            Err(e) => {
                let err = CliError::Output(e);
                let _ = writeln!(ctx.stderr, "error: {err}");
                err.exit_status()
            }
        }
    }
}

#[cfg(test)]
mod tests;
