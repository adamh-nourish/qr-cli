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
use qr_core::{Matrix, RenderOptions};

use crate::cli::GenerateArgs;
use crate::diag::Reporter;
use crate::error::CliError;
use crate::output::Renderer;

/// What an automatic check does when the code does not read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnAutoFailure {
    /// Report an error, write nothing and exit 6, as `--verify` does.
    Fail,
    /// Warn and write the code anyway.
    Warn,
}

/// The policy for automatic checks. `rqrr` misread none of the 200 styled
/// codes in `qr-core/tests/verify_corpus.rs` (0.0%, under the 2% limit),
/// so an automatic check fails like an explicit one.
pub(crate) const AUTO_VERIFY: OnAutoFailure = OnAutoFailure::Fail;

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
    pub(crate) fn select(args: &GenerateArgs, renderer: Renderer, styled: bool) -> Self {
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
            reporter.note("verified", &describe(subject.format, verified));
            Ok(Some(true))
        }
        Err(err) if err.is_failure() && mode == Mode::Auto && on_auto == OnAutoFailure::Warn => {
            reporter.warn(
                "verification_failed",
                &format!("{err}; writing it anyway"),
                Some("check that it scans, or try --ec H or --shape square"),
            );
            Ok(Some(false))
        }
        Err(err) => Err(CliError::Verify {
            error: err,
            automatic: mode == Mode::Auto,
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

/// The `-v` note for a successful check.
fn describe(format: &str, verified: Verified) -> String {
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
    format!("verified: the {format} output reads back as the input ({how}{inverted})")
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

    fn args(extra: &[&str]) -> GenerateArgs {
        let mut argv = vec!["qr"];
        argv.extend_from_slice(extra);
        argv.push("x");
        Cli::try_parse_from(argv).unwrap().generate
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
             encoded; writing it anyway\n  hint: check that it scans, or try --ec H or \
             --shape square\n"
        );

        // A broken image is a bug, whatever the policy.
        let (result, _) = outcome(Mode::Auto, OnAutoFailure::Warn, b"junk", PNG, &options);
        let err = result.unwrap_err();
        assert_eq!(err.exit_status(), ExitStatus::Internal);
        assert_eq!(err.code(), "internal");
    }
}
