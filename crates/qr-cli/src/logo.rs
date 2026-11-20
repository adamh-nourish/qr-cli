//! `--logo` and `--logo-scale`: reading the logo through the bounded
//! loader, the formats that can carry it and the error correction it
//! needs.

use qr_core::EcLevel;
use qr_core::loader::load_image;
use qr_core::logo::Logo;

use crate::cli::{OutputFormat, RenderArgs};
use crate::diag::Reporter;
use crate::error::CliError;
use crate::input::open_regular_file;
use crate::output::Renderer;

/// The error correction level a logo code is encoded at. Clearing the
/// modules under the logo leans on error correction, so it is H.
pub(crate) const LOGO_EC: EcLevel = EcLevel::H;

/// Checks that `format` can carry a logo: raster images and SVG can; PDF,
/// EPS (deferred to v1.x) and the text formats cannot.
///
/// # Errors
///
/// [`CliError::LogoNeedsImage`] for any other format.
pub(crate) fn check_format(args: &RenderArgs, format: OutputFormat) -> Result<(), CliError> {
    let carries = match format.renderer() {
        Renderer::Raster(_) => true,
        Renderer::Vector(vector) => vector == qr_core::VectorFormat::Svg,
        Renderer::Text => false,
    };
    if args.logo.is_some() && !carries {
        return Err(CliError::LogoNeedsImage(format));
    }
    Ok(())
}

/// The error correction level to encode at: `--ec` (default M), or H with
/// a logo. A note (`-v`) says so when `--ec` was not given; a lower `--ec`
/// is overridden with a warning, since the logo would otherwise leave too
/// little redundancy to read.
pub(crate) fn ec_level(args: &RenderArgs, reporter: &mut Reporter<'_>) -> EcLevel {
    let asked = args.ec_level;
    if args.logo.is_none() {
        return asked.unwrap_or(EcLevel::M);
    }
    match asked {
        None => reporter.note(
            "logo_ec",
            &format!(
                "--logo sets error correction to {LOGO_EC} so the covered modules can be recovered"
            ),
        ),
        Some(level) if level != LOGO_EC => reporter.warn(
            "logo_ec_overridden",
            &format!(
                "--logo needs error correction {LOGO_EC}; using {LOGO_EC} instead of --ec {level}"
            ),
            Some("drop --ec, or remove --logo to keep a lower level"),
        ),
        Some(_) => {}
    }
    LOGO_EC
}

/// Reads the `--logo` image, if any, at `--logo-scale`.
///
/// Only regular files are read (never a FIFO or device that could block
/// or never end), and the size is checked before reading: everything else
/// is the bounded loader's job.
///
/// # Errors
///
/// [`CliError::ReadLogo`] if the file cannot be opened or is not a regular
/// file; [`CliError::Logo`] if it is not an acceptable image.
pub(crate) fn load(args: &RenderArgs) -> Result<Option<Logo>, CliError> {
    let Some(path) = args.logo.as_deref() else {
        return Ok(None);
    };
    let read_error = |error| CliError::ReadLogo {
        path: path.to_owned(),
        error,
    };
    let (file, len) = open_regular_file(path).map_err(read_error)?;
    let image = load_image(file, Some(len)).map_err(|error| CliError::Logo {
        path: path.to_owned(),
        error,
    })?;
    let logo = Logo::new(image);
    Ok(Some(match args.logo_scale {
        Some(scale) => logo.with_scale(scale),
        None => logo,
    }))
}

#[cfg(test)]
mod tests {
    use std::{fs, io};

    use clap::Parser as _;
    use qr_core::logo::LogoScale;

    use super::*;
    use crate::cli::Cli;
    use crate::diag::Verbosity;

    fn args(extra: &[&str]) -> RenderArgs {
        let mut argv = vec!["qr"];
        argv.extend_from_slice(extra);
        argv.push("x");
        Cli::try_parse_from(argv).unwrap().generate.render
    }

