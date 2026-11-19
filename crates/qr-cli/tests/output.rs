//! End-to-end tests of file output: `-o`, `-f`, atomic writes,
//! `--no-clobber`, symlinks, `--data-uri` and `--json`, run against the
//! real `qr` binary in scratch directories.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn qr() -> Command {
    let mut cmd = cargo_bin_cmd!("qr");
    cmd.env_remove("NO_COLOR").env_remove("CLICOLOR_FORCE");
    cmd
}

/// Names in `dir` other than `keep`, to catch stray temporary files.
fn leftovers(dir: &Path, keep: &[&str]) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !keep.contains(&name.as_str()))
        .collect()
}

#[test]
fn extension_picks_the_format() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("code.svg", &b"<?xml"[..]),
        ("code.SVG", b"<?xml"),
        ("code.pdf", b"%PDF-1.4\n"),
        ("code.eps", b"%!PS-Adobe-3.0 EPSF-3.0\n"),
    ];
    for (name, magic) in cases {
        let path = dir.path().join(name);
        qr().arg("hello")
            .arg("-o")
            .arg(&path)
            .assert()
            .success()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::is_empty());
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(magic), "{name}");
    }
}

#[test]
fn svg_output_is_well_formed_and_safe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.svg");
    qr().args(["--fg", "#1a73e8", "--bg", "ivory", "--scale", "4", "hello"])
        .arg("-o")
        .arg(&path)
        .assert()
        .success();
    let svg = fs::read_to_string(&path).unwrap();
    let doc = roxmltree::Document::parse(&svg).unwrap();
    let root = doc.root_element();
    assert_eq!(root.tag_name().name(), "svg");
    // 21 modules plus a 4-module quiet zone on each side, 4 px each.
    assert_eq!(root.attribute("width"), Some("116"));
    assert!(svg.contains("#1a73e8") && svg.contains("#fffff0"), "{svg}");
    for node in doc.descendants() {
        assert_ne!(node.tag_name().name(), "script");
    }
}

#[test]
fn txt_files_hold_the_text_rendering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.txt");
    qr().arg("hello").arg("-o").arg(&path).assert().success();
    let stdout = qr().arg("hello").output().unwrap().stdout;
    assert_eq!(fs::read(&path).unwrap(), stdout);
}

#[test]
fn dash_writes_to_stdout() {
    let out = qr()
        .args(["-f", "pdf", "-o", "-", "hello"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.starts_with(b"%PDF-1.4\n"));
    assert!(out.stdout.ends_with(b"%%EOF\n"));
}

#[test]
fn unknown_extensions_are_usage_errors_and_create_nothing() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["code.gif", "code"] {
        let path = dir.path().join(name);
        qr().arg("hello")
            .arg("-o")
            .arg(&path)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "error: cannot infer the output format from '",
            ))
            .stderr(predicate::str::contains(
                "hint: pass -f FORMAT or use one of .png, .svg,",
            ));
        assert!(!path.exists());
    }
    assert_eq!(leftovers(dir.path(), &[]), Vec::<String>::new());
}

#[test]
fn raster_formats_are_a_clear_usage_error_for_now() {
    let dir = tempfile::tempdir().unwrap();
    for (args, name) in [
        (&["-f", "png"][..], "png"),
        (&["-f", "jpeg"], "jpeg"),
        (&["-f", "webp"], "webp"),
    ] {
        qr().args(args)
            .arg("hello")
            .assert()
            .code(2)
            .stderr(format!(
                "error: {name} output is not supported yet\n  \
                 hint: use -f svg or -f pdf for a scalable image\n"
            ));
    }
    let path = dir.path().join("code.png");
    qr().arg("hello").arg("-o").arg(&path).assert().code(2);
    assert!(!path.exists());
}

#[test]
fn existing_files_are_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.svg");
    fs::write(&path, "old contents").unwrap();
    qr().arg("hello").arg("-o").arg(&path).assert().success();
    assert!(fs::read_to_string(&path).unwrap().contains("<svg"));
    assert_eq!(leftovers(dir.path(), &["code.svg"]), Vec::<String>::new());
}

