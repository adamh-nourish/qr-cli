//! In-process tests of [`run`](crate::run) with a simulated environment.

use std::io::{self, Write};

use super::{Context, Env, ExitStatus, run};

/// A writer that fails every operation, standing in for a closed pipe.
struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }
}

/// The environment of a pipeline: no terminals, no colour variables.
fn piped() -> Env {
    Env::default()
}

/// An interactive terminal 80 columns wide.
fn terminal() -> Env {
    Env {
        stdin_is_tty: true,
        stdout_is_tty: true,
        stderr_is_tty: true,
        terminal_width: Some(80),
        ..Env::default()
    }
}

struct Outcome {
    status: ExitStatus,
    out: String,
    err: String,
}

fn run_with(args: &[&str], stdin: &[u8], env: Env) -> Outcome {
    let mut stdin = stdin;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let status = run(
        args.iter().copied(),
        &mut Context {
            stdin: &mut stdin,
            stdout: &mut out,
            stderr: &mut err,
            env,
        },
    );
    Outcome {
        status,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn run_piped(args: &[&str]) -> Outcome {
    run_with(args, b"", piped())
}

/// Reads a Unicode half-block rendering back into a grid of modules.
fn parse_half_blocks(text: &str) -> Vec<Vec<bool>> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line
            .strip_prefix("\x1b[30;107m")
            .and_then(|l| l.strip_suffix("\x1b[0m"))
            .unwrap_or(line);
        let cells: Vec<(bool, bool)> = line
            .chars()
            .map(|c| match c {
                '█' => (true, true),
                '▀' => (true, false),
                '▄' => (false, true),
                ' ' => (false, false),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        rows.push(cells.iter().map(|c| c.0).collect());
        rows.push(cells.iter().map(|c| c.1).collect());
    }
    rows
}

/// Decodes a grid of modules (quiet zone included) with `rqrr`.
fn decode_grid(grid: &[Vec<bool>]) -> String {
    let scale = 4;
    let w = grid[0].len() * scale;
    let h = grid.len() * scale;
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        if grid[y / scale][x / scale] { 0 } else { 255 }
    });
    let grids = img.detect_grids();
    assert_eq!(grids.len(), 1, "expected one code");
    grids[0].decode().unwrap().1
}

#[test]
fn version_flags_print_name_and_version() {
    let expected = format!("qr {}\n", env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        let o = run_piped(&["qr", flag]);
        assert_eq!(o.status, ExitStatus::Success);
        assert_eq!(o.out, expected);
        assert_eq!(o.err, "");
    }
}

#[test]
fn help_goes_to_stdout_and_lists_examples_and_exit_codes() {
    let o = run_piped(&["qr", "--help"]);
    assert_eq!(o.status, ExitStatus::Success);
    assert!(o.out.contains("Usage: qr [OPTIONS] [TEXT]"), "{}", o.out);
    assert!(o.out.contains("Exit codes:"), "{}", o.out);
    assert_eq!(o.err, "");
}

