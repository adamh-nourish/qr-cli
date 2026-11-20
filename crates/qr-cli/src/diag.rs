//! Diagnostics on stderr: errors, warnings, hints and verbose notes.
//!
//! Every message is passed through [`qr_core::sanitize_for_terminal`]
//! before it is written, because messages can quote user data such as file
//! names. Colour is decided once, from `--color`, the environment and
//! whether stderr is a terminal.
//!
//! With `--json`, each diagnostic is instead one line of JSON keyed by its
//! kind, such as `{"warning":{"code":"...","message":"...","hint":"..."}}`.
//! Every error, warning and note has a stable `code`. JSON text is not
//! sanitised but escaped by [`json::object`], which writes every control
//! and bidi character as `\uXXXX`: nothing reaches the terminal raw, and a
//! consumer gets back the exact text, such as a file name.

use std::io::Write;

use anstyle::{AnsiColor, Style};
use qr_core::sanitize_for_terminal;

use crate::cli::ColorChoice;
use crate::env::Env;
use crate::output::json::{self, Diagnostic};

impl ColorChoice {
    /// Whether to colour a stream, given whether it is a terminal.
    ///
    /// In `auto`, `NO_COLOR` turns colour off and takes precedence over
    /// `CLICOLOR_FORCE`, which turns it on for non-terminals. An explicit
    /// `--color always` or `--color never` overrides both.
    pub fn enabled(self, is_tty: bool, env: &Env) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => !env.no_color && (env.clicolor_force || is_tty),
        }
    }
}

/// How much to say on stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Verbosity {
    /// Errors only (`-q`).
    Quiet,
    /// Errors and warnings.
    #[default]
    Normal,
    /// Errors, warnings and notes (`-v`).
    Verbose,
}

impl Verbosity {
    /// The verbosity selected by the `-q` and `-v` flags.
    pub const fn from_flags(quiet: bool, verbose: bool) -> Self {
        match (quiet, verbose) {
            (true, _) => Self::Quiet,
            (false, true) => Self::Verbose,
            (false, false) => Self::Normal,
        }
    }
}

const ERROR: Style = Style::new()
    .bold()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
const WARNING: Style = Style::new()
    .bold()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
const HINT: Style = Style::new()
    .bold()
    .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));
const NOTE: Style = Style::new().bold();

/// How a [`Reporter`] writes: colour, verbosity and JSON. It is `Copy` and
/// `Send`, so worker threads can each build a reporter over their own
/// buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReporterSettings {
    color: bool,
    verbosity: Verbosity,
    json: bool,
}

impl ReporterSettings {
    /// A reporter with these settings writing to `sink`.
    pub fn reporter(self, sink: &mut dyn Write) -> Reporter<'_> {
        Reporter::new(sink, self.color, self.verbosity).with_json(self.json)
    }
}

/// Writes diagnostics to stderr.
///
/// Writing is best effort: if stderr itself is broken there is nowhere left
/// to report that, so write errors are ignored.
pub struct Reporter<'a> {
    stderr: &'a mut dyn Write,
    color: bool,
    verbosity: Verbosity,
    json: bool,
}

impl<'a> Reporter<'a> {
    /// Creates a reporter that writes to `stderr`.
    pub fn new(stderr: &'a mut dyn Write, color: bool, verbosity: Verbosity) -> Self {
        Self {
            stderr,
            color,
            verbosity,
            json: false,
        }
    }

    /// Returns this reporter writing JSON lines (for `--json`) or text.
    #[must_use]
    pub fn with_json(mut self, json: bool) -> Self {
        self.json = json;
        self
    }

    /// The settings this reporter writes with, to build another reporter
    /// that writes the same way (for example into a buffer on a worker
    /// thread, forwarded later with [`forward`](Self::forward)).
    pub fn settings(&self) -> ReporterSettings {
        ReporterSettings {
            color: self.color,
            verbosity: self.verbosity,
            json: self.json,
        }
    }

    /// Writes diagnostics another reporter with the same
    /// [`settings`](Self::settings) produced, unchanged.
    pub fn forward(&mut self, diagnostics: &[u8]) {
        let _ = self.stderr.write_all(diagnostics);
    }

    /// Reports an error, always. `code` is the stable name `--json` shows.
    pub fn error(&mut self, code: &str, message: &str, hint: Option<&str>) {
        self.emit(ERROR, "error", code, message, hint);
    }

    /// Reports a warning unless `-q` was given. `code` is the stable name
    /// `--json` shows.
    pub fn warn(&mut self, code: &str, message: &str, hint: Option<&str>) {
        if self.verbosity != Verbosity::Quiet {
            self.emit(WARNING, "warning", code, message, hint);
        }
    }

    /// Reports a detail only when `-v` was given. `code` is the stable name
    /// `--json` shows.
    pub fn note(&mut self, code: &str, message: &str) {
        if self.verbosity == Verbosity::Verbose {
            self.emit(NOTE, "note", code, message, None);
        }
    }

