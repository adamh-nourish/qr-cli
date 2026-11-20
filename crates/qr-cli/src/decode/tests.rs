//! In-process tests of `qr decode` through [`run`](crate::run).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use qr_core::decode::Point;
use qr_core::{Color, EcLevel, EncodeOptions, RenderOptions, encode, render_png};

use super::*;
use crate::input::FailingReader;
use crate::{Context, run};

struct Outcome {
    status: ExitStatus,
    out: Vec<u8>,
    err: String,
}

impl Outcome {
    fn text(&self) -> &str {
        std::str::from_utf8(&self.out).unwrap()
    }
}

fn run_with(args: &[&str], stdin: &[u8], env: Env) -> Outcome {
    let mut stdin = stdin;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut command_line = vec!["qr", "decode"];
    command_line.extend_from_slice(args);
    let status = run(
        command_line,
        &mut Context {
            stdin: &mut stdin,
            stdout: &mut out,
            stderr: &mut err,
            env,
        },
    );
    Outcome {
        status,
        out,
        err: String::from_utf8(err).unwrap(),
    }
}

fn piped(args: &[&str]) -> Outcome {
    run_with(args, b"", Env::default())
}

fn png_bytes(data: &[u8], render: &RenderOptions) -> Vec<u8> {
    let code = encode(data, &EncodeOptions::default()).unwrap();
    render_png(code.matrix(), render).unwrap()
}

/// Writes a PNG holding `data` as `name` in `dir`.
fn code_file(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, png_bytes(data, &RenderOptions::default())).unwrap();
    path
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn prints_each_codes_data_on_its_own_line() {
    let dir = tempfile::tempdir().unwrap();
    let a = code_file(dir.path(), "a.png", b"hello");
    let out = piped(&[s(&a)]);
    assert_eq!(out.status, ExitStatus::Success, "{}", out.err);
    assert_eq!(out.text(), "hello\n");
    assert_eq!(out.err, "");

    // Several images: each line is prefixed with its path.
    let b = code_file(dir.path(), "b.png", b"world");
    let out = piped(&[s(&a), s(&b)]);
    assert_eq!(out.status, ExitStatus::Success);
    assert_eq!(
        out.text(),
        format!("{}: hello\n{}: world\n", a.display(), b.display())
    );
}

#[test]
fn data_is_escaped_unless_raw() {
    let dir = tempfile::tempdir().unwrap();
    let data = b"a\x1b]0;pwned\x07\nb\xff\xfe\xe2\x80\xaez";
    let path = dir.path().join("evil.png");
    fs::write(&path, png_bytes(data, &RenderOptions::default())).unwrap();

    let out = piped(&[s(&path)]);
    assert_eq!(
        out.text(),
        "a\\u{1b}]0;pwned\\u{7}\\u{a}b\\x{ff}\\x{fe}\\u{202e}z\n"
    );
    // Even on a terminal, the escaped form is what is printed.
    let tty = Env {
        stdout_is_tty: true,
        ..Env::default()
    };
    assert_eq!(run_with(&[s(&path)], b"", tty).out, out.out);

    let raw = piped(&[s(&path), "--raw"]);
    assert_eq!(raw.status, ExitStatus::Success);
    let mut expected = data.to_vec();
    expected.push(b'\n');
    assert_eq!(raw.out, expected);

    // --raw is refused on a terminal, before anything is read.
    let refused = run_with(&[s(&path), "--raw"], b"", tty);
    assert_eq!(refused.status, ExitStatus::Usage);
    assert_eq!(refused.out, b"");
    assert_eq!(
        refused.err,
        "error: refusing to write --raw output to a terminal\n  \
         hint: redirect or pipe stdout, or drop --raw to print the data escaped\n"
    );
}

/// Checks that `qr decode` escapes `bad` (and shows it as `shown`) in
/// the path prefix of a decoded line and in the message for a missing
/// file.
fn escapes_names_with(bad: &str, shown: &str) {
    let dir = tempfile::tempdir().unwrap();
    let a = code_file(dir.path(), &format!("a{bad}b.png"), b"x");
    let missing = dir.path().join(format!("gone{bad}.png"));
    let out = piped(&[s(&a), s(&missing)]);
    assert!(!out.text().contains(bad) && !out.err.contains(bad));
    assert!(
        out.text().contains(&format!("a{shown}b.png: x\n")),
        "{}",
        out.text()
    );
    assert!(out.err.contains(&format!("gone{shown}.png")), "{}", out.err);
}

