//! End-to-end tests that run the real `qr` binary.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn version_prints_name_and_version_and_exits_zero() {
    let expected = format!("qr {}\n", env!("CARGO_PKG_VERSION"));
    cargo_bin_cmd!("qr")
        .arg("--version")
        .assert()
        .success()
        .stdout(expected)
        .stderr(predicate::str::is_empty());
}

#[test]
fn short_version_flag_matches_long_flag() {
    let long = cargo_bin_cmd!("qr").arg("--version").output().unwrap();
    let short = cargo_bin_cmd!("qr").arg("-V").output().unwrap();
    assert!(short.status.success());
    assert_eq!(long.stdout, short.stdout);
}

#[test]
fn help_exits_zero() {
    cargo_bin_cmd!("qr")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: qr"));
}

#[test]
fn no_arguments_exits_with_usage_status() {
    cargo_bin_cmd!("qr")
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Usage: qr"));
}

#[test]
fn unknown_flag_exits_with_usage_status() {
    cargo_bin_cmd!("qr")
        .arg("--bogus")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument '--bogus'"));
}
