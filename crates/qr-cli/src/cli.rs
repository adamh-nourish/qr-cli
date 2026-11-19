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
use qr_core::render::{MAX_CANVAS_SIDE, MAX_DPI};
use qr_core::{Color, EcLevel, Mask, ModeChoice, ModuleShape, Version};

/// The largest `--margin` accepted, in modules. Equal to the renderer's own
/// cap, [`qr_core::render::MAX_MARGIN`] (a test keeps them in step).
pub const MAX_MARGIN: u16 = 100;

/// The largest `--scale` accepted: one module filling the largest canvas
/// any renderer draws. The renderer still checks the whole canvas.
pub const MAX_SCALE: u32 = MAX_CANVAS_SIDE;

const AFTER_HELP: &str = "\
Examples:
  qr 'https://example.com'     Show a QR code in the terminal
  echo hello | qr              Encode stdin (one trailing newline is dropped)
  qr 'hi' -o code.svg          Write an SVG file (format from the extension)
  qr 'hi' --size 512 -o c.png  Write a 512x512 PNG image
  qr 'hi' -f pdf -o - > c.pdf  Write a PDF to stdout
  qr 'hi' --data-uri --json    Print a JSON object with an SVG data: URI
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
#[allow(clippy::struct_excessive_bools)] // Each flag is an independent switch.
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

    /// Write the code to PATH ('-' for stdout, not /dev/stdout). Existing
    /// files are replaced atomically and keep their permissions; a symlink
    /// is replaced, not followed.
    #[arg(short = 'o', long = "output", value_name = "PATH|-")]
    pub output: Option<PathBuf>,

    /// Output format [default: from the -o extension; else terminal when
    /// stdout is a terminal, otherwise text].
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

    /// Size of one module in pixels (PNG, JPEG, WebP, SVG) or points (PDF,
    /// EPS) [default: 10].
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_SCALE))
    )]
    pub scale: Option<u32>,

    /// Width and height of a PNG, JPEG or WebP image in pixels, with the
    /// largest whole module size that fits (vector formats are at most this
    /// size).
    #[arg(
        long,
        value_name = "PX",
        conflicts_with = "scale",
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_CANVAS_SIDE))
    )]
    pub size: Option<u32>,

    /// Pixel density to record in a PNG or JPEG file, in dots per inch.
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_DPI))
    )]
    pub dpi: Option<u32>,

    /// Colour of dark modules: #RGB, #RRGGBB, #RRGGBBAA or a CSS name
    /// [default: black].
    #[arg(long, value_name = "COLOR", value_parser = Color::from_str)]
    pub fg: Option<Color>,

    /// Colour of light modules and the quiet zone [default: white].
    #[arg(long, value_name = "COLOR", value_parser = Color::from_str)]
    pub bg: Option<Color>,

    /// Leave the background unpainted (image formats only).
    #[arg(long)]
    pub transparent: bool,

    /// Swap dark and light modules (or --fg and --bg for images).
    #[arg(long)]
    pub invert: bool,

    /// How dark modules are drawn in images; the three finder patterns stay
    /// square.
    #[arg(long, value_enum, value_name = "SHAPE", default_value_t = Shape::Square)]
    pub shape: Shape,

    /// Read the rendered code back with a decoder and fail (exit 6) unless it
    /// holds the input. Automatic for --shape, --fg, --bg, --transparent and
    /// --invert images.
    #[arg(long, overrides_with = "no_verify")]
    pub verify: bool,

    /// Skip the automatic read-back check.
    #[arg(long, overrides_with = "verify")]
    pub no_verify: bool,

    /// Print the image as a base64 data: URI instead of raw bytes
    /// [default format: svg]. With -o, the file holds the URI: use a .txt
    /// name.
    #[arg(long)]
    pub data_uri: bool,

    /// Fail instead of replacing an existing -o file.
    #[arg(long, requires = "output")]
    pub no_clobber: bool,

    /// Print a JSON result object on stdout and JSON errors on stderr.
    #[arg(long)]
    pub json: bool,

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
    /// PNG image (1-bit indexed, or RGBA when transparent).
    Png,
    /// Scalable Vector Graphics.
    Svg,
    /// JPEG image (quality 92; at least 4 pixels per module).
    #[value(alias = "jpg")]
    Jpeg,
    /// Lossless WebP image.
    Webp,
    /// One-page vector PDF.
    Pdf,
    /// Encapsulated PostScript.
    Eps,
}

