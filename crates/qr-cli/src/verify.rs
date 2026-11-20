//! `--verify` and `--no-verify`: read the rendered code back with `rqrr`
//! before anything is written.
//!
//! Verification runs when asked for, and automatically for styled images
//! (see [`style::is_styled`](crate::style::is_styled)). An explicit
//! `--verify` that fails is an error (exit 6) and nothing is written. What
//! an automatic check does on failure is [`AUTO_VERIFY`]: DECISIONS
//! section 9, risk 2, makes it fail only if `rqrr`'s false-negative rate on
//! styled codes is at most 2%, and warn otherwise.

use qr_core::verify::{Verified, VerifyError, verify_raster, verify_rendering};
use qr_core::{Color, Matrix, ModuleShape, RenderOptions};

use crate::cli::StyleArgs;
use crate::diag::Reporter;
use crate::error::CliError;
use crate::output::Renderer;
use crate::style::as_drawn;

/// What an automatic check does when the code does not read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnAutoFailure {
    /// Report an error, write nothing and exit 6, as `--verify` does.
    Fail,
    /// Warn and write the code anyway.
    Warn,
}

/// The policy for automatic checks. `rqrr` misread none of the 200 styled
/// codes in `qr-core/tests/verify_corpus.rs` (0.0%, under the 2% limit)
/// and 1 of the 200 logo codes in `qr-core/tests/logo_corpus.rs` (0.5%),
/// so an automatic check fails like an explicit one.
pub(crate) const AUTO_VERIFY: OnAutoFailure = OnAutoFailure::Fail;

/// The one change most likely to make a code that failed its check read
/// back, chosen from what is known about how it was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remedy {
    /// The colours, as drawn, are below [`qr_core::MIN_CONTRAST`]: neither
    /// error correction nor size helps.
    Contrast,
    /// A logo covers part of the symbol: a smaller one helps most.
    Logo,
    /// A raster image, which is read at its own size: bigger modules help.
    Raster {
        /// Whether `--size` set the size (so `--scale` would be ignored).
        sized: bool,
        /// Whether the modules are already square.
        square: bool,
    },
    /// Vector or text output, which is always read at
    /// [`qr_core::verify::VERIFY_SCALE`], so `--scale` cannot help.
    Other {
        /// Whether the modules are already square.
        square: bool,
    },
}

impl Remedy {
    /// The remedy for `subject`.
    pub(crate) fn for_subject(subject: &Subject<'_>) -> Self {
        let options = subject.options;
        let square = options.shape == ModuleShape::Square;
        match subject.renderer {
            // Text is checked as plain black on white.
            Renderer::Text => Self::Other { square: true },
            renderer if as_drawn(options, renderer).contrast().is_low() => Self::Contrast,
            _ if options.logo.is_some() => Self::Logo,
            Renderer::Raster(_) => Self::Raster {
                sized: options.size.is_some(),
                square,
            },
            Renderer::Vector(_) => Self::Other { square },
        }
    }

    /// The hint for this remedy.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Contrast => "use a darker --fg or a lighter --bg",
            Self::Logo => "try a smaller --logo-scale",
            Self::Raster {
                sized: false,
                square: false,
            } => "try a larger --scale or --shape square",
            Self::Raster {
                sized: false,
                square: true,
            } => "try a larger --scale",
            Self::Raster {
                sized: true,
                square: false,
            } => "try a larger --size or --shape square",
            Self::Raster {
                sized: true,
                square: true,
            } => "try a larger --size",
            Self::Other { square: false } => "try --ec H or --shape square",
            Self::Other { square: true } => "try --ec H",
        }
    }
}

/// Whether and why a code is verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Not verified.
    Off,
    /// Verified because the image is styled.
    Auto,
    /// Verified because `--verify` was given.
    Explicit,
}

