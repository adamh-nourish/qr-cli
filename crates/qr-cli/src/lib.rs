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
//! - [`output`]: output format resolution, atomic file writes, `data:`
//!   URIs and the `--json` objects;
//! - `style` and `verify`: colour and shape options, contrast warnings and
//!   the `--verify` read-back check;
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
pub mod output;
mod style;
mod tooling;
mod verify;

use std::ffi::OsString;
use std::io::{Read, Write};

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{CommandFactory as _, Parser as _};
use qr_core::{sanitize_for_terminal, sanitize_lines_for_terminal};

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
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(err) => return report_parse_error(&err, wants_json(&args), ctx),
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
    )
    .with_json(cli.generate.json);

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
            reporter.error(err.code(), &err.to_string(), err.hint().as_deref());
            err.exit_status()
        }
    }
}

/// Whether `--json` appears among the options, so that a command line clap
/// rejects still gets its error as JSON. Arguments after `--` are data.
fn wants_json(args: &[OsString]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|a| a.as_os_str() != "--")
        .any(|a| a.as_os_str() == "--json")
}

/// Prints a clap error (or `--help`/`--version` output) and picks the status.
fn report_parse_error(err: &clap::Error, json: bool, ctx: &mut Context<'_>) -> ExitStatus {
    if json && err.use_stderr() {
        let extra = extra_argument(err);
        let message = extra.as_ref().map_or_else(
            || {
                first_error_line(&err.render().to_string())
                    .unwrap_or_else(|| "invalid command line".to_owned())
            },
            |word| format!("unexpected extra argument '{word}'"),
        );
        let hint = extra
            .is_some()
            .then_some("quote text that contains spaces: qr 'hello world'");
        Reporter::new(&mut *ctx.stderr, false, Verbosity::Normal)
            .with_json(true)
            .error("usage", &message, hint);
        return ExitStatus::Usage;
    }
    if let Some(extra) = extra_argument(err) {
        // Best effort, as below. The argument is user data.
        let _ = write!(
            ctx.stderr,
            "error: unexpected extra argument '{}'\n  \
             hint: quote text that contains spaces: qr 'hello world'\n",
            sanitize_for_terminal(&extra)
        );
        return ExitStatus::Usage;
    }
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

/// The text of clap's first `error:` line, without the label.
fn first_error_line(rendered: &str) -> Option<String> {
    rendered
        .lines()
        .find_map(|line| line.strip_prefix("error: "))
        .map(str::to_owned)
}

/// The stray word in `qr hello world`, if that is what `err` is about.
///
/// With `args_conflicts_with_subcommands`, clap reads a second positional
/// word as a subcommand and reports "the subcommand 'world' cannot be used
/// with '[TEXT]'", even when no such subcommand exists. That is almost
/// always unquoted text, so it gets a clearer message. A real subcommand
/// name (`qr hello man`) keeps clap's own error.
fn extra_argument(err: &clap::Error) -> Option<String> {
    if err.kind() != ErrorKind::ArgumentConflict {
        return None;
    }
    let Some(ContextValue::String(word)) = err.get(ContextKind::InvalidSubcommand) else {
        return None;
    };
    let mut command = Cli::command();
    command.build();
    let is_subcommand = command
        .get_subcommands()
        .any(|sub| sub.get_name() == word || sub.get_all_aliases().any(|alias| alias == word));
    (!is_subcommand).then(|| word.clone())
}

#[cfg(test)]
mod tests;
