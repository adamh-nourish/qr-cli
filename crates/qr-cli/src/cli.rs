//! Command-line argument definitions.
//!
//! Generation is the root command (`qr [OPTIONS] [TEXT]`); subcommands are
//! for tooling and, in later changes, structured payloads, `decode` and
//! `batch`. Keeping every clap type in this module lets completions and the
//! man page be generated from a single source of truth.

use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use qr_core::{EcLevel, Mask, ModeChoice, Version};

/// The largest `--margin` accepted, in modules. Equal to the renderer's own
/// cap, [`qr_core::render::MAX_MARGIN`] (a test keeps them in step).
pub const MAX_MARGIN: u16 = 100;

const AFTER_HELP: &str = "\
Examples:
  qr 'https://example.com'     Show a QR code in the terminal
  echo hello | qr              Encode stdin (one trailing newline is dropped)
  qr -i payload.txt -f ascii   Encode a file as ASCII art
  qr -- help                   Encode a word that is also a command name,
                               such as 'help', 'man' or 'completions'

Exit codes:
  0 ok, 1 internal error, 2 usage error, 3 data too long, 4 I/O error,
  5 no code found (decode), 6 verification failed";

/// Generate QR codes from text, files or standard input.
#[derive(Debug, Parser)]
#[command(
    name = "qr",
    // Fixed, so help and errors say `qr` rather than `qr.exe` on Windows.
    bin_name = "qr",
    version,
    about,
    long_about = None,
    after_help = AFTER_HELP,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// A tooling subcommand. Without one, `qr` generates a code.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Options for the default generate command.
    #[command(flatten)]
    pub generate: GenerateArgs,

    /// Options that apply to every command.
    #[command(flatten)]
    pub global: GlobalArgs,
}

/// Subcommands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Print a shell completion script to stdout.
    Completions {
        /// The shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print the man page (roff) to stdout.
    Man,
}

/// Options for generating a code.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct GenerateArgs {
    /// Text to encode; quote it if it contains spaces. '-' reads stdin, which
    /// is also the default when stdin is not a terminal.
    #[arg(value_name = "TEXT")]
    pub text: Option<String>,

    /// Read the data to encode from FILE (raw bytes); '-' reads stdin.
    #[arg(
        short = 'i',
        long = "input",
        value_name = "FILE",
        conflicts_with = "text"
    )]
    pub input: Option<PathBuf>,

    /// Output format [default: terminal when stdout is a terminal, else text].
    #[arg(short = 'f', long = "format", value_enum, value_name = "FORMAT")]
    pub format: Option<OutputFormat>,

    /// Error correction level: L, M, Q or H.
    #[arg(long = "ec", value_name = "LEVEL", default_value = "M", value_parser = EcLevel::from_str)]
    pub ec_level: EcLevel,

    /// Smallest symbol version to use, 1 to 40 (larger data still grows it).
    #[arg(long = "qr-version", value_name = "N", value_parser = Version::from_str)]
    pub qr_version: Option<Version>,

    /// Data mode: auto, numeric, alnum or byte.
    #[arg(long, value_name = "MODE", default_value = "auto", value_parser = ModeChoice::from_str)]
    pub mode: ModeChoice,

    /// Force mask pattern 0 to 7 instead of picking one automatically.
    #[arg(long, value_name = "N", value_parser = Mask::from_str)]
    pub mask: Option<Mask>,

    /// Quiet zone around the code, in modules (below 4 may not scan).
    #[arg(
        long,
        value_name = "N",
        default_value_t = 4,
        value_parser = clap::value_parser!(u16).range(0..=i64::from(MAX_MARGIN))
    )]
    pub margin: u16,

    /// Swap dark and light modules (for light-on-dark text output).
    #[arg(long)]
    pub invert: bool,

    /// Keep a trailing newline read from stdin or a file.
    #[arg(long)]
    pub keep_newline: bool,
}

/// Options shared by every command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub struct GlobalArgs {
    /// When to use colour; NO_COLOR and CLICOLOR_FORCE are honoured in auto.
    // Plain text on purpose: clap prints this line in `--help`.
    #[allow(clippy::doc_markdown)]
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = ColorChoice::Auto, global = true)]
    pub color: ColorChoice,

    /// Print errors only, no warnings.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Also print what was encoded and how.
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

/// How a code is written out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum OutputFormat {
    /// Unicode half blocks, forced black on white with ANSI colours.
    Terminal,
    /// Unicode half blocks with no escape sequences.
    Text,
    /// '##' and spaces, pure ASCII.
    Ascii,
}

/// The `--color` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, ValueEnum)]
pub enum ColorChoice {
    /// Colour when writing to a terminal, unless the environment says not to.
    #[default]
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults_match_the_design_record() {
        let cli = Cli::try_parse_from(["qr", "x"]).unwrap();
        let g = cli.generate;
        assert_eq!(g.text.as_deref(), Some("x"));
        assert_eq!(g.ec_level, EcLevel::M);
        assert_eq!(g.mode, ModeChoice::Auto);
        assert_eq!(g.margin, 4);
        assert_eq!((g.qr_version, g.mask, g.format), (None, None, None));
        assert_eq!(cli.global.color, ColorChoice::Auto);
        assert!(cli.command.is_none());
    }

    #[test]
    fn subcommand_names_can_be_encoded_after_double_dash() {
        let cli = Cli::try_parse_from(["qr", "--", "completions"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.generate.text.as_deref(), Some("completions"));
        let cli = Cli::try_parse_from(["qr", "man"]).unwrap();
        assert_eq!(cli.command, Some(Command::Man));
    }

    #[test]
    fn text_and_input_file_conflict() {
        let err = Cli::try_parse_from(["qr", "x", "-i", "f"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn quiet_and_verbose_conflict() {
        let err = Cli::try_parse_from(["qr", "-q", "-v", "x"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn option_values_are_validated() {
        for args in [
            ["qr", "--ec", "X"],
            ["qr", "--qr-version", "41"],
            ["qr", "--mode", "kanji"],
            ["qr", "--mask", "8"],
            ["qr", "--margin", "101"],
        ] {
            let err = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation,
                "{args:?}"
            );
        }
    }
}
