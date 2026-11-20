//! Command-line argument definitions.
//!
//! Generation is the root command (`qr [OPTIONS] [TEXT]`); subcommands
//! build structured [`payload`]s (`qr wifi`, `qr vcard`, ...), read codes
//! back (`qr decode`, [`DecodeArgs`]) or are tooling; a later change adds
//! `batch`. Keeping every clap type in this module lets completions and
//! the man page be generated from a single source of truth.

use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use qr_core::logo::LogoScale;
use qr_core::render::{MAX_CANVAS_SIDE, MAX_DPI};
use qr_core::{Color, EcLevel, Mask, ModeChoice, ModuleShape, Version};

pub mod payload;

pub use self::payload::PayloadCommand;

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
  qr 'hi' --shape dots --fg navy -o c.png
                               Write a styled PNG (checked automatically)
  qr 'https://example.com' --logo logo.png -o c.png
                               Put a logo in the middle (EC H, checked)
  qr 'hi' --data-uri --json    Print a JSON object with an SVG data: URI
  qr wifi --ssid Home --password-stdin < pw.txt
                               Share a WiFi network (see 'qr wifi --help')
  qr decode code.png           Print the data held in an image's QR codes
  qr -- wifi                   Encode a word that is also a command name,
                               such as 'wifi', 'url', 'email' or 'help'

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
    /// A payload or tooling subcommand. Without one, `qr` encodes TEXT.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Options for the default generate command.
    #[command(flatten)]
    pub generate: GenerateArgs,

    /// Options that apply to every command.
    #[command(flatten)]
    pub global: GlobalArgs,
}

impl Cli {
    /// The render options in effect: a payload subcommand's own, or the
    /// default command's.
    pub fn render_args(&self) -> &RenderArgs {
        match &self.command {
            Some(Command::Payload(command)) => &command.output().render,
            _ => &self.generate.render,
        }
    }

    /// Whether `--json` was given to the command that runs.
    pub fn json(&self) -> bool {
        match &self.command {
            Some(Command::Decode(decode)) => decode.json,
            _ => self.render_args().json,
        }
    }
}

/// Subcommands.
#[derive(Debug, Clone, PartialEq, Subcommand)]
// Parsed once per run, so the size of the payload variant does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// A structured payload: `url`, `wifi`, `vcard`, `mecard`, `email`,
    /// `sms`, `tel`, `geo` or `event`.
    #[command(flatten)]
    Payload(PayloadCommand),
    /// Read the QR codes in PNG, JPEG or WebP images and print their data.
    Decode(DecodeArgs),
    /// Print a shell completion script to stdout.
    Completions {
        /// The shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Print the man page (roff) to stdout.
    Man,
}

/// Options for generating a code from text, a file or stdin.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct GenerateArgs {
    /// Text to encode; quote it if it contains spaces. '-' reads stdin, which
    /// is also the default when stdin is not a terminal. Put -- before text
    /// that is also a command name: qr -- wifi.
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

    /// Keep a trailing newline read from stdin or a file.
    #[arg(long)]
    pub keep_newline: bool,

    /// Output, encoding and styling options.
    #[command(flatten)]
    pub render: RenderArgs,
}

/// Options that control how a code is encoded, drawn and written. The
/// default command and every payload subcommand share them.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[allow(clippy::struct_excessive_bools)] // Each flag is an independent switch.
pub struct RenderArgs {
    /// Write the code to PATH ('-' for stdout, not /dev/stdout). Existing
    /// files are replaced atomically and keep their permissions; a symlink
    /// is replaced, not followed.
    #[arg(short = 'o', long = "output", value_name = "PATH|-")]
    pub output: Option<PathBuf>,

    /// Output format [default: from the -o extension; else terminal when
    /// stdout is a terminal, otherwise text].
    #[arg(short = 'f', long = "format", value_enum, value_name = "FORMAT")]
    pub format: Option<OutputFormat>,

    /// Error correction level: L, M, Q or H [default: M; H with --logo].
    #[arg(long = "ec", value_name = "LEVEL", value_parser = EcLevel::from_str)]
    pub ec_level: Option<EcLevel>,

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

