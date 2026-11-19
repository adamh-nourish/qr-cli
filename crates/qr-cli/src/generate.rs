//! The default command: read the input, encode it, render it and write it.

use std::io::{Read, Write};

use qr_core::render::{DEFAULT_MARGIN, DEFAULT_SCALE};
use qr_core::{
    Color, EncodeOptions, Matrix, QrCode, RasterFormat, RenderOptions, TextFormat, TextOptions,
    VersionChoice, encode, render_raster, render_text, render_vector,
};

use crate::cli::{ColorChoice, GenerateArgs, OutputFormat};
use crate::diag::Reporter;
use crate::env::Env;
use crate::error::CliError;
use crate::input::{Input, InputRequest, read_input};
use crate::output::json::{self, Generated};
use crate::output::{Delivered, Plan, Renderer, Target};

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

        // Output errors come first, before stdin is consumed.
        let plan = Plan::resolve(args, env.stdout_is_tty, reporter)?;

        let request = InputRequest {
            text: args.text.as_deref(),
            file: args.input.as_deref(),
            keep_newline: args.keep_newline,
        };
        let input = read_input(&request, stdin, env.stdin_is_tty)?;
        let code = encode(&input.data, &encode_options(args))?;
        reporter.note("encoded", &describe(&input, &code));

        if usize::from(args.margin) < DEFAULT_MARGIN {
            reporter.warn(
                "margin_below_quiet_zone",
                &format!(
                    "a margin of {} is below the {DEFAULT_MARGIN}-module quiet zone \
                     the standard requires; some scanners may fail",
                    args.margin
                ),
                Some("use --margin 4"),
            );
        }

        let rendered = match plan.format.renderer() {
            Renderer::Raster(format) => {
                warn_lost_settings(args, plan.format, reporter);
                render_raster(code.matrix(), format, &render_options(args))?
            }
            Renderer::Vector(format) => {
                warn_lost_settings(args, plan.format, reporter);
                render_vector(code.matrix(), format, &render_options(args))?
            }
            Renderer::Text => {
                warn_unused_image_flags(args, reporter);
                // Colour follows the stream the text ends up on.
                let enabled = color.enabled(plan.is_stdout() && env.stdout_is_tty, env);
                let text_format = plan.format.text_format(enabled).ok_or_else(|| {
                    CliError::Internal(format!("{} is not a text format", plan.format.name()))
                })?;
                if plan.format == OutputFormat::Terminal && !enabled && plan.is_stdout() {
                    reporter.note(
                        "colour_off",
                        "colour is off, so the code is drawn in the terminal's own colours; \
                         if it looks reversed, add --invert",
                    );
                }
                let options = TextOptions::default()
                    .with_margin(usize::from(args.margin))
                    .with_invert(args.invert);
                // Any text format wraps on a narrow terminal, and ASCII (two
                // columns per module) is the widest of them.
                if plan.is_stdout() && env.stdout_is_tty {
                    check_width(code.matrix(), text_format, &options, env, reporter);
                }
                render_text(code.matrix(), text_format, &options).into_bytes()
            }
        };

        let delivered = plan.deliver(rendered, stdout)?;
        if plan.json {
            let line = json::object(&result(&plan, &input, &code, delivered));
            stdout
                .write_all(line.as_bytes())
                .map_err(CliError::Output)?;
        }
        Ok(())
    }
}

/// The renderer options selected by the flags. `--invert` swaps the
/// colours, so it composes with `--fg` and `--bg`.
fn render_options(args: &GenerateArgs) -> RenderOptions {
    let (mut fg, mut bg) = (
        args.fg.unwrap_or(Color::BLACK),
        args.bg.unwrap_or(Color::WHITE),
    );
    if args.invert {
        std::mem::swap(&mut fg, &mut bg);
    }
    RenderOptions::default()
        .with_margin(usize::from(args.margin))
        .with_scale(args.scale.unwrap_or(DEFAULT_SCALE))
        .with_size(args.size)
        .with_dpi(args.dpi)
        .with_fg(fg)
        .with_bg(bg)
        .with_transparent(args.transparent)
}

/// Warns about image-only flags given with a text format, where they would
/// otherwise be silently ignored.
fn warn_unused_image_flags(args: &GenerateArgs, reporter: &mut Reporter<'_>) {
    let unused: Vec<&str> = [
        (args.scale.is_some(), "--scale"),
        (args.size.is_some(), "--size"),
        (args.dpi.is_some(), "--dpi"),
        (args.fg.is_some(), "--fg"),
        (args.bg.is_some(), "--bg"),
        (args.transparent, "--transparent"),
    ]
    .into_iter()
    .filter_map(|(set, flag)| set.then_some(flag))
    .collect();
    if !unused.is_empty() {
        let verb = if unused.len() == 1 {
            "applies"
        } else {
            "apply"
        };
        reporter.warn(
            "image_flags_ignored",
            &format!("{} only {verb} to image formats", unused.join(", ")),
            Some("add -f svg or -o code.svg"),
        );
    }
}

/// Warns about image settings `format` cannot keep: `--dpi` outside PNG
/// and JPEG, and transparency in a JPEG.
fn warn_lost_settings(args: &GenerateArgs, format: OutputFormat, reporter: &mut Reporter<'_>) {
    let raster = match format.renderer() {
        Renderer::Raster(raster) => Some(raster),
        Renderer::Vector(_) | Renderer::Text => None,
    };
    if args.dpi.is_some() && !raster.is_some_and(RasterFormat::records_dpi) {
        reporter.warn(
            "dpi_ignored",
            &format!("--dpi has no effect on {} output", format.name()),
            Some(if raster.is_some() {
                "use -f png or -f jpeg to record a pixel density"
            } else {
                "vector output has no pixels; set its size with --scale"
            }),
        );
    }
    let translucent = [args.fg, args.bg]
        .into_iter()
        .flatten()
        .any(|c| !c.is_opaque());
    if (args.transparent || translucent) && raster.is_some_and(|f| !f.has_alpha()) {
        reporter.warn(
            "transparency_flattened",
            &format!(
                "{} has no transparency, so the colours are flattened onto white",
                format.name()
            ),
            Some("use -f png or -f webp to keep transparency"),
        );
    }
}

/// The `--json` result object.
fn result(plan: &Plan, input: &Input, code: &QrCode, delivered: Delivered) -> Generated {
    Generated {
        output: match &plan.target {
            Target::File(path) => Some(path.to_string_lossy().into_owned()),
            Target::Stdout => None,
        },
        format: plan.format.name(),
        version: code.version().number(),
        ec: code.ec_level().to_string(),
        mode: code.mode().to_string(),
        mask: code.mask().number(),
        modules: code.matrix().width(),
        bytes: input.data.len(),
        written: delivered.written,
        data_uri: delivered.data_uri,
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
            "terminal_too_narrow",
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