    fn emit(&mut self, style: Style, label: &str, code: &str, message: &str, hint: Option<&str>) {
        if self.json {
            // `json::keyed` escapes controls and bidi characters losslessly.
            let line = json::keyed(
                label,
                &Diagnostic {
                    code,
                    message,
                    hint,
                },
            );
            let _ = self.stderr.write_all(line.as_bytes());
            return;
        }
        let message = sanitize_for_terminal(message);
        let hint = hint.map(sanitize_for_terminal);
        let _ = writeln!(self.stderr, "{}: {message}", self.paint(style, label));
        if let Some(hint) = hint {
            let _ = writeln!(self.stderr, "  {}: {hint}", self.paint(HINT, "hint"));
        }
    }

    fn paint(&self, style: Style, text: &str) -> String {
        if self.color {
            format!("{}{text}{}", style.render(), style.render_reset())
        } else {
            text.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(no_color: bool, clicolor_force: bool) -> Env {
        Env {
            no_color,
            clicolor_force,
            ..Env::default()
        }
    }

    #[test]
    fn color_choice_resolution_table() {
        // (choice, is_tty, NO_COLOR, CLICOLOR_FORCE) -> enabled
        let table = [
            (ColorChoice::Always, false, true, false, true),
            (ColorChoice::Never, true, false, true, false),
            (ColorChoice::Auto, true, false, false, true),
            (ColorChoice::Auto, false, false, false, false),
            (ColorChoice::Auto, true, true, false, false),
            (ColorChoice::Auto, false, false, true, true),
            (ColorChoice::Auto, true, true, true, false),
        ];
        for (choice, tty, nc, force, expected) in table {
            assert_eq!(
                choice.enabled(tty, &env(nc, force)),
                expected,
                "{choice:?} tty={tty} NO_COLOR={nc} CLICOLOR_FORCE={force}"
            );
        }
    }

    #[test]
    fn verbosity_from_flags() {
        assert_eq!(Verbosity::from_flags(false, false), Verbosity::Normal);
        assert_eq!(Verbosity::from_flags(true, false), Verbosity::Quiet);
        assert_eq!(Verbosity::from_flags(false, true), Verbosity::Verbose);
        assert_eq!(Verbosity::from_flags(true, true), Verbosity::Quiet);
    }

    fn report(color: bool, verbosity: Verbosity) -> String {
        let mut buf = Vec::new();
        let mut r = Reporter::new(&mut buf, color, verbosity);
        r.error("usage", "bad \x1b[2J thing", Some("try \u{202e}this"));
        r.warn("careful_now", "careful", None);
        r.note("detail_note", "detail");
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn plain_output_is_sanitized_and_filtered_by_verbosity() {
        assert_eq!(
            report(false, Verbosity::Normal),
            "error: bad \\u{1b}[2J thing\n  hint: try \\u{202e}this\nwarning: careful\n"
        );
        assert_eq!(
            report(false, Verbosity::Quiet),
            "error: bad \\u{1b}[2J thing\n  hint: try \\u{202e}this\n"
        );
        assert!(report(false, Verbosity::Verbose).ends_with("warning: careful\nnote: detail\n"));
    }

    #[test]
    fn json_mode_writes_one_object_per_line_and_never_colours() {
        let mut buf = Vec::new();
        let mut r = Reporter::new(&mut buf, true, Verbosity::Verbose).with_json(true);
        r.error("usage", "bad \x1b[2J \u{202e}thing\u{9b}", Some("hint"));
        r.warn("careful_now", "careful", None);
        r.note("detail_note", "detail");
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(
            out,
            "{\"error\":{\"code\":\"usage\",\"message\":\"bad \\u001b[2J \\u202ething\\u009b\",\"hint\":\"hint\"}}\n\
             {\"warning\":{\"code\":\"careful_now\",\"message\":\"careful\"}}\n\
             {\"note\":{\"code\":\"detail_note\",\"message\":\"detail\"}}\n"
        );
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // Escaped, not sanitised: the exact text comes back.
        assert_eq!(
            lines[0]["error"]["message"],
            "bad \x1b[2J \u{202e}thing\u{9b}"
        );
    }

    #[test]
    fn settings_build_a_reporter_that_writes_the_same_way() {
        let mut buffered = Vec::new();
        let mut direct = Vec::new();
        let mut main = Reporter::new(&mut direct, false, Verbosity::Quiet).with_json(true);
        let settings = main.settings();
        let mut worker = settings.reporter(&mut buffered);
        worker.warn("hidden", "quiet hides this", None);
        worker.error("shown", "an error", None);
        assert_eq!(worker.settings(), settings);
        main.forward(&buffered);
        assert_eq!(
            String::from_utf8(direct).unwrap(),
            "{\"error\":{\"code\":\"shown\",\"message\":\"an error\"}}\n"
        );
    }

    #[test]
    fn coloured_labels_are_reset() {
        let out = report(true, Verbosity::Verbose);
        assert!(
            out.starts_with("\x1b[1m\x1b[31merror\x1b[0m: bad"),
            "{out:?}"
        );
        assert!(out.contains("\x1b[36mhint\x1b[0m"), "{out:?}");
        assert!(out.contains("\x1b[33mwarning\x1b[0m"), "{out:?}");
        assert!(out.contains("\x1b[1mnote\x1b[0m"), "{out:?}");
    }
}