    /// How dark modules are drawn in png, jpeg, webp, svg, pdf and eps
    /// output; the three finder patterns stay square.
    #[arg(long, value_enum, value_name = "SHAPE", default_value_t = Shape::Square)]
    pub shape: Shape,

    /// Draw a PNG, JPEG or WebP image (at most 10 MiB and 8192x8192) over
    /// the centre of a png, jpeg, webp or svg code. Sets --ec H (a lower
    /// --ec is overridden with a warning) and checks the code reads back.
    #[arg(long, value_name = "FILE")]
    pub logo: Option<PathBuf>,

    /// Width of the --logo as a fraction of the code's width, 0.05 to 0.3
    /// [default: 0.2].
    #[arg(long, value_name = "FRACTION", requires = "logo", value_parser = LogoScale::from_str)]
    pub logo_scale: Option<LogoScale>,

    /// Read the rendered code back with a decoder and fail (exit 6) unless it
    /// holds the input. Automatic for images with a --logo or a non-default
    /// --shape/--fg/--bg, --transparent or --invert. Text output is checked
    /// as a plain module grid (the symbol, not the characters).
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
}

const DECODE_AFTER_HELP: &str = "\
Each code's data is printed on its own line, with control and bidi
characters escaped (\\u{1b}) and bytes that are not UTF-8 as \\x{ff}.
With several images, each line starts with the image's path.

Examples:
  qr decode code.png           Print the data in code.png
  qr decode a.png b.jpg --json Print a JSON object for both images
  qr decode - < code.webp      Read the image from stdin
  qr decode code.png --raw > data.txt
                               Write the data unescaped, one code per line
                               (not to a terminal)

Exit codes:
  0 every image held a code, 4 an image could not be read,
  5 an image held no readable code (4 wins if both happen)";

/// Options for `qr decode`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[command(after_help = DECODE_AFTER_HELP)]
pub struct DecodeArgs {
    /// PNG, JPEG or WebP images to read (at most 10 MiB and 8192x8192
    /// pixels each); '-' reads one from stdin. The format comes from the
    /// file's contents, not its name.
    #[arg(value_name = "IMAGE", required = true, num_args = 1..)]
    pub images: Vec<PathBuf>,

    /// Print one JSON object on stdout with every image's codes (data,
    /// version, error correction, mask and corners) and JSON errors on
    /// stderr.
    #[arg(long, conflicts_with = "raw")]
    pub json: bool,

    /// Print each code's data unescaped, followed by a newline, with no
    /// path prefix even for several images. Refused when stdout is a
    /// terminal, where the data could send escape sequences.
    #[arg(long)]
    pub raw: bool,
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

    /// Also print details on stderr (what was encoded, or decoded, and how).
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
        assert_eq!(cli.generate.text.as_deref(), Some("x"));
        assert!(!cli.generate.keep_newline);
        assert_eq!(cli.generate.input, None);
        let g = cli.render_args().clone();
        assert_eq!(g.ec_level, None);
        assert_eq!((g.logo, g.logo_scale), (None, None));
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
    fn payload_subcommands_take_the_render_options() {
        let cli = Cli::try_parse_from([
            "qr", "wifi", "--ssid", "Home", "--auth", "WPA2", "--hidden", "-o", "w.png", "--shape",
            "dots", "--ec", "H",
        ])
        .unwrap();
        let Some(Command::Payload(PayloadCommand::Wifi(wifi))) = &cli.command else {
            panic!("expected wifi: {:?}", cli.command);
        };
        assert_eq!(wifi.ssid, "Home");
        assert_eq!(wifi.auth, qr_core::payload::WifiAuth::Wpa);
        assert!(wifi.hidden && !wifi.password_stdin && wifi.password.is_none());
        let render = cli.render_args();
        assert_eq!(
            render.output.as_deref(),
            Some(std::path::Path::new("w.png"))
        );
        assert_eq!(
            (render.shape, render.ec_level),
            (Shape::Dots, Some(EcLevel::H))
        );
        assert!(!wifi.output.print_payload);
    }