    fn ec(extra: &[&str]) -> (EcLevel, String) {
        let mut buf = Vec::new();
        let mut reporter = Reporter::new(&mut buf, false, Verbosity::Verbose);
        let level = ec_level(&args(extra), &mut reporter);
        (level, String::from_utf8(buf).unwrap())
    }

    #[test]
    fn a_logo_forces_ec_h() {
        assert_eq!(ec(&[]), (EcLevel::M, String::new()));
        assert_eq!(ec(&["--ec", "q"]), (EcLevel::Q, String::new()));
        let (level, out) = ec(&["--logo", "l.png"]);
        assert_eq!(level, EcLevel::H);
        assert_eq!(
            out,
            "note: --logo sets error correction to H so the covered modules can be recovered\n"
        );
        let (level, out) = ec(&["--logo", "l.png", "--ec", "L"]);
        assert_eq!(level, EcLevel::H);
        assert_eq!(
            out,
            "warning: --logo needs error correction H; using H instead of --ec L\n  \
             hint: drop --ec, or remove --logo to keep a lower level\n"
        );
        assert_eq!(
            ec(&["--logo", "l.png", "--ec", "h"]),
            (EcLevel::H, String::new())
        );
    }

    #[test]
    fn only_raster_and_svg_carry_a_logo() {
        let with = args(&["--logo", "l.png"]);
        let without = args(&[]);
        for format in [
            OutputFormat::Png,
            OutputFormat::Jpeg,
            OutputFormat::Webp,
            OutputFormat::Svg,
        ] {
            assert!(check_format(&with, format).is_ok(), "{format:?}");
        }
        for format in [
            OutputFormat::Pdf,
            OutputFormat::Eps,
            OutputFormat::Text,
            OutputFormat::Ascii,
            OutputFormat::Terminal,
        ] {
            let err = check_format(&with, format).unwrap_err();
            assert!(matches!(err, CliError::LogoNeedsImage(f) if f == format));
            assert!(check_format(&without, format).is_ok());
        }
    }

    #[test]
    fn loads_regular_files_only() {
        assert!(load(&args(&[])).unwrap().is_none());
        let dir = tempfile::tempdir().unwrap();
        let err = load(&args(&["--logo", dir.path().to_str().unwrap()])).unwrap_err();
        assert!(
            matches!(&err, CliError::ReadLogo { error, .. } if error.kind() == io::ErrorKind::IsADirectory)
        );
        let missing = dir.path().join("missing.png");
        let err = load(&args(&["--logo", missing.to_str().unwrap()])).unwrap_err();
        assert!(
            matches!(&err, CliError::ReadLogo { error, .. } if error.kind() == io::ErrorKind::NotFound)
        );

        let png = dir.path().join("logo.png");
        let code = qr_core::encode(b"l", &qr_core::EncodeOptions::default()).unwrap();
        let bytes = qr_core::render_png(code.matrix(), &qr_core::RenderOptions::default()).unwrap();
        fs::write(&png, bytes).unwrap();
        let path = png.to_str().unwrap();
        let logo = load(&args(&["--logo", path])).unwrap().unwrap();
        assert_eq!(
            (logo.image().width(), logo.scale()),
            (290, LogoScale::DEFAULT)
        );
        let logo = load(&args(&["--logo", path, "--logo-scale", "0.05"]))
            .unwrap()
            .unwrap();
        assert_eq!(logo.scale(), LogoScale::MIN);

        let text = dir.path().join("logo.txt");
        fs::write(&text, "not an image").unwrap();
        let err = load(&args(&["--logo", text.to_str().unwrap()])).unwrap_err();
        assert!(matches!(&err, CliError::Logo { .. }), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn devices_are_not_read() {
        let err = load(&args(&["--logo", "/dev/zero"])).unwrap_err();
        assert!(
            matches!(&err, CliError::ReadLogo { error, .. } if error.to_string() == "not a regular file"),
            "{err:?}"
        );
    }
}
