//! End-to-end tests that run the real `qr` binary.
//!
//! Under the test harness stdin is empty and stdout is a pipe, so output is
//! deterministic: the default format is plain Unicode text.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

/// The binary with colour variables cleared, so the caller's environment
/// cannot change the output.
fn qr() -> Command {
    let mut cmd = cargo_bin_cmd!("qr");
    cmd.env_remove("NO_COLOR").env_remove("CLICOLOR_FORCE");
    cmd
}

fn stdout_of(cmd: &mut Command) -> String {
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn version_prints_name_and_version_and_exits_zero() {
    let expected = format!("qr {}\n", env!("CARGO_PKG_VERSION"));
    qr().arg("--version")
        .assert()
        .success()
        .stdout(expected)
        .stderr(predicate::str::is_empty());
}

#[test]
fn help_is_stable() {
    insta::assert_snapshot!("help", stdout_of(qr().arg("--help")));
}

#[test]
fn text_render_is_stable() {
    insta::assert_snapshot!("text_hello", stdout_of(qr().arg("hello")));
}

#[test]
fn ascii_render_is_stable() {
    insta::assert_snapshot!(
        "ascii_hello_margin_1",
        stdout_of(qr().args(["-f", "ascii", "--margin", "1", "-q", "hello"]))
    );
}

#[test]
fn terminal_render_with_forced_colour_is_stable() {
    insta::assert_snapshot!(
        "terminal_hello_margin_1",
        stdout_of(
            qr().args(["-f", "terminal", "--margin", "1", "-q", "hello"])
                .env("CLICOLOR_FORCE", "1")
        )
    );
}

#[test]
fn no_color_beats_clicolor_force() {
    let out = stdout_of(
        qr().args(["-f", "terminal", "hello"])
            .env("CLICOLOR_FORCE", "1")
            .env("NO_COLOR", "1"),
    );
    assert!(!out.contains('\x1b'));
}

#[test]
fn stdin_and_files_encode_like_the_argument() {
    let from_arg = stdout_of(qr().arg("hello"));
    let from_stdin = stdout_of(qr().write_stdin("hello\n"));
    let from_dash = stdout_of(qr().arg("-").write_stdin("hello\r\n"));
    assert_eq!(from_arg, from_stdin);
    assert_eq!(from_arg, from_dash);

    let from_dash_file = stdout_of(qr().args(["-i", "-"]).write_stdin("hello\n"));
    assert_eq!(from_arg, from_dash_file);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("payload.txt");
    std::fs::write(&path, "hello\n").unwrap();
    let from_file = stdout_of(qr().arg("-i").arg(&path));
    let kept = stdout_of(qr().arg("--keep-newline").arg("-i").arg(&path));
    assert_eq!(from_arg, from_file);
    assert_ne!(from_arg, kept);
}

#[test]
fn unquoted_text_is_an_extra_argument_with_a_quoting_hint() {
    for args in [&["hello", "world"][..], &["--", "hello", "world"]] {
        qr().args(args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(
                "error: unexpected extra argument 'world'\n  \
                 hint: quote text that contains spaces: qr 'hello world'\n",
            );
    }
}

#[test]
fn closed_stdout_exits_4() {
    use std::io::Write as _;
    use std::process::Stdio;

    // `qr -` blocks on stdin, so the reading end of its stdout can be closed
    // before it writes anything. The write then fails with a broken pipe
    // (Rust ignores SIGPIPE), which must be an I/O error, not a panic.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_qr"))
        .arg("-")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"hello\n").unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("error: cannot write output: "),
        "{stderr}"
    );
}

#[test]
fn empty_stdin_exits_with_usage_status() {
    qr().assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "no data to encode: stdin is empty",
        ));
}

#[test]
fn text_and_input_file_together_is_a_usage_error() {
    qr().args(["hello", "-i", "x.txt"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn unknown_flag_exits_with_usage_status() {
    qr().arg("--bogus")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument '--bogus'"));
}

#[test]
fn data_too_long_exits_3() {
    qr().args(["--ec", "L"])
        .write_stdin("a".repeat(2_954))
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(
            "error: data too long: 2954 bytes exceeds the maximum of 2953 for byte mode at \
             error correction level L (version 40)\n  hint: shorten the input\n",
        );
}

#[test]
fn oversized_stdin_exits_3_without_reading_it_all() {
    qr().write_stdin(vec![b'1'; 1 << 20])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("stdin has more than 7089 bytes"));
}

#[test]
fn missing_file_exits_4() {
    qr().args(["-i", "definitely/missing.txt"])
        .assert()
        .code(4)
        .stderr(
            "error: cannot read 'definitely/missing.txt': no such file or directory\n  \
             hint: check the path passed to -i\n",
        );
}

#[test]
fn completions_and_man_page() {
    qr().args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef qr"));
    qr().arg("man")
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH qr 1"));
}
