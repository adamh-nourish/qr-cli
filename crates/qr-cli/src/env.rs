//! What the process knows about its surroundings: which streams are
//! terminals, how wide the terminal is and the colour environment variables.
//!
//! [`Env::detect`] reads the real process state once at startup. Tests build
//! an [`Env`] by hand, so every terminal-dependent branch can be exercised
//! without a terminal.

use std::ffi::OsStr;
use std::io::{self, IsTerminal as _};

/// The process environment that affects output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // Each flag is an independent fact.
pub struct Env {
    /// Whether stdin is a terminal (and so must not be read implicitly).
    pub stdin_is_tty: bool,
    /// Whether stdout is a terminal.
    pub stdout_is_tty: bool,
    /// Whether stderr is a terminal.
    pub stderr_is_tty: bool,
    /// Width of the terminal on stdout in columns, if known.
    pub terminal_width: Option<usize>,
    /// `NO_COLOR` is set to a non-empty value.
    pub no_color: bool,
    /// `CLICOLOR_FORCE` is set to a non-empty value other than `0`.
    pub clicolor_force: bool,
}

impl Env {
    /// Reads the environment of the current process.
    pub fn detect() -> Self {
        let stdout_is_tty = io::stdout().is_terminal();
        Self {
            stdin_is_tty: io::stdin().is_terminal(),
            stdout_is_tty,
            stderr_is_tty: io::stderr().is_terminal(),
            terminal_width: if stdout_is_tty {
                terminal_size::terminal_size().map(|(w, _)| usize::from(w.0))
            } else {
                None
            },
            no_color: no_color(std::env::var_os("NO_COLOR").as_deref()),
            clicolor_force: clicolor_force(std::env::var_os("CLICOLOR_FORCE").as_deref()),
        }
    }
}

/// `NO_COLOR` disables colour when it is present and not empty
/// (<https://no-color.org>).
fn no_color(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// `CLICOLOR_FORCE` forces colour when it is present, not empty and not `0`
/// (<https://bixense.com/clicolors/>).
fn clicolor_force(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn no_color_needs_a_non_empty_value() {
        assert!(!no_color(None));
        assert!(!no_color(Some(OsStr::new(""))));
        assert!(no_color(Some(OsStr::new("1"))));
        assert!(no_color(Some(OsStr::new("0"))));
    }

    #[test]
    fn clicolor_force_ignores_empty_and_zero() {
        assert!(!clicolor_force(None));
        assert!(!clicolor_force(Some(OsStr::new(""))));
        assert!(!clicolor_force(Some(OsStr::new("0"))));
        assert!(clicolor_force(Some(OsStr::new("1"))));
    }

    #[test]
    fn detect_reads_the_process_without_failing() {
        // Under a test runner the streams are usually pipes; either way
        // the width is only probed for a terminal.
        let env = Env::detect();
        if !env.stdout_is_tty {
            assert_eq!(env.terminal_width, None);
        }
    }
}