#[test]
fn no_input_on_a_terminal_is_a_usage_error_not_a_hang() {
    let o = run_with(&["qr"], b"never read", terminal());
    assert_eq!(o.status, ExitStatus::Usage);
    assert_eq!(o.out, "");
    assert!(o.err.contains("no input: stdin is a terminal"), "{}", o.err);
    assert!(o.err.contains("hint\x1b[0m: pass TEXT"), "{}", o.err);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let o = run_piped(&["qr", "--definitely-not-a-flag"]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert_eq!(o.out, "");
    assert!(o.err.contains("--definitely-not-a-flag"), "{}", o.err);
}

#[test]
fn clap_errors_do_not_echo_escape_sequences() {
    let o = run_piped(&["qr", "--ec", "\x1b]0;pwned\x07"]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert!(!o.err.contains('\x1b'), "{:?}", o.err);
    assert!(o.err.contains("\\u{1b}]0;pwned\\u{7}"), "{:?}", o.err);
}

#[test]
fn unwritable_stdout_is_an_io_error() {
    let mut err = Vec::new();
    let status = run(
        ["qr", "--version"],
        &mut Context {
            stdin: &mut io::empty(),
            stdout: &mut BrokenPipe,
            stderr: &mut err,
            env: piped(),
        },
    );
    assert_eq!(status, ExitStatus::Io);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "error: cannot write output: the pipe was closed\n"
    );

    for args in [
        &["qr", "hello"][..],
        &["qr", "man"],
        &["qr", "completions", "bash"],
    ] {
        let mut err = Vec::new();
        let status = run(
            args.iter().copied(),
            &mut Context {
                stdin: &mut io::empty(),
                stdout: &mut BrokenPipe,
                stderr: &mut err,
                env: piped(),
            },
        );
        assert_eq!(status, ExitStatus::Io, "{args:?}");
        let err = String::from_utf8(err).unwrap();
        assert!(err.starts_with("error: cannot write output"), "{err}");
    }
}

#[test]
fn unwritable_stderr_still_reports_usage_status() {
    let mut out = Vec::new();
    let status = run(
        ["qr", "--nope"],
        &mut Context {
            stdin: &mut io::empty(),
            stdout: &mut out,
            stderr: &mut BrokenPipe,
            env: piped(),
        },
    );
    assert_eq!(status, ExitStatus::Usage);
    assert_eq!(out, b"");
}

#[test]
fn core_and_cli_versions_move_in_lockstep() {
    assert_eq!(qr_core::VERSION, env!("CARGO_PKG_VERSION"));
}

#[test]
fn cli_default_margin_matches_the_core_quiet_zone() {
    let cli = <crate::Cli as clap::Parser>::try_parse_from(["qr", "x"]).unwrap();
    assert_eq!(
        usize::from(cli.generate.margin),
        qr_core::render::DEFAULT_MARGIN
    );
    assert_eq!(
        usize::from(crate::cli::MAX_MARGIN),
        qr_core::render::MAX_MARGIN
    );
}

#[test]
fn piped_stdout_defaults_to_plain_text_that_decodes() {
    let o = run_piped(&["qr", "https://example.com"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(o.err, "");
    assert!(!o.out.contains('\x1b'));
    assert_eq!(
        decode_grid(&parse_half_blocks(&o.out)),
        "https://example.com"
    );
}

#[test]
fn terminal_stdout_defaults_to_ansi_black_on_white() {
    let o = run_with(&["qr", "hello"], b"", terminal());
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    for line in o.out.lines() {
        assert!(
            line.starts_with("\x1b[30;107m") && line.ends_with("\x1b[0m"),
            "{line:?}"
        );
    }
    assert_eq!(decode_grid(&parse_half_blocks(&o.out)), "hello");
}

#[test]
fn no_color_falls_back_to_plain_half_blocks() {
    for (env, args) in [
        (
            Env {
                no_color: true,
                ..terminal()
            },
            &["qr", "hello"][..],
        ),
        (terminal(), &["qr", "--color", "never", "hello"]),
    ] {
        let o = run_with(args, b"", env);
        assert_eq!(o.status, ExitStatus::Success);
        assert!(!o.out.contains('\x1b'), "{args:?}");
        assert_eq!(decode_grid(&parse_half_blocks(&o.out)), "hello");
    }
}

#[test]
fn clicolor_force_and_color_always_colour_a_pipe() {
    let forced = Env {
        clicolor_force: true,
        ..piped()
    };
    let o = run_with(&["qr", "-f", "terminal", "x"], b"", forced);
    assert!(o.out.starts_with("\x1b[30;107m"));
    let o = run_piped(&["qr", "-f", "terminal", "--color", "always", "x"]);
    assert!(o.out.starts_with("\x1b[30;107m"));
    let o = run_piped(&["qr", "-f", "terminal", "x"]);
    assert!(!o.out.contains('\x1b'));
}

#[test]
fn explicit_formats_override_the_terminal_default() {
    let o = run_with(&["qr", "-f", "text", "x"], b"", terminal());
    assert!(!o.out.contains('\x1b'));
    let o = run_with(&["qr", "-f", "ascii", "x"], b"", terminal());
    assert!(o.out.is_ascii());
    assert!(o.out.contains("##"));
    assert_eq!(o.out.lines().count(), 29);
}

#[test]
fn stdin_is_read_and_one_trailing_newline_dropped() {
    let o = run_with(&["qr", "-v"], b"from stdin\n", piped());
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(decode_grid(&parse_half_blocks(&o.out)), "from stdin");
    assert!(o.err.contains("encoded 10 bytes from stdin"), "{}", o.err);

    let o = run_with(
        &["qr", "-", "--keep-newline", "-v"],
        b"from stdin\n",
        piped(),
    );
    assert_eq!(decode_grid(&parse_half_blocks(&o.out)), "from stdin\n");
    assert!(o.err.contains("encoded 11 bytes from stdin"), "{}", o.err);
}

#[test]
fn empty_stdin_is_a_usage_error() {
    let o = run_with(&["qr"], b"\n", piped());
    assert_eq!(o.status, ExitStatus::Usage);
    assert!(
        o.err.contains("no data to encode: stdin is empty"),
        "{}",
        o.err
    );
}

#[test]
fn verbose_describes_the_symbol_and_quiet_hides_warnings() {
    let o = run_piped(&[
        "qr",
        "-v",
        "--ec",
        "H",
        "--mask",
        "3",
        "--qr-version",
        "5",
        "12345",
    ]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(
        o.err,
        "note: encoded 5 bytes from the TEXT argument as version 5 (37x37), numeric mode, \
         error correction H, mask 3\n"
    );

    let o = run_piped(&["qr", "--margin", "1", "x"]);
    assert!(
        o.err.starts_with("warning: a margin of 1 is below"),
        "{}",
        o.err
    );
    assert!(o.err.contains("hint: use --margin 4"), "{}", o.err);
    let o = run_piped(&["qr", "-q", "--margin", "1", "x"]);
    assert_eq!(o.err, "");
    assert_eq!(o.status, ExitStatus::Success);
}

#[test]
fn verbose_explains_the_no_colour_fallback() {
    let env = Env {
        no_color: true,
        ..terminal()
    };
    let o = run_with(&["qr", "-v", "x"], b"", env);
    assert!(o.err.contains("note: colour is off"), "{}", o.err);
    assert!(o.err.contains("--invert"), "{}", o.err);
}

#[test]
fn diagnostics_are_coloured_only_on_a_colour_terminal() {
    let o = run_with(&["qr", "--margin", "0", "x"], b"", terminal());
    assert!(o.err.starts_with("\x1b[1m\x1b[33mwarning"), "{:?}", o.err);
    let env = Env {
        no_color: true,
        ..terminal()
    };
    let o = run_with(&["qr", "--margin", "0", "x"], b"", env);
    assert!(o.err.starts_with("warning:"), "{:?}", o.err);
}

#[test]
fn narrow_terminals_get_a_width_warning() {
    let narrow = Env {
        terminal_width: Some(20),
        ..terminal()
    };
    let o = run_with(&["qr", "x"], b"", narrow);
    assert_eq!(o.status, ExitStatus::Success);
    assert!(
        o.err
            .contains("the code is 29 columns wide but the terminal has 20"),
        "{}",
        o.err
    );

    // Unknown width, or text output, never warns.
    let unknown = Env {
        terminal_width: None,
        ..terminal()
    };
    assert!(
        !run_with(&["qr", "x"], b"", unknown)
            .err
            .contains("columns wide")
    );
    // Piped output never warns, whatever the terminal width.
    let piped_narrow = Env {
        stdout_is_tty: false,
        ..narrow
    };
    assert!(
        !run_with(&["qr", "-f", "ascii", "x"], b"", piped_narrow)
            .err
            .contains("columns wide")
    );
}

#[test]
fn every_text_format_on_a_terminal_gets_the_width_warning() {
    let narrow = Env {
        terminal_width: Some(40),
        ..terminal()
    };
    // ASCII uses two columns per module: 21 + 2 * 4 = 29 modules, 58 columns.
    let o = run_with(&["qr", "-f", "ascii", "x"], b"", narrow);
    assert_eq!(o.status, ExitStatus::Success);
    assert!(
        o.err
            .contains("the code is 58 columns wide but the terminal has 40"),
        "{}",
        o.err
    );
    assert!(
        o.err.contains(": widen the terminal, use -f text"),
        "{}",
        o.err
    );
    assert!(!o.err.contains("--margin"), "{}", o.err);

    // Plain text fits in 40 columns, but not in 20.
    assert!(
        !run_with(&["qr", "-f", "text", "x"], b"", narrow)
            .err
            .contains("columns wide")
    );
    let narrower = Env {
        terminal_width: Some(20),
        ..terminal()
    };
    let o = run_with(&["qr", "-f", "text", "x"], b"", narrower);
    assert!(o.err.contains("the code is 29 columns wide"), "{}", o.err);
    assert!(
        o.err.contains(": widen the terminal or shorten the input"),
        "{}",
        o.err
    );
}

#[test]
fn unquoted_words_are_reported_as_an_extra_argument() {
    for args in [
        &["qr", "hello", "world"][..],
        &["qr", "--", "hello", "world"],
    ] {
        let o = run_piped(args);
        assert_eq!(o.status, ExitStatus::Usage, "{args:?}");
        assert_eq!(
            o.err,
            "error: unexpected extra argument 'world'\n  \
             hint: quote text that contains spaces: qr 'hello world'\n",
            "{args:?}"
        );
        assert_eq!(o.out, "");
    }

    // The stray word is user data, so it is sanitised.
    let o = run_piped(&["qr", "hello", "\x1b[31mred"]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert!(o.err.contains("'\\u{1b}[31mred'"), "{}", o.err);
    assert!(!o.err.contains('\x1b'));

    // A real subcommand name keeps clap's own error.
    for word in ["man", "completions", "help"] {
        let o = run_piped(&["qr", "hello", word]);
        assert_eq!(o.status, ExitStatus::Usage, "{word}");
        assert!(
            o.err
                .contains(&format!("the subcommand '{word}' cannot be used")),
            "{word}: {}",
            o.err
        );
    }
}

#[test]
fn invert_swaps_dark_and_light() {
    let normal = run_piped(&["qr", "--margin", "0", "x"]).out;
    let inverted = run_piped(&["qr", "--margin", "0", "--invert", "x"]).out;
    let (a, b) = (parse_half_blocks(&normal), parse_half_blocks(&inverted));
    // Compare every real row (the last parsed row is a phantom half row).
    for (ra, rb) in a.iter().zip(&b).take(a.len() - 1) {
        for (ma, mb) in ra.iter().zip(rb) {
            assert_ne!(ma, mb);
        }
    }
}

#[test]
fn encoding_errors_map_to_exit_codes_with_hints() {
    let long = "a".repeat(2_400);
    let o = run_piped(&["qr", "--ec", "M", &long]);
    assert_eq!(o.status, ExitStatus::DataTooLong);
    assert!(
        o.err
            .starts_with("error: data too long: 2400 bytes exceeds the maximum of 2331"),
        "{}",
        o.err
    );
    assert!(
        o.err
            .contains("hint: shorten the input or lower --ec (now M"),
        "{}",
        o.err
    );

    let o = run_piped(&["qr", "--mode", "numeric", "12a"]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert!(o.err.contains("byte 0x61 at offset 2"), "{}", o.err);
    assert!(o.err.contains("hint: use --mode auto"), "{}", o.err);
}

#[test]
fn oversized_stdin_stops_reading_at_the_limit() {
    let big = vec![b'9'; crate::input::MAX_READ_BYTES + 1];
    let o = run_with(&["qr"], &big, piped());
    assert_eq!(o.status, ExitStatus::DataTooLong);
    assert!(
        o.err.contains("stdin has more than 7089 bytes"),
        "{}",
        o.err
    );
}

#[test]
fn missing_input_file_is_an_io_error_with_a_sanitized_path() {
    let o = run_piped(&["qr", "-i", "/nonexistent/dir/\x1b[31mevil"]);
    assert_eq!(o.status, ExitStatus::Io);
    assert!(
        o.err
            .starts_with("error: cannot read '/nonexistent/dir/\\u{1b}[31mevil'"),
        "{}",
        o.err
    );
    assert!(!o.err.contains('\x1b'));
}

#[test]
fn completions_cover_every_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let o = run_piped(&["qr", "completions", shell]);
        assert_eq!(o.status, ExitStatus::Success, "{shell}: {}", o.err);
        assert!(o.out.contains("qr"), "{shell}");
        assert!(o.out.contains("keep-newline"), "{shell}");
    }
    let o = run_piped(&["qr", "completions", "tcsh"]);
    assert_eq!(o.status, ExitStatus::Usage);
}

#[test]
fn man_page_is_roff_for_qr() {
    let o = run_piped(&["qr", "man"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert!(o.out.starts_with(".ie \\n(.g .ds Aq"), "{}", &o.out[..40]);
    assert!(o.out.contains(".TH qr 1"), "{}", o.out);
    assert!(o.out.contains("keep\\-newline"), "{}", o.out);
}

#[test]
fn subcommand_words_are_encodable_after_double_dash() {
    let o = run_piped(&["qr", "--", "man"]);
    assert_eq!(o.status, ExitStatus::Success);
    assert_eq!(decode_grid(&parse_half_blocks(&o.out)), "man");
}