impl Mode {
    /// The mode for `args`: `--verify` and `--no-verify` decide, otherwise
    /// styled images (never text) are checked.
    pub(crate) fn select(args: &StyleArgs, renderer: Renderer, styled: bool) -> Self {
        if args.verify {
            Self::Explicit
        } else if args.no_verify || renderer == Renderer::Text || !styled {
            Self::Off
        } else {
            Self::Auto
        }
    }
}

/// What is checked: the rendered output and how it was drawn.
pub(crate) struct Subject<'a> {
    /// The symbol.
    pub(crate) matrix: &'a Matrix,
    /// The renderer that drew `rendered`.
    pub(crate) renderer: Renderer,
    /// The output format's name, for messages.
    pub(crate) format: &'static str,
    /// The renderer's output.
    pub(crate) rendered: &'a [u8],
    /// The options it was drawn with (for text, the module grid is checked
    /// with plain options instead).
    pub(crate) options: &'a RenderOptions,
    /// The encoded data.
    pub(crate) data: &'a [u8],
}

/// Verifies `subject` in `mode`, reporting the result.
///
/// Returns whether the code read back (`None` if it was not checked).
///
/// # Errors
///
/// [`CliError::Verify`] if verification fails in [`Mode::Explicit`], or in
/// [`Mode::Auto`] when `on_auto` is [`OnAutoFailure::Fail`], and whenever
/// verification itself cannot run.
pub(crate) fn run(
    mode: Mode,
    on_auto: OnAutoFailure,
    subject: &Subject<'_>,
    reporter: &mut Reporter<'_>,
) -> Result<Option<bool>, CliError> {
    if mode == Mode::Off {
        return Ok(None);
    }
    match check(subject) {
        Ok(verified) => {
            reporter.note("verified", &describe(subject, verified));
            Ok(Some(true))
        }
        Err(err) if err.is_failure() && mode == Mode::Auto && on_auto == OnAutoFailure::Warn => {
            reporter.warn(
                "verification_failed",
                &format!("{err}; writing it anyway"),
                Some(&format!(
                    "check that it scans, or {}",
                    Remedy::for_subject(subject).hint()
                )),
            );
            Ok(Some(false))
        }
        Err(err) => Err(CliError::Verify {
            error: err,
            automatic: mode == Mode::Auto,
            remedy: Remedy::for_subject(subject),
        }),
    }
}

fn check(subject: &Subject<'_>) -> Result<Verified, VerifyError> {
    match subject.renderer {
        Renderer::Raster(format) => verify_raster(
            subject.matrix,
            format,
            subject.rendered,
            subject.options,
            subject.data,
        ),
        Renderer::Vector(_) => verify_rendering(
            subject.matrix,
            subject.options,
            subject.format,
            subject.data,
        ),
        // Text has no colours or shapes: check the module grid itself.
        Renderer::Text => verify_rendering(
            subject.matrix,
            &RenderOptions::default().with_margin(subject.options.margin),
            subject.format,
            subject.data,
        ),
    }
}

