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
//! - `payload`: the structured payload subcommands (`qr wifi`, ...);
//! - `decode`: `qr decode`, reading codes back from images;
//! - [`output`]: output format resolution, atomic file writes, `data:`
//!   URIs and the `--json` objects;
//! - `style` and `verify`: colour and shape options, contrast warnings and
//!   the `--verify` read-back check;
//! - `logo`: `--logo`, read through `qr_core`'s bounded image loader;
//! - [`diag`]: errors, warnings and notes on stderr;
//! - [`env`](mod@env): terminal and environment detection;
//! - [`error`] and [`exit`]: typed errors and the exit status contract.

#![forbid(unsafe_code)]

pub mod cli;
mod decode;
pub mod diag;
pub mod env;
pub mod error;
pub mod exit;
mod generate;
pub mod input;
mod logo;
pub mod output;
mod payload;
mod style;
mod tooling;
mod verify;

use std::ffi::OsString;
use std::io::{Read, Write};

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{CommandFactory as _, Parser as _, Subcommand as _};
use qr_core::{sanitize_for_terminal, sanitize_lines_for_terminal};

pub use crate::cli::Cli;
use crate::cli::{Command, PayloadCommand};
use crate::decode::Decode;
use crate::diag::{Reporter, Verbosity};
pub use crate::env::Env;
pub use crate::error::CliError;
pub use crate::exit::ExitStatus;
use crate::generate::{Generate, Source};
use crate::input::InputRequest;

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
    let parsed = Cli::try_parse_from(&args);
    let misplaced = match &parsed {
        // `qr -o a.png wifi` parses as TEXT 'wifi'; `qr -o a.png tel 1`
        // fails on the stray '1'. Either way the user meant the subcommand.
        Ok(cli) if cli.command.is_none() && cli.generate.text.is_some() => {
            misplaced_subcommand(&args)
        }
        Err(err) if err.use_stderr() => misplaced_subcommand(&args),
        _ => None,
    };
    if let Some(misplaced) = misplaced {
        Reporter::new(&mut *ctx.stderr, false, Verbosity::Normal)
            .with_json(wants_json(&args))
            .error("usage", &misplaced.message(), Some(&misplaced.hint()));
        return ExitStatus::Usage;
    }
    let cli = match parsed {
        Ok(cli) => cli,
        Err(err) => return report_parse_error(&err, &args, ctx),
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
    .with_json(cli.json());

    let done = |result: Result<(), CliError>| result.map(|()| ExitStatus::Success);
    let result = match &cli.command {
        Some(Command::Completions { shell }) => done(tooling::completions(*shell, &mut **stdout)),
        Some(Command::Man) => done(tooling::man(&mut **stdout)),
        Some(Command::Decode(args)) => Decode {
            args,
            env,
            stdin: &mut **stdin,
            stdout: &mut **stdout,
            reporter: &mut reporter,
        }
        .run(),
        Some(Command::Payload(command)) if command.output().print_payload => done(
            payload::build(command, &mut **stdin, env.stdin_is_tty)
                .and_then(|p| payload::print(&p, &mut **stdout, env.stdout_is_tty)),
        ),
        Some(Command::Payload(command)) => done(
            Generate {
                args: &command.output().render,
                source: Source::Payload(command),
                color: global.color,
                env,
                stdin: &mut **stdin,
                stdout: &mut **stdout,
                reporter: &mut reporter,
            }
            .run(),
        ),
        None => done(
            Generate {
                args: &cli.generate.render,
                source: Source::Input(InputRequest {
                    text: cli.generate.text.as_deref(),
                    file: cli.generate.input.as_deref(),
                    keep_newline: cli.generate.keep_newline,
                }),
                color: global.color,
                env,
                stdin: &mut **stdin,
                stdout: &mut **stdout,
                reporter: &mut reporter,
            }
            .run(),
        ),
    }
    .and_then(|status| stdout.flush().map(|()| status).map_err(CliError::Output));

    match result {
        Ok(status) => status,
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
fn report_parse_error(err: &clap::Error, args: &[OsString], ctx: &mut Context<'_>) -> ExitStatus {
    let json = wants_json(args);
    let literal = literal_word_hint(err, args);
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
            .error("usage", &message, hint.or(literal.as_deref()));
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
        if let Some(hint) = literal {
            let _ = writeln!(ctx.stderr, "  hint: {hint}");
        }
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

/// For `qr wifi` with its required fields missing: how to encode the word
/// itself instead (DECISIONS §9.5).
fn literal_word_hint(err: &clap::Error, args: &[OsString]) -> Option<String> {
    if err.kind() != ErrorKind::MissingRequiredArgument {
        return None;
    }
    let word = args.get(1)?.to_str()?;
    is_payload_subcommand(word)
        .then(|| format!("to encode the word '{word}' itself, run: qr -- {word}"))
}

/// Whether `word` names a payload subcommand (`wifi`, `url`, ...).
fn is_payload_subcommand(word: &str) -> bool {
    PayloadCommand::augment_subcommands(clap::Command::new("qr"))
        .get_subcommands()
        .any(|sub| sub.get_name() == word)
}

/// A payload subcommand written after options, as in `qr -o a.png wifi`.
struct Misplaced {
    /// The subcommand.
    name: String,
    /// The options before it.
    before: Vec<String>,
    /// Everything after it.
    after: Vec<String>,
}

impl Misplaced {
    fn message(&self) -> String {
        format!("options go after the subcommand: '{}'", self.name)
    }

    /// The command line with the options moved after the subcommand.
    fn hint(&self) -> String {
        let words: Vec<String> = std::iter::once(self.name.clone())
            .chain(redact_passwords(&self.after))
            .chain(redact_passwords(&self.before))
            .map(|w| shell_quote(&w))
            .collect();
        format!(
            "run: qr {} (or put -- before '{}' to encode the word itself)",
            words.join(" "),
            self.name
        )
    }
}

/// Finds a payload subcommand name that is the first positional word but
/// comes after options. Words after `--` are data, and option values (as
/// in `-o wifi`) are skipped, so only a word clap took as TEXT matches.
fn misplaced_subcommand(args: &[OsString]) -> Option<Misplaced> {
    let mut command = Cli::command();
    command.build();
    let takes_value = |arg: &clap::Arg| arg.get_action().takes_values();
    let long = |name: &str| {
        command
            .get_arguments()
            .find(|a| a.get_long() == Some(name))
            .is_some_and(takes_value)
    };
    let short = |c: char| {
        command
            .get_arguments()
            .find(|a| a.get_short() == Some(c))
            .is_some_and(takes_value)
    };
    let words: Vec<&str> = args
        .iter()
        .skip(1)
        .map(|a| a.to_str())
        .collect::<Option<_>>()?;
    let mut i = 0;
    while let Some(&word) = words.get(i) {
        if word == "--" {
            return None;
        }
        if let Some(name) = word.strip_prefix("--") {
            if !name.contains('=') && long(name) {
                i += 1;
            }
        } else if let Some(cluster) = word.strip_prefix('-').filter(|c| !c.is_empty()) {
            // `-qo a.png`: the first flag that takes a value ends the cluster.
            if let Some(at) = cluster
                .char_indices()
                .find(|&(_, c)| short(c))
                .map(|(at, _)| at)
            {
                if at + 1 == cluster.len() {
                    i += 1;
                }
            }
        } else {
            return (i > 0 && is_payload_subcommand(word)).then(|| Misplaced {
                name: word.to_owned(),
                before: words[..i].iter().map(|&w| w.to_owned()).collect(),
                after: words[i + 1..].iter().map(|&w| w.to_owned()).collect(),
            });
        }
        i += 1;
    }
    None
}

/// Replaces `--password` values, so a corrected command line shown in a
/// hint does not repeat a secret.
fn redact_passwords(words: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(words.len());
    let mut secret_next = false;
    for word in words {
        if std::mem::take(&mut secret_next) {
            out.push("PASSWORD".to_owned());
        } else if word.starts_with("--password=") {
            out.push("--password=PASSWORD".to_owned());
        } else {
            secret_next = word == "--password";
            out.push(word.clone());
        }
    }
    out
}

/// Quotes `word` for a POSIX shell when it holds anything but plain
/// characters.
fn shell_quote(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./:=@+,%".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests;