#[test]
fn no_clobber_refuses_an_existing_file_and_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.svg");
    fs::write(&path, "old contents").unwrap();
    qr().arg("hello")
        .arg("-o")
        .arg(&path)
        .arg("--no-clobber")
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("already exists"))
        .stderr(predicate::str::contains("drop --no-clobber"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "old contents");
    assert_eq!(leftovers(dir.path(), &["code.svg"]), Vec::<String>::new());

    // A new path is written normally.
    let fresh = dir.path().join("fresh.svg");
    qr().arg("hello")
        .arg("-o")
        .arg(&fresh)
        .arg("--no-clobber")
        .assert()
        .success();
    assert!(fresh.exists());
}

#[test]
fn no_clobber_without_an_output_path_is_a_usage_error() {
    qr().args(["--no-clobber", "hello"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--output"));
}

#[cfg(unix)]
#[test]
fn a_symlinked_output_is_replaced_and_its_target_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim.txt");
    fs::write(&victim, "precious").unwrap();
    let link = dir.path().join("code.svg");
    std::os::unix::fs::symlink(&victim, &link).unwrap();

    // With --no-clobber the link counts as an existing file.
    qr().arg("hello")
        .arg("-o")
        .arg(&link)
        .arg("--no-clobber")
        .assert()
        .code(4);
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    qr().arg("hello").arg("-o").arg(&link).assert().success();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
    let meta = fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_file());
    assert!(fs::read_to_string(&link).unwrap().contains("<svg"));
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_is_replaced_without_creating_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("nowhere.svg");
    let link = dir.path().join("code.svg");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    qr().arg("hello").arg("-o").arg(&link).assert().success();
    assert!(!target.exists());
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_file());
}

#[test]
fn missing_directories_and_directory_paths_are_io_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no/such/dir/code.svg");
    qr().arg("hello")
        .arg("-o")
        .arg(&missing)
        .assert()
        .code(4)
        .stderr(predicate::str::contains(": no such file or directory"))
        .stderr(predicate::str::contains(
            "hint: check that the directory exists",
        ));

    let as_dir = dir.path().join("dir.svg");
    fs::create_dir(&as_dir).unwrap();
    qr().arg("hello")
        .arg("-o")
        .arg(&as_dir)
        .assert()
        .code(4)
        .stderr(predicate::str::contains(": it is a directory"));
}

#[test]
fn a_failed_render_leaves_no_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.svg");
    qr().args(["--scale", "16384", "hello"])
        .arg("-o")
        .arg(&path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("exceeds the 16384 limit for svg"));
    assert_eq!(leftovers(dir.path(), &[]), Vec::<String>::new());
}

#[test]
fn output_errors_come_before_reading_stdin() {
    // stdin would be fine, but the output path is bad: exit 2, not 0.
    qr().args(["-o", "x.gif"])
        .write_stdin("hello\n")
        .assert()
        .code(2);
}

#[test]
fn data_uri_wraps_the_image_in_base64() {
    let svg = qr().args(["-f", "svg", "hello"]).output().unwrap().stdout;
    let expected = format!(
        "data:image/svg+xml;base64,{}\n",
        qr_cli::output::base64(&svg)
    );
    qr().args(["--data-uri", "hello"])
        .assert()
        .success()
        .stdout(expected.clone());

    // Into a file, the URI is the file's content.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("uri.txt");
    qr().args(["--data-uri", "-f", "svg", "hello"])
        .arg("-o")
        .arg(&path)
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);

    qr().args(["--data-uri", "-f", "text", "hello"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--data-uri needs an image format, not text",
        ));
}

#[test]
fn json_prints_a_result_object() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.pdf");
    let out = qr()
        .args(["--json", "hello"])
        .arg("-o")
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stderr, b"");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["output"], path.to_str().unwrap());
    assert_eq!(v["format"], "pdf");
    assert_eq!(v["version"], 1);
    assert_eq!(v["modules"], 21);
    assert_eq!(v["written"], fs::metadata(&path).unwrap().len());
}

#[test]
fn json_errors_are_objects_on_stderr() {
    let out = qr()
        .args(["--json", "-o", "x.gif", "hello"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stdout, b"");
    let v: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], "unknown_extension");
    assert_eq!(
        v["error"]["message"],
        "cannot infer the output format from 'x.gif'"
    );

    let out = qr().args(["--json", "--bogus"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], "usage");
}