/// The `--shape` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, ValueEnum)]
pub enum Shape {
    /// Full squares: the standard look.
    #[default]
    Square,
    /// Squares with their exposed corners rounded.
    Rounded,
    /// Circles.
    Dots,
}

impl From<Shape> for ModuleShape {
    fn from(shape: Shape) -> Self {
        match shape {
            Shape::Square => Self::Square,
            Shape::Rounded => Self::Rounded,
            Shape::Dots => Self::Dots,
        }
    }
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
        assert_eq!((g.output, g.scale, g.fg, g.bg), (None, None, None, None));
        assert_eq!((g.size, g.dpi), (None, None));
        assert!(!(g.transparent || g.invert || g.data_uri || g.no_clobber || g.json));
        assert_eq!(g.shape, Shape::Square);
        assert!(!(g.verify || g.no_verify));
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
    fn output_flags_parse() {
        let cli = Cli::try_parse_from([
            "qr",
            "-o",
            "a.svg",
            "-f",
            "jpg",
            "--scale",
            "3",
            "--fg",
            "navy",
            "--bg",
            "#fff8",
            "--no-clobber",
            "x",
        ])
        .unwrap();
        let g = cli.generate;
        assert_eq!(g.output.as_deref(), Some(std::path::Path::new("a.svg")));
        assert_eq!(g.format, Some(OutputFormat::Jpeg));
        assert_eq!(g.scale, Some(3));
        assert_eq!(g.fg, Some(Color::rgb(0, 0, 0x80)));
        assert_eq!(g.bg, Some(Color::rgba(255, 255, 255, 0x88)));
        assert!(g.no_clobber);
    }

    #[test]
    fn size_and_dpi_parse_and_size_conflicts_with_scale() {
        let cli = Cli::try_parse_from(["qr", "--size", "512", "--dpi", "300", "x"]).unwrap();
        assert_eq!(
            (cli.generate.size, cli.generate.dpi),
            (Some(512), Some(300))
        );
        let err = Cli::try_parse_from(["qr", "--size", "512", "--scale", "4", "x"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn shape_and_verify_flags_parse() {
        let parse = |args: &[&str]| Cli::try_parse_from(args).unwrap().generate;
        let g = parse(&["qr", "--shape", "dots", "x"]);
        assert_eq!(g.shape, Shape::Dots);
        assert_eq!(ModuleShape::from(g.shape), ModuleShape::Dots);
        assert_eq!(ModuleShape::from(Shape::Rounded), ModuleShape::Rounded);
        assert_eq!(ModuleShape::from(Shape::Square), ModuleShape::Square);
        // The last of --verify and --no-verify wins.
        let g = parse(&["qr", "--verify", "--no-verify", "x"]);
        assert!(!g.verify && g.no_verify);
        let g = parse(&["qr", "--no-verify", "--verify", "x"]);
        assert!(g.verify && !g.no_verify);
        let err = Cli::try_parse_from(["qr", "--shape", "star", "x"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn no_clobber_needs_an_output_path() {
        let err = Cli::try_parse_from(["qr", "--no-clobber", "x"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
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
            ["qr", "--scale", "0"],
            ["qr", "--scale", "16385"],
            ["qr", "--size", "0"],
            ["qr", "--size", "16385"],
            ["qr", "--dpi", "0"],
            ["qr", "--dpi", "65536"],
            ["qr", "--fg", "notacolour"],
            ["qr", "--bg", "#12"],
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