/// The `-v` note for a successful check of `subject`.
fn describe(subject: &Subject<'_>, verified: Verified) -> String {
    let side = verified.side;
    let how = if verified.redrawn {
        format!("drawn at {side}x{side} px")
    } else {
        format!("decoded at {side}x{side} px")
    };
    let inverted = if verified.inverted {
        ", read as light on dark"
    } else {
        ""
    };
    // A transparent image is read as if placed on its --bg colour (JPEG has
    // no transparency and text no colours); on another page it may differ.
    let backdrop = match subject.renderer {
        Renderer::Raster(format) if !format.has_alpha() => None,
        Renderer::Text => None,
        Renderer::Raster(_) | Renderer::Vector(_) => subject.options.transparent.then(|| {
            format!(
                ", transparency read on {} (--bg)",
                subject.options.bg.over(Color::WHITE)
            )
        }),
    }
    .unwrap_or_default();
    format!(
        "verified: the {} output reads back as the input ({how}{inverted}{backdrop})",
        subject.format
    )
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use qr_core::{
        Color, EncodeOptions, ModuleShape, RasterFormat, VectorFormat, encode, render_raster,
    };

    use super::*;
    use crate::cli::Cli;
    use crate::diag::Verbosity;
    use crate::exit::ExitStatus;

    const PNG: Renderer = Renderer::Raster(RasterFormat::Png);
    const SVG: Renderer = Renderer::Vector(VectorFormat::Svg);

    fn args(extra: &[&str]) -> StyleArgs {
        let mut argv = vec!["qr"];
        argv.extend_from_slice(extra);
        argv.push("x");
        Cli::try_parse_from(argv).unwrap().generate.render.style
    }

    #[test]
    fn mode_selection() {
        let table = [
            (&[][..], PNG, false, Mode::Off),
            (&[][..], PNG, true, Mode::Auto),
            (&[][..], SVG, true, Mode::Auto),
            (&[][..], Renderer::Text, true, Mode::Off),
            (&["--no-verify"][..], PNG, true, Mode::Off),
            (&["--verify"][..], PNG, false, Mode::Explicit),
            (&["--verify"][..], Renderer::Text, false, Mode::Explicit),
            (&["--no-verify", "--verify"][..], PNG, true, Mode::Explicit),
        ];
        for (flags, renderer, styled, expected) in table {
            assert_eq!(
                Mode::select(&args(flags), renderer, styled),
                expected,
                "{flags:?} {renderer:?} styled={styled}"
            );
        }
        assert_eq!(AUTO_VERIFY, OnAutoFailure::Fail);
    }

    /// Runs a check of `rendered` (as a PNG of "data") and returns the
    /// result and stderr.
    fn outcome(
        mode: Mode,
        on_auto: OnAutoFailure,
        output: &[u8],
        renderer: Renderer,
        options: &RenderOptions,
    ) -> (Result<Option<bool>, CliError>, String) {
        let code = encode(b"data", &EncodeOptions::default()).unwrap();
        let mut buf = Vec::new();
        let mut reporter = Reporter::new(&mut buf, false, Verbosity::Verbose);
        let subject = Subject {
            matrix: code.matrix(),
            renderer,
            format: "png",
            rendered: output,
            options,
            data: b"data",
        };
        let result = run(mode, on_auto, &subject, &mut reporter);
        (result, String::from_utf8(buf).unwrap())
    }

    fn png(data: &[u8], options: &RenderOptions) -> Vec<u8> {
        let code = encode(data, &EncodeOptions::default()).unwrap();
        render_raster(code.matrix(), RasterFormat::Png, options).unwrap()
    }

    #[test]
    fn a_good_code_verifies_and_says_so_with_v() {
        let options = RenderOptions::default().with_shape(ModuleShape::Dots);
        let (result, err) = outcome(
            Mode::Auto,
            OnAutoFailure::Fail,
            &png(b"data", &options),
            PNG,
            &options,
        );
        assert_eq!(result.unwrap(), Some(true));
        assert_eq!(
            err,
            "note: verified: the png output reads back as the input (decoded at 290x290 px)\n"
        );
        let (result, err) = outcome(Mode::Off, OnAutoFailure::Fail, b"", PNG, &options);
        assert_eq!(result.unwrap(), None);
        assert_eq!(err, "");

        let inverted = RenderOptions::default()
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK);
        let (result, err) = outcome(Mode::Explicit, OnAutoFailure::Fail, b"", SVG, &inverted);
        assert_eq!(result.unwrap(), Some(true));
        assert!(
            err.contains("(drawn at 290x290 px, read as light on dark)"),
            "{err}"
        );
        let (result, err) = outcome(
            Mode::Explicit,
            OnAutoFailure::Fail,
            b"",
            SVG,
            &inverted.clone().with_transparent(true),
        );
        assert_eq!(result.unwrap(), Some(true));
        assert!(
            err.contains("read as light on dark, transparency read on #000000 (--bg))"),
            "{err}"
        );
        let (result, _) = outcome(
            Mode::Explicit,
            OnAutoFailure::Fail,
            b"",
            Renderer::Text,
            &inverted,
        );
        assert_eq!(result.unwrap(), Some(true));
    }

    #[test]
    fn the_hint_follows_what_is_known_about_the_drawing() {
        let code = encode(b"data", &EncodeOptions::default()).unwrap();
        let remedy = |renderer, options: &RenderOptions| {
            Remedy::for_subject(&Subject {
                matrix: code.matrix(),
                renderer,
                format: "x",
                rendered: b"",
                options,
                data: b"data",
            })
        };
        let pale = RenderOptions::default().with_fg(Color::rgb(0xdd, 0xdd, 0xdd));
        let dots = RenderOptions::default().with_shape(ModuleShape::Dots);
        let jpeg = Renderer::Raster(RasterFormat::Jpeg);
        // A transparent background is drawn on white in a JPEG: white on
        // transparent white is invisible there.
        let ghost = RenderOptions::default()
            .with_fg(Color::WHITE)
            .with_bg(Color::BLACK)
            .with_transparent(true);
        let image = qr_core::loader::Bitmap::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
        let logo = RenderOptions::default().with_logo(Some(qr_core::logo::Logo::new(image)));
        let pale_logo = logo.clone().with_fg(Color::rgb(0xdd, 0xdd, 0xdd));
        let table = [
            (SVG, &pale, "use a darker --fg or a lighter --bg"),
            (PNG, &pale, "use a darker --fg or a lighter --bg"),
            (jpeg, &ghost, "use a darker --fg or a lighter --bg"),
            (PNG, &ghost, "try a larger --scale"),
            (SVG, &dots, "try --ec H or --shape square"),
            (SVG, &RenderOptions::default(), "try --ec H"),
            (PNG, &dots, "try a larger --scale or --shape square"),
            (
                PNG,
                &dots.clone().with_size(Some(64)),
                "try a larger --size or --shape square",
            ),
            (
                PNG,
                &RenderOptions::default().with_size(Some(64)),
                "try a larger --size",
            ),
            // Text is checked as plain black on white, whatever the colours.
            (Renderer::Text, &pale, "try --ec H"),
            (PNG, &logo, "try a smaller --logo-scale"),
            (SVG, &logo, "try a smaller --logo-scale"),
            (PNG, &pale_logo, "use a darker --fg or a lighter --bg"),
        ];
        for (renderer, options, hint) in table {
            assert_eq!(
                remedy(renderer, options).hint(),
                hint,
                "{renderer:?} {options:?}"
            );
        }
    }

    #[test]
    fn failures_follow_the_mode_and_policy() {
        let options = RenderOptions::default();
        let wrong = png(b"other data", &options);
        for (mode, automatic) in [(Mode::Explicit, false), (Mode::Auto, true)] {
            let (result, _) = outcome(mode, OnAutoFailure::Fail, &wrong, PNG, &options);
            let err = result.unwrap_err();
            assert!(
                matches!(err, CliError::Verify { automatic: a, .. } if a == automatic),
                "{err:?}"
            );
            assert_eq!(err.exit_status(), ExitStatus::VerificationFailed);
            assert_eq!(err.code(), "verification_failed");
        }
        // An explicit check fails even under the warning policy.
        let (result, _) = outcome(Mode::Explicit, OnAutoFailure::Warn, &wrong, PNG, &options);
        assert!(result.is_err());

        let (result, err) = outcome(Mode::Auto, OnAutoFailure::Warn, &wrong, PNG, &options);
        assert_eq!(result.unwrap(), Some(false));
        assert_eq!(
            err,
            "warning: the rendered png reads back as 10 bytes that differ from the 4 bytes \
             encoded; writing it anyway\n  hint: check that it scans, or try a larger \
             --scale\n"
        );

        // A broken image is a bug, whatever the policy.
        let (result, _) = outcome(Mode::Auto, OnAutoFailure::Warn, b"junk", PNG, &options);
        let err = result.unwrap_err();
        assert_eq!(err.exit_status(), ExitStatus::Internal);
        assert_eq!(err.code(), "internal");
    }
}
