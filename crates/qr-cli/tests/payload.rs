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
}

#[test]
fn print_payload_conflicts_with_output_options() {
    qr().args(["tel", "1", "--print-payload", "-o", "x.png"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
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
