//! Unit tests for output planning and delivery.

use std::fs;
use std::io;

use clap::Parser as _;

use super::*;
use crate::cli::Cli;
use crate::diag::Verbosity;
use crate::exit::ExitStatus;

fn args(argv: &[&str]) -> GenerateArgs {
    let mut full = vec!["qr"];
    full.extend_from_slice(argv);
    Cli::try_parse_from(full).unwrap().generate
}

/// Resolves a plan and returns it with whatever was reported on stderr.
fn resolve(argv: &[&str], tty: bool) -> (Result<Plan, CliError>, String) {
    let mut err = Vec::new();
    let plan = {
        let mut reporter = Reporter::new(&mut err, false, Verbosity::Normal);
        Plan::resolve(&args(argv), tty, &mut reporter)
    };
    (plan, String::from_utf8(err).unwrap())
}

fn plan(argv: &[&str], tty: bool) -> Plan {
    let (plan, _) = resolve(argv, tty);
    plan.unwrap()
}

fn error(argv: &[&str], tty: bool) -> CliError {
    let (plan, _) = resolve(argv, tty);
    plan.unwrap_err()
}

#[test]
fn targets_come_from_dash_o() {
    assert_eq!(Target::from_arg(None), Target::Stdout);
    assert_eq!(Target::from_arg(Some(Path::new("-"))), Target::Stdout);
    assert_eq!(
        Target::from_arg(Some(Path::new("./-"))),
        Target::File("./-".into())
    );
}

#[test]
fn format_resolution_order() {
    use OutputFormat as F;
    // (args, stdout is a terminal, expected format)
    let table: [(&[&str], bool, F); 9] = [
        (&["x"], true, F::Terminal),
        (&["x"], false, F::Text),
        (&["-o", "-", "x"], false, F::Text),
        (&["-o", "-", "x"], true, F::Terminal),
        (&["-f", "eps", "x"], true, F::Eps),
        (&["-o", "a.pdf", "x"], true, F::Pdf),
        (&["-o", "a.txt", "-f", "ascii", "x"], false, F::Ascii),
        (&["--data-uri", "x"], true, F::Svg),
        (&["--data-uri", "-f", "pdf", "x"], true, F::Pdf),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (argv, tty, expected) in table {
        // Keep file targets inside a scratch directory.
        let argv: Vec<String> = argv
            .iter()
            .map(|a| {
                if a.contains('.') {
                    dir.path().join(a).to_string_lossy().into_owned()
                } else {
                    (*a).to_owned()
                }
            })
            .collect();
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        assert_eq!(plan(&argv, tty).format, expected, "{argv:?}");
    }
}

#[test]
fn unknown_or_missing_extensions_are_usage_errors() {
    for path in ["out.gif", "out", "out.svg.bak"] {
        let err = error(&["-o", path, "x"], false);
        assert!(matches!(&err, CliError::UnknownExtension(p) if p == Path::new(path)));
        assert_eq!(err.exit_status(), ExitStatus::Usage);
        assert_eq!(
            err.to_string(),
            format!("cannot infer the output format from '{path}'")
        );
        assert!(err.hint().unwrap().contains(".svg"), "{:?}", err.hint());
    }
}

#[test]
fn raster_formats_are_not_supported_yet() {
    for argv in [
        &["-f", "png", "x"][..],
        &["-f", "jpg", "x"],
        &["-f", "webp", "-o", "-", "x"],
        &["-o", "a.jpeg", "x"],
        &["-o", "a.PNG", "x"],
    ] {
        let err = error(argv, false);
        assert!(matches!(err, CliError::UnsupportedFormat(_)), "{argv:?}");
        assert_eq!(err.exit_status(), ExitStatus::Usage);
        assert!(err.to_string().ends_with("output is not supported yet"));
    }
}

#[test]
fn binary_output_is_refused_on_a_terminal_only() {
    let err = error(&["-f", "pdf", "x"], true);
    assert!(matches!(err, CliError::BinaryToTerminal(OutputFormat::Pdf)));
    assert_eq!(
        err.to_string(),
        "refusing to write binary pdf output to a terminal"
    );
    assert_eq!(
        err.hint().unwrap(),
        "redirect stdout to a file or use -o code.pdf"
    );
    assert!(matches!(
        error(&["-f", "pdf", "-o", "-", "x"], true),
        CliError::BinaryToTerminal(_)
    ));
    // Piped stdout, a data: URI (which is text) and text formats are fine.
    assert_eq!(plan(&["-f", "pdf", "x"], false).format, OutputFormat::Pdf);
    assert!(plan(&["-f", "pdf", "--data-uri", "x"], true).data_uri);
    assert_eq!(plan(&["-f", "svg", "x"], true).format, OutputFormat::Svg);
    assert_eq!(plan(&["-f", "eps", "x"], true).format, OutputFormat::Eps);
}

#[test]
fn data_uri_needs_an_image_format() {
    let err = error(&["--data-uri", "-f", "ascii", "x"], false);
    assert!(matches!(
        err,
        CliError::DataUriNeedsImage(OutputFormat::Ascii)
    ));
    assert_eq!(
        err.to_string(),
        "--data-uri needs an image format, not ascii"
    );
    assert_eq!(err.exit_status(), ExitStatus::Usage);
}

#[test]
fn json_cannot_share_stdout_with_the_code() {
    let err = error(&["--json", "x"], false);
    assert!(matches!(err, CliError::JsonNeedsOutput(OutputFormat::Text)));
    assert_eq!(err.code(), "json_needs_output");
    assert!(err.hint().unwrap().contains("--data-uri"));
    assert!(plan(&["--json", "--data-uri", "x"], false).json);
}

#[test]
fn file_targets_are_prechecked() {
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("a.svg");
    fs::write(&existing, "old").unwrap();
    let existing = existing.to_str().unwrap();

    let err = error(&["-o", existing, "--no-clobber", "x"], false);
    assert!(matches!(err, CliError::OutputExists(_)));
    assert_eq!(err.exit_status(), ExitStatus::Io);
    assert_eq!(err.code(), "output_exists");
    assert_ne!(plan(&["-o", existing, "x"], false).target, Target::Stdout);

    let as_dir = dir.path().join("d.svg");
    fs::create_dir(&as_dir).unwrap();
    let err = error(&["-o", as_dir.to_str().unwrap(), "x"], false);
    assert!(err.to_string().ends_with("': it is a directory"), "{err}");
    assert_eq!(err.exit_status(), ExitStatus::Io);
}

#[test]
fn a_format_that_contradicts_the_extension_is_used_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.pdf");
    let path = path.to_str().unwrap();
    let (plan, err) = resolve(&["-f", "svg", "-o", path, "x"], false);
    assert_eq!(plan.unwrap().format, OutputFormat::Svg);
    assert!(
        err.starts_with("warning: writing svg to '") && err.contains("suggests pdf"),
        "{err}"
    );

    // Matching, extension-less, text-to-.txt and data: URI cases are quiet.
    let txt = dir.path().join("code.txt");
    let bare = dir.path().join("code");
    for argv in [
        vec!["-f", "pdf", "-o", path, "x"],
        vec!["-f", "ascii", "-o", txt.to_str().unwrap(), "x"],
        vec!["-f", "svg", "-o", bare.to_str().unwrap(), "x"],
        vec!["-f", "svg", "--data-uri", "-o", path, "x"],
    ] {
        let (plan, err) = resolve(&argv, false);
        assert!(plan.is_ok(), "{argv:?}");
        assert_eq!(err, "", "{argv:?}");
    }
}