    #[test]
    fn payload_flags_conflict_where_they_should() {
        for args in [
            &[
                "qr",
                "wifi",
                "--ssid",
                "a",
                "--password",
                "p",
                "--password-stdin",
            ][..],
            &["qr", "tel", "1", "--print-payload", "-o", "a.png"],
            &["qr", "tel", "1", "--print-payload", "--json"],
            &["qr", "-o", "a.png", "tel", "1"],
        ] {
            let err = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "{args:?}"
            );
        }
        let err = Cli::try_parse_from(["qr", "wifi", "--ssid", "a", "--auth", "wpa4"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(err.to_string().contains("expected wpa, wep or nopass"));
    }

    #[test]
    fn geo_takes_negative_coordinates() {
        let cli = Cli::try_parse_from(["qr", "geo", "-33.8", "-151.2", "--print-payload"]).unwrap();
        let Some(Command::Payload(PayloadCommand::Geo(geo))) = cli.command else {
            panic!("expected geo");
        };
        assert_eq!((geo.latitude, geo.longitude), (-33.8, -151.2));
        assert!(geo.output.print_payload);
    }

    #[test]
    fn every_payload_name_is_literal_text_after_double_dash() {
        for word in [
            "url", "wifi", "vcard", "mecard", "email", "sms", "tel", "geo", "event",
        ] {
            let cli = Cli::try_parse_from(["qr", "--", word]).unwrap();
            assert!(cli.command.is_none(), "{word}");
            assert_eq!(cli.generate.text.as_deref(), Some(word));
            // Without `--`, the word is the subcommand, which needs arguments.
            assert!(Cli::try_parse_from(["qr", word]).is_err(), "{word}");
        }
    }

    #[test]
    fn decode_takes_images_and_flags() {
        let cli = Cli::try_parse_from(["qr", "decode", "a.png", "-", "--json"]).unwrap();
        let Some(Command::Decode(decode)) = &cli.command else {
            panic!("expected decode: {:?}", cli.command);
        };
        assert_eq!(decode.images, [PathBuf::from("a.png"), PathBuf::from("-")]);
        assert!(decode.json && !decode.raw);
        assert!(cli.json());
        let cli = Cli::try_parse_from(["qr", "decode", "a.png", "--raw", "-q"]).unwrap();
        assert!(!cli.json() && cli.global.quiet);
        // At least one image; --raw and --json do not mix.
        let err = Cli::try_parse_from(["qr", "decode"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        let err = Cli::try_parse_from(["qr", "decode", "a", "--raw", "--json"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        // The word itself can still be encoded.
        let cli = Cli::try_parse_from(["qr", "--", "decode"]).unwrap();
        assert_eq!(cli.generate.text.as_deref(), Some("decode"));
        assert!(!cli.json());
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
        let g = cli.generate.render;
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
            (cli.generate.render.size, cli.generate.render.dpi),
            (Some(512), Some(300))
        );
        let err = Cli::try_parse_from(["qr", "--size", "512", "--scale", "4", "x"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn shape_and_verify_flags_parse() {
        let parse = |args: &[&str]| Cli::try_parse_from(args).unwrap().generate.render;
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
    fn logo_flags_parse() {
        let cli =
            Cli::try_parse_from(["qr", "--logo", "l.png", "--logo-scale", "0.3", "x"]).unwrap();
        let g = cli.generate.render;
        assert_eq!(g.logo.as_deref(), Some(std::path::Path::new("l.png")));
        assert_eq!(g.logo_scale, Some(LogoScale::MAX));
        // The scale needs a logo and a value in range.
        let err = Cli::try_parse_from(["qr", "--logo-scale", "0.2", "x"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        for bad in ["0.31", "0.04", "big"] {
            let err = Cli::try_parse_from(["qr", "--logo", "l.png", "--logo-scale", bad, "x"])
                .unwrap_err();
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation, "{bad}");
        }
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
