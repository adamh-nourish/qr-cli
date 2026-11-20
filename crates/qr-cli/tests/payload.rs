//! End-to-end tests of the payload subcommands (`qr wifi`, `qr vcard`, ...)
//! against the real `qr` binary.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn qr() -> Command {
    let mut cmd = cargo_bin_cmd!("qr");
    cmd.env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env("COLUMNS", "100");
    cmd
}

/// The `--print-payload` output of `args`, without the trailing newline.
fn payload(args: &[&str], stdin: &str) -> String {
    let out = qr()
        .args(args)
        .arg("--print-payload")
        .write_stdin(stdin)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    text.strip_suffix('\n').unwrap().to_owned()
}

/// Decodes the single QR code in a PNG.
fn scan(png: &[u8]) -> String {
    let img = image::load_from_memory(png).unwrap().to_luma8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        img.get_pixel(u32::try_from(x).unwrap(), u32::try_from(y).unwrap())
            .0[0]
    });
    let grids = prepared.detect_grids();
    assert_eq!(grids.len(), 1, "expected one symbol");
    grids[0].decode().unwrap().1
}

#[test]
fn every_subcommand_prints_its_payload() {
    let cases: [(&[&str], &str); 9] = [
        (&["url", "example.com/a?b=1"], "https://example.com/a?b=1"),
        (
            &["wifi", "--ssid", "Café;Net", "--auth", "nopass"],
            "WIFI:T:nopass;S:Café\\;Net;;",
        ),
        (
            &[
                "vcard",
                "Ada Lovelace",
                "--phone",
                "+44 1",
                "--email",
                "ada@x.io",
            ],
            "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Lovelace;Ada;;;\r\nFN:Ada Lovelace\r\n\
             TEL:+44 1\r\nEMAIL:ada@x.io\r\nEND:VCARD\r\n",
        ),
        (&["mecard", "Ada", "--phone", "1"], "MECARD:N:Ada;TEL:1;;"),
        (
            &["email", "ada@x.io", "--subject", "Hi & bye"],
            "mailto:ada@x.io?subject=Hi%20%26%20bye",
        ),
        (
            &["sms", "+44 7700 900123", "On my way"],
            "SMSTO:+447700900123:On my way",
        ),
        (&["tel", "+44 20 7946 0000"], "tel:+442079460000"),
        (&["geo", "-33.8568", "151.2153"], "geo:-33.8568,151.2153"),
        (
            &["event", "Launch", "--start", "2026-11-20T15:00+01:00"],
            "BEGIN:VEVENT\r\nSUMMARY:Launch\r\nDTSTART:20261120T140000Z\r\nEND:VEVENT\r\n",
        ),
    ];
    for (args, expected) in cases {
        assert_eq!(payload(args, ""), expected, "{args:?}");
    }
}

#[test]
fn password_stdin_keeps_the_secret_out_of_argv_and_messages() {
    let args = ["wifi", "--ssid", "Home", "--password-stdin"];
    assert_eq!(
        payload(&args, "s3cret:pass\n"),
        "WIFI:T:WPA;S:Home;P:s3cret\\:pass;;"
    );
    // Verbose notes describe the data but never print it.
    qr().args(args)
        .args(["-v", "-f", "text"])
        .write_stdin("s3cret:pass\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("the wifi payload"))
        .stderr(predicate::str::contains("s3cret").not());
    // A bad password is reported without echoing it.
    qr().args(args)
        .write_stdin("short\n")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid password"))
        .stderr(predicate::str::contains("short").not());
}

#[test]
fn printed_payloads_pipe_back_into_qr_unchanged() {
    let args = ["vcard", "Ada Lovelace", "--note", "a;b\nc"];
    let printed = qr()
        .args(args)
        .arg("--print-payload")
        .output()
        .unwrap()
        .stdout;
    let direct = qr().args(args).args(["-f", "svg"]).output().unwrap();
    let piped = qr()
        .args(["-f", "svg"])
        .write_stdin(printed)
        .output()
        .unwrap();
    assert!(direct.status.success() && piped.status.success());
    assert_eq!(direct.stdout, piped.stdout);
}

#[test]
fn payload_codes_scan_back_with_render_options() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wifi.png");
    let out = qr()
        .args([
            "wifi",
            "--ssid",
            "Home \"5G\"",
            "--password-stdin",
            "--hidden",
        ])
        .args(["--shape", "rounded", "--ec", "Q", "--json", "-o"])
        .arg(&path)
        .write_stdin("p,a;s:s\\word")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["format"], "png");
    assert_eq!(v["ec"], "Q");
    assert_eq!(v["verified"], true);
    let expected = "WIFI:T:WPA;S:Home \\\"5G\\\";P:p\\,a\\;s\\:s\\\\word;H:true;;";
    assert_eq!(v["bytes"], expected.len());
    assert_eq!(scan(&std::fs::read(&path).unwrap()), expected);
}

