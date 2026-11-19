//! The default command: read the input, encode it and render it.

use std::io::{Read, Write};

use qr_core::render::DEFAULT_MARGIN;
use qr_core::{
    EncodeOptions, Matrix, QrCode, TextFormat, TextOptions, VersionChoice, encode, render_text,
};

use crate::cli::{ColorChoice, GenerateArgs, OutputFormat};
use crate::diag::Reporter;
use crate::env::Env;
use crate::error::CliError;
use crate::input::{Input, InputRequest, read_input};

/// Streams and settings the generate command needs.
pub(crate) struct Generate<'a, 'r> {
    pub(crate) args: &'a GenerateArgs,
    pub(crate) color: ColorChoice,
    pub(crate) env: &'a Env,
    pub(crate) stdin: &'a mut dyn Read,
    pub(crate) stdout: &'a mut dyn Write,
    pub(crate) reporter: &'a mut Reporter<'r>,
}

impl Generate<'_, '_> {
    /// Runs the command.
    pub(crate) fn run(self) -> Result<(), CliError> {
        let Self {
            args,
            color,
            env,
            stdin,
            stdout,
            reporter,
        } = self;

        let request = InputRequest {
            text: args.text.as_deref(),
            file: args.input.as_deref(),
            keep_newline: args.keep_newline,
        };
        let input = read_input(&request, stdin, env.stdin_is_tty)?;
        let code = encode(&input.data, &encode_options(args))?;
        reporter.note(&describe(&input, &code));

        let format = args.format.unwrap_or(if env.stdout_is_tty {
            OutputFormat::Terminal
        } else {
            OutputFormat::Text
        });
        let text_format = match format {
            OutputFormat::Terminal if color.enabled(env.stdout_is_tty, env) => TextFormat::Ansi,
            OutputFormat::Terminal => {
                reporter.note(
                    "colour is off, so the code is drawn in the terminal's own colours; \
                     if it looks reversed, add --invert",
                );
                TextFormat::Unicode
            }
            OutputFormat::Text => TextFormat::Unicode,
            OutputFormat::Ascii => TextFormat::Ascii,
        };
        let options = TextOptions::default()
            .with_margin(usize::from(args.margin))
            .with_invert(args.invert);

        if options.margin < DEFAULT_MARGIN {
            reporter.warn(
                &format!(
                    "a margin of {} is below the {DEFAULT_MARGIN}-module quiet zone \
                     the standard requires; some scanners may fail",
                    options.margin
                ),
                Some("use --margin 4"),
            );
        }
        // Any text format wraps on a narrow terminal, and ASCII (two columns
        // per module) is the widest of them.
        if env.stdout_is_tty {
            check_width(code.matrix(), text_format, &options, env, reporter);
        }

        let rendered = render_text(code.matrix(), text_format, &options);
        stdout
            .write_all(rendered.as_bytes())
            .map_err(CliError::Output)
    }
}

/// The encoder options selected by the flags.
fn encode_options(args: &GenerateArgs) -> EncodeOptions {
    EncodeOptions::default()
        .with_ec_level(args.ec_level)
        .with_mode(args.mode)
        .with_version(
            args.qr_version
                .map_or(VersionChoice::Auto, VersionChoice::AtLeast),
        )
        .with_mask(args.mask)
}

/// The `-v` summary of what was encoded.
fn describe(input: &Input, code: &QrCode) -> String {
    let width = code.matrix().width();
    format!(
        "encoded {len} bytes from {source} as version {version} ({width}x{width}), \
         {mode} mode, error correction {ec}, mask {mask}",
        len = input.data.len(),
        source = input.source,
        version = code.version(),
        mode = code.mode(),
        ec = code.ec_level(),
        mask = code.mask(),
    )
}

/// Warns when the rendering is wider than the terminal, where it would wrap
/// and could not scan.
fn check_width(
    matrix: &Matrix,
    format: TextFormat,
    options: &TextOptions,
    env: &Env,
    reporter: &mut Reporter<'_>,
) {
    let Some(available) = env.terminal_width else {
        return;
    };
    let needed = format.columns(matrix, options);
    if needed > available {
        reporter.warn(
            &format!(
                "the code is {needed} columns wide but the terminal has {available}; \
                 it will wrap and not scan"
            ),
            Some(if format == TextFormat::Ascii {
                "widen the terminal, use -f text (half as wide) or shorten the input"
            } else {
                "widen the terminal or shorten the input"
            }),
        );
    }
}
