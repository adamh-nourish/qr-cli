//! `qr completions` and `qr man`.
//!
//! Both are generated from the clap definition in [`crate::cli`], so they
//! can never drift from the real argument parser. Output is built in memory
//! first: `clap_complete` panics on write errors, and a closed pipe must be
//! an ordinary I/O error (exit 4) instead.

use std::io::Write;

use clap::CommandFactory as _;
use clap_complete::Shell;

use crate::cli::Cli;
use crate::error::CliError;

/// Writes the completion script for `shell` to `out`.
///
/// # Errors
///
/// [`CliError::Output`] if writing fails.
pub(crate) fn completions(shell: Shell, out: &mut dyn Write) -> Result<(), CliError> {
    let mut command = Cli::command();
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut command, "qr", &mut script);
    out.write_all(&script).map_err(CliError::Output)
}

/// Writes the man page, in roff, to `out`.
///
/// # Errors
///
/// [`CliError::Output`] if writing fails.
pub(crate) fn man(out: &mut dyn Write) -> Result<(), CliError> {
    let mut page = Vec::new();
    clap_mangen::Man::new(Cli::command())
        .render(&mut page)
        .map_err(|e| CliError::Internal(format!("cannot render the man page: {e}")))?;
    out.write_all(&page).map_err(CliError::Output)
}