#[test]
fn subcommand_names_are_encoded_literally_after_double_dash() {
    for word in ["wifi", "url", "event"] {
        let literal = qr().args(["-f", "svg", "--", word]).output().unwrap();
        let piped = qr().args(["-f", "svg"]).write_stdin(word).output().unwrap();
        assert!(literal.status.success(), "{literal:?}");
        assert_eq!(literal.stdout, piped.stdout, "{word}");
    }
    qr().arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("qr -- wifi"));
}

#[test]
fn invalid_fields_are_usage_errors_with_the_value() {
    qr().args(["url", "https://exa mple.com"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "invalid URL 'https://exa mple.com'",
        ));
    qr().args(["geo", "91", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid latitude '91'"));
    qr().args(["event", "x", "--start", "2026-11-20", "--end", "2026-11-20"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("exclusive"));
    qr().args(["wifi", "--ssid", "Home"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--auth nopass"));
    qr().args(["tel", "x", "--json", "--data-uri"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"invalid_payload\""));
}

#[test]
fn output_errors_come_before_the_password_is_read() {
    // An unknown extension is reported, and stdin is never consumed.
    qr().args([
        "wifi",
        "--ssid",
        "Home",
        "--password-stdin",
        "-o",
        "w.unknown",
    ])
    .write_stdin("hunter2hunter2")
    .assert()
    .code(2)
    .stderr(predicate::str::contains("cannot infer the output format"));
    // A password that would fail validation pins the order: had stdin been
    // read first, its error would win.
    qr().args([
        "wifi",
        "--ssid",
        "Home",
        "--password-stdin",
        "-o",
        "w.unknown",
    ])
    .write_stdin("short\n")
    .assert()
    .code(2)
    .stderr(predicate::str::contains("cannot infer the output format"))
    .stderr(predicate::str::contains("password").not());
}

#[test]
fn options_before_a_subcommand_get_the_corrected_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let cases: [(&[&str], &str); 5] = [
        (
            &["-o", "a.png", "tel", "1"],
            "error: options go after the subcommand: 'tel'\n  \
             hint: run: qr tel 1 -o a.png (or put -- before 'tel' to encode the word itself)\n",
        ),
        (
            &["-o", "a.png", "url", "example.com"],
            "error: options go after the subcommand: 'url'\n  \
             hint: run: qr url example.com -o a.png (or put -- before 'url' to encode the word \
             itself)\n",
        ),
        (
            &["--shape", "dots", "vcard", "Ada Lovelace"],
            "error: options go after the subcommand: 'vcard'\n  \
             hint: run: qr vcard 'Ada Lovelace' --shape dots (or put -- before 'vcard' to encode \
             the word itself)\n",
        ),
        (
            &["-o", "a.png", "wifi"],
            "error: options go after the subcommand: 'wifi'\n  \
             hint: run: qr wifi -o a.png (or put -- before 'wifi' to encode the word itself)\n",
        ),
        (
            // A password given on the command line is not repeated.
            &[
                "-qo",
                "a.png",
                "wifi",
                "--ssid",
                "Home",
                "--password",
                "hunter22",
            ],
            "error: options go after the subcommand: 'wifi'\n  \
             hint: run: qr wifi --ssid Home --password PASSWORD -qo a.png (or put -- before \
             'wifi' to encode the word itself)\n",
        ),
    ];
    for (args, expected) in cases {
        qr().current_dir(dir.path())
            .args(args)
            .assert()
            .code(2)
            .stdout("")
            .stderr(expected.to_owned());
    }
    assert!(!dir.path().join("a.png").exists(), "nothing is written");

    // Under --json the hint is a field.
    let out = qr()
        .current_dir(dir.path())
        .args(["--json", "-o", "a.png", "wifi"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], "usage");
    assert_eq!(
        v["error"]["hint"],
        "run: qr wifi --json -o a.png (or put -- before 'wifi' to encode the word itself)"
    );

    // An option value that is a subcommand name is just a value, and --
    // still makes the word data.
    let literal = qr().args(["-f", "svg", "--", "wifi"]).output().unwrap();
    let piped = qr()
        .args(["-f", "svg"])
        .write_stdin("wifi")
        .output()
        .unwrap();
    assert_eq!(literal.stdout, piped.stdout);
    qr().current_dir(dir.path())
        .args(["-o", "url", "-f", "svg", "hello"])
        .assert()
        .success();
    assert!(dir.path().join("url").exists());
}

#[test]
fn a_subcommand_missing_its_fields_says_how_to_encode_the_word() {
    for (word, missing) in [("wifi", "--ssid"), ("email", "<TO>"), ("geo", "<LAT>")] {
        qr().arg(word)
            .assert()
            .code(2)
            .stderr(predicate::str::contains(missing))
            .stderr(predicate::str::contains(format!(
                "  hint: to encode the word '{word}' itself, run: qr -- {word}\n"
            )));
    }
    let out = qr().args(["email", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(
        v["error"]["hint"],
        "to encode the word 'email' itself, run: qr -- email"
    );
}

#[test]
fn url_schemes_without_slashes_are_schemes() {
    qr().args(["url", "mailto:a@b.com"])
        .assert()
        .code(2)
        .stderr(
            "error: invalid URL 'mailto:a@b.com': this kind of link has its own subcommand\n  \
             hint: use qr email for mailto: links, for example: qr email ada@example.com\n",
        );
    qr().args(["url", "javascript:alert(1)"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "script and data: links are not allowed",
        ));
    qr().args(["url", "javascript://x%0aalert(1)"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not allowed"));
    for (input, expected) in [
        ("localhost:8080/x", "https://localhost:8080/x"),
        ("urn:isbn:0451450523", "urn:isbn:0451450523"),
    ] {
        assert_eq!(payload(&["url", input], ""), expected);
    }
    // A vCard URL is normalised the same way.
    assert!(
        payload(&["vcard", "A", "--url", "example.com"], "")
            .contains("URL:https://example.com\r\n")
    );
}

#[test]
fn payload_errors_split_the_hint_from_the_message() {
    qr().args(["wifi", "--ssid", "Home"])
        .assert()
        .code(2)
        .stderr(
            "error: a wpa or wep network needs a password\n  \
             hint: pass --password-stdin (or --password), or --auth nopass for an open network\n",
        );
    let out = qr()
        .args(["mecard", "A", "--json", "--data-uri"])
        .args(["--url", "a b"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], "invalid_payload");
    assert_eq!(
        v["error"]["message"],
        "invalid URL 'a b': contains whitespace"
    );
    assert_eq!(v["error"]["hint"], "write a space as %20");
    // The length is given, the password is not.
    qr().args(["wifi", "--ssid", "Home", "--password-stdin"])
        .write_stdin("abcdef\n")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("(got 6 characters)"))
        .stderr(predicate::str::contains("abcdef").not());
    qr().args(["wifi", "--ssid", "Home", "--password-stdin"])
        .write_stdin("")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "error: invalid password: stdin is empty\n",
        ));
}

#[test]
fn sms_takes_the_message_as_a_word_or_body() {
    let positional = payload(&["sms", "+44 1", "hi there"], "");
    assert_eq!(positional, "SMSTO:+441:hi there");
    assert_eq!(
        payload(&["sms", "+44 1", "--body", "hi there"], ""),
        positional
    );
    qr().args(["sms", "1", "a", "--body", "b"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn print_payload_conflicts_with_output_options() {
    qr().args(["tel", "1", "--print-payload", "-o", "x.png"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn every_subcommand_help_has_examples_and_a_render_options_heading() {
    for sub in [
        "url", "wifi", "vcard", "mecard", "email", "sms", "tel", "geo", "event",
    ] {
        let out = qr().args([sub, "--help"]).output().unwrap();
        assert!(out.status.success());
        let help = String::from_utf8(out.stdout).unwrap();
        let examples = help.split("Examples:\n").nth(1).unwrap_or_default();
        let count = examples
            .lines()
            .filter(|l| {
                l.starts_with(&format!("  qr {sub} ")) || l.contains(&format!("| qr {sub} "))
            })
            .count();
        assert!((2..=5).contains(&count), "{sub}: {count} examples\n{help}");
        let (own, render) = help.split_once("Render options:\n").unwrap();
        assert!(
            render.contains("--shape") && !own.contains("--shape"),
            "{sub}"
        );
    }
    let out = qr().args(["wifi", "--help"]).output().unwrap();
    let wifi_help = String::from_utf8(out.stdout).unwrap();
    insta::assert_snapshot!("wifi_help", wifi_help);
}

#[test]
fn subcommand_help_lists_fields_and_render_options() {
    qr().args(["wifi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--password-stdin"))
        .stdout(predicate::str::contains("--print-payload"))
        .stdout(predicate::str::contains("--shape"));
}