/// A writer that fails every operation.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn delivery_to_stdout_and_files() {
    let mut out = Vec::new();
    let raw = plan(&["-f", "svg", "x"], false);
    let d = raw.deliver(b"<svg/>".to_vec(), &mut out).unwrap();
    assert_eq!(
        (out.as_slice(), d.written, d.data_uri),
        (&b"<svg/>"[..], 6, None)
    );

    let mut out = Vec::new();
    let uri = plan(&["--data-uri", "x"], false);
    let d = uri.deliver(b"<svg/>".to_vec(), &mut out).unwrap();
    assert_eq!(out, b"data:image/svg+xml;base64,PHN2Zy8+\n");
    assert_eq!((d.written, d.data_uri), (35, None));

    let mut out = Vec::new();
    let json = plan(&["--data-uri", "--json", "x"], false);
    let d = json.deliver(b"<svg/>".to_vec(), &mut out).unwrap();
    assert_eq!(out, b"");
    assert_eq!(
        d.data_uri.as_deref(),
        Some("data:image/svg+xml;base64,PHN2Zy8+")
    );
    assert_eq!(d.written, 34);

    let err = raw.deliver(b"x".to_vec(), &mut Broken).unwrap_err();
    assert!(matches!(err, CliError::Output(_)));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.eps");
    let file = plan(&["-o", path.to_str().unwrap(), "--json", "x"], false);
    let mut out = Vec::new();
    let d = file.deliver(b"%!PS".to_vec(), &mut out).unwrap();
    assert_eq!((d.written, out.len()), (4, 0));
    assert_eq!(fs::read(&path).unwrap(), b"%!PS");
}

#[test]
fn write_errors_convert_to_cli_errors() {
    let exists: CliError = WriteError::Exists("a".into()).into();
    assert_eq!(exists.to_string(), "output file 'a' already exists");
    assert_eq!(
        exists.hint().unwrap(),
        "remove it, choose another path or drop --no-clobber"
    );
    let missing: CliError = WriteError::Io {
        path: "d/a".into(),
        error: io::ErrorKind::NotFound.into(),
    }
    .into();
    assert_eq!(
        missing.to_string(),
        "cannot write 'd/a': no such file or directory"
    );
    assert_eq!(missing.hint().unwrap(), "check that the directory exists");
    assert_eq!(missing.exit_status(), ExitStatus::Io);
    let denied: CliError = WriteError::Io {
        path: "a".into(),
        error: io::ErrorKind::PermissionDenied.into(),
    }
    .into();
    assert_eq!(denied.hint(), None);
    assert_eq!(denied.code(), "write_failed");
}