#[test]
fn file_names_are_escaped_in_prefixes_and_messages() {
    // A bidi override is a legal file name everywhere, Windows included.
    escapes_names_with("\u{202e}", "\\u{202e}");
}

#[test]
#[cfg(unix)]
fn control_characters_in_file_names_are_escaped() {
    // Windows refuses control characters in file names.
    escapes_names_with("\x1b[2J", "\\u{1b}[2J");
}

#[test]
fn json_reports_every_image_and_the_totals() {
    let dir = tempfile::tempdir().unwrap();
    let a = code_file(dir.path(), "a.png", b"hello");
    let bin = dir.path().join("bin.png");
    fs::write(&bin, png_bytes(b"\xffbin", &RenderOptions::default())).unwrap();
    let blank = dir.path().join("blank.png");
    let white = RenderOptions::default().with_fg(Color::WHITE);
    fs::write(&blank, png_bytes(b"gone", &white)).unwrap();
    let missing = dir.path().join("missing.png");

    let out = piped(&[s(&a), s(&bin), s(&blank), s(&missing), "--json"]);
    assert_eq!(out.status, ExitStatus::Io);
    let value: serde_json::Value = serde_json::from_slice(&out.out).unwrap();
    let images = value["images"].as_array().unwrap();
    assert_eq!(images.len(), 4);

    assert_eq!(images[0]["path"], s(&a));
    let first = &images[0]["payloads"][0];
    assert_eq!(first["text"], "hello");
    assert!(first.get("base64").is_none());
    assert_eq!(first["bytes"], 5);
    assert_eq!(
        (first["version"].as_u64(), first["ec"].as_str()),
        (Some(1), Some("M"))
    );
    assert!(first["mask"].as_u64().unwrap() < 8);
    assert_eq!(first["inverted"], false);
    assert_eq!(
        first["bounds"],
        serde_json::json!([[40, 40], [250, 40], [250, 250], [40, 250]])
    );
    assert!(images[0].get("error").is_none());

    let binary = &images[1]["payloads"][0];
    assert_eq!(binary["text"], "\u{fffd}bin");
    assert_eq!(binary["base64"], "/2Jpbg==");

    assert_eq!(images[2]["payloads"], serde_json::json!([]));
    assert_eq!(images[2]["error"]["code"], "no_code_found");
    assert_eq!(
        images[2]["error"]["message"],
        format!("no QR code found in '{}'", blank.display())
    );
    assert_eq!(images[3]["error"]["code"], "read_failed");
    assert_eq!(
        value["summary"],
        serde_json::json!({"images": 4, "codes": 2, "unreadable": 1, "no_code": 1})
    );

    // Errors are JSON lines on stderr: one per failed image, then the summary.
    let lines: Vec<serde_json::Value> = out
        .err
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let codes: Vec<&str> = lines
        .iter()
        .map(|l| l["error"]["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["no_code_found", "read_failed", "decode_incomplete"]);
    assert_eq!(
        lines[2]["error"]["message"],
        "2 of 4 images failed (1 unreadable, 1 without a readable code); 2 codes decoded"
    );
}

#[test]
fn per_image_failures_do_not_stop_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let a = code_file(dir.path(), "a.png", b"first");
    let text = dir.path().join("notes.png");
    fs::write(&text, "not an image").unwrap();
    let b = code_file(dir.path(), "b.png", b"last");

    let out = piped(&[s(&text), s(&a), s(dir.path()), s(&b)]);
    assert_eq!(out.status, ExitStatus::Io);
    assert_eq!(
        out.text(),
        format!("{}: first\n{}: last\n", a.display(), b.display())
    );
    let expected = format!(
        "error: cannot decode '{}': not a png, jpeg or webp image (the header matches none of them)\n  \
         hint: qr decode reads png, jpeg and webp images\n\
         error: cannot read '{}': it is a directory\n  \
         hint: name an image file, not a directory\n\
         error: 2 of 4 images failed (2 unreadable); 2 codes decoded\n",
        text.display(),
        dir.path().display()
    );
    assert_eq!(out.err, expected);
}

#[test]
fn no_code_is_exit_5_and_unreadable_wins_over_it() {
    let dir = tempfile::tempdir().unwrap();
    let blank = dir.path().join("blank.png");
    let white = RenderOptions::default().with_fg(Color::WHITE);
    fs::write(&blank, png_bytes(b"x", &white)).unwrap();
    let out = piped(&[s(&blank)]);
    assert_eq!(out.status, ExitStatus::NoCodeFound);
    assert_eq!(out.out, b"");
    // One image: no summary line.
    assert_eq!(
        out.err,
        format!(
            "error: no QR code found in '{}'\n  \
             hint: crop the image closer to the code and check it is sharp and not cut off\n",
            blank.display()
        )
    );
    let a = code_file(dir.path(), "a.png", b"ok");
    let out = piped(&[s(&a), s(&blank)]);
    assert_eq!(out.status, ExitStatus::NoCodeFound);
    assert!(
        out.err
            .ends_with("error: 1 of 2 images failed (1 without a readable code); 1 code decoded\n")
    );
    let out = piped(&[s(&blank), s(&dir.path().join("missing.png"))]);
    assert_eq!(out.status, ExitStatus::Io);
}

#[test]
fn stdin_is_read_once_and_never_from_a_terminal() {
    let png = png_bytes(b"from stdin", &RenderOptions::default());
    let out = run_with(&["-"], &png, Env::default());
    assert_eq!(out.status, ExitStatus::Success, "{}", out.err);
    assert_eq!(out.text(), "from stdin\n");

    let out = run_with(&["-", "--json"], &png, Env::default());
    let value: serde_json::Value = serde_json::from_slice(&out.out).unwrap();
    assert_eq!(value["images"][0]["path"], "-");

    let out = run_with(&["-", "x.png", "-"], &png, Env::default());
    assert_eq!(out.status, ExitStatus::Usage);
    assert!(
        out.err
            .starts_with("error: stdin ('-') can be read only once\n")
    );

    let tty = Env {
        stdin_is_tty: true,
        ..Env::default()
    };
    let out = run_with(&["-"], &png, tty);
    assert_eq!(out.status, ExitStatus::Usage);
    assert_eq!(
        out.err,
        "error: no image: stdin is a terminal\n  \
         hint: pipe an image in, for example: qr decode - < code.png\n"
    );
    // A file can still be read with stdin on a terminal.
    let dir = tempfile::tempdir().unwrap();
    let a = code_file(dir.path(), "a.png", b"file");
    assert_eq!(run_with(&[s(&a)], b"", tty).status, ExitStatus::Success);

    let out = run_with(&["-"], b"", Env::default());
    assert_eq!(out.status, ExitStatus::Io);
    assert!(
        out.err
            .contains("cannot decode stdin: the image file is empty")
    );
}

#[test]
fn a_failing_stdin_is_a_read_error() {
    let args = DecodeArgs {
        images: vec![PathBuf::from("-")],
        json: false,
        raw: false,
    };
    let err = read(Path::new("-"), &InputSource::Stdin, &mut FailingReader).unwrap_err();
    assert!(matches!(err, CliError::ReadImage { .. }), "{err:?}");
    assert_eq!(err.code(), "read_failed");
    assert_eq!(err.exit_status(), ExitStatus::Io);
    assert_eq!(err.to_string(), "cannot read stdin: device on fire");
    assert!(check(&args, &Env::default()).is_ok());
}

#[test]
fn a_broken_stdout_is_an_output_error() {
    struct BrokenPipe;
    impl Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let a = code_file(dir.path(), "a.png", b"x");
    for extra in [None, Some("--json")] {
        let mut args = vec!["qr", "decode", s(&a)];
        args.extend(extra);
        let mut err = Vec::new();
        let status = run(
            args,
            &mut Context {
                stdin: &mut io::empty(),
                stdout: &mut BrokenPipe,
                stderr: &mut err,
                env: Env::default(),
            },
        );
        assert_eq!(status, ExitStatus::Io, "{extra:?}");
        assert!(
            String::from_utf8(err)
                .unwrap()
                .contains("the pipe was closed")
        );
    }
}

#[test]
fn verbose_notes_describe_each_code() {
    let dir = tempfile::tempdir().unwrap();
    let inverse = RenderOptions::default()
        .with_fg(Color::WHITE)
        .with_bg(Color::BLACK);
    let path = dir.path().join("inv.png");
    fs::write(&path, png_bytes(b"night", &inverse)).unwrap();
    let out = piped(&[s(&path), "-v"]);
    assert_eq!(out.text(), "night\n");
    assert!(
        out.err.starts_with(&format!(
            "note: '{}': version 1, error correction M, mask ",
            path.display()
        )),
        "{}",
        out.err
    );
    assert!(
        out.err
            .ends_with(", 5 bytes, light on dark, at (40, 40)-(250, 250)\n"),
        "{}",
        out.err
    );
    assert_eq!(piped(&[s(&path), "-q"]).err, "");
}

#[test]
fn undecodable_codes_get_their_own_message() {
    let origin = InputSource::File("a.png".into());
    let one = CliError::NoCodeFound {
        origin: origin.clone(),
        undecodable: 1,
    };
    assert_eq!(one.to_string(), "a QR code in 'a.png' could not be decoded");
    assert_eq!(one.code(), "undecodable");
    assert_eq!(one.exit_status(), ExitStatus::NoCodeFound);
    assert_eq!(
        one.hint().as_deref(),
        Some("try a sharper, larger or straighter picture of the code")
    );
    let two = CliError::NoCodeFound {
        origin,
        undecodable: 2,
    };
    assert_eq!(
        two.to_string(),
        "2 QR codes in 'a.png' could not be decoded"
    );
}

#[test]
fn oversized_images_have_their_own_hint() {
    let err = CliError::Image {
        origin: InputSource::Stdin,
        error: LoadError::DimensionsTooLarge {
            dimensions: Some((9_000, 10)),
            max: 8_192,
        },
    };
    assert_eq!(err.code(), "invalid_image");
    assert_eq!(err.exit_status(), ExitStatus::Io);
    assert_eq!(
        err.to_string(),
        "cannot decode stdin: the image is 9000x10 pixels and exceeds the 8192x8192 pixel limit"
    );
    assert_eq!(
        err.hint().as_deref(),
        Some("scale the image down (a code needs only a few pixels per module)")
    );
    let read = CliError::Image {
        origin: InputSource::Stdin,
        error: LoadError::Read(io::Error::other("x")),
    };
    assert_eq!(read.code(), "read_failed");
    assert_eq!(read.hint(), None);
}

#[test]
fn payloads_escape_invalid_utf8_byte_by_byte() {
    assert_eq!(escape_payload(b""), "");
    assert_eq!(escape_payload(b"plain"), "plain");
    assert_eq!(escape_payload(b"\xc3"), "\\x{c3}");
    assert_eq!(escape_payload("é\u{9b}".as_bytes()), "é\\u{9b}");
    assert_eq!(escape_payload(b"a\x80\x80b\r"), "a\\x{80}\\x{80}b\\u{d}");
}

#[test]
fn json_entries_carry_the_metadata() {
    let code = DecodedCode {
        payload: b"\xc3(".to_vec(),
        version: qr_core::Version::new(7).unwrap(),
        ec_level: EcLevel::Q,
        mask: qr_core::Mask::new(5).unwrap(),
        bounds: [Point { x: -1, y: 2 }; 4],
        inverted: true,
    };
    let entry = payload_entry(&code);
    assert_eq!(entry.text, "\u{fffd}(");
    assert_eq!(entry.base64.as_deref(), Some("wyg="));
    assert_eq!((entry.bytes, entry.version, entry.mask), (2, 7, 5));
    assert_eq!(entry.ec, "Q");
    assert!(entry.inverted);
    assert_eq!(entry.bounds, [[-1, 2]; 4]);
}

#[test]
fn the_status_and_summary_reflect_every_image() {
    let summary = |unreadable, no_code| DecodeSummary {
        images: 3,
        codes: 1,
        unreadable,
        no_code,
    };
    assert_eq!(status(&summary(0, 0)), ExitStatus::Success);
    assert_eq!(status(&summary(0, 2)), ExitStatus::NoCodeFound);
    assert_eq!(status(&summary(1, 1)), ExitStatus::Io);
    assert_eq!(
        summarise(&summary(1, 1)),
        "2 of 3 images failed (1 unreadable, 1 without a readable code); 1 code decoded"
    );
    assert_eq!(
        summarise(&DecodeSummary {
            codes: 0,
            ..summary(3, 0)
        }),
        "3 of 3 images failed (3 unreadable); 0 codes decoded"
    );
}

#[test]
fn json_mode_writes_nothing_per_code() {
    let mut out = Vec::new();
    let scan = Scan {
        codes: vec![DecodedCode {
            payload: b"x".to_vec(),
            version: qr_core::Version::new(1).unwrap(),
            ec_level: EcLevel::L,
            mask: qr_core::Mask::new(0).unwrap(),
            bounds: [Point::default(); 4],
            inverted: false,
        }],
        ..Scan::default()
    };
    Mode::Json.write(Path::new("a"), &scan, &mut out).unwrap();
    assert_eq!(out, b"");
    Mode::Raw.write(Path::new("a"), &scan, &mut out).unwrap();
    assert_eq!(out, b"x\n");
}
