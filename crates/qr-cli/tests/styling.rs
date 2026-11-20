//! End-to-end tests of styling and verification: `--shape`, colours,
//! `--invert`, the contrast warnings, `--verify`/`--no-verify` and exit
//! code 6, run against the real `qr` binary.

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

/// Decodes the QR code in the image at `path`, independently of `qr`.
fn scan(path: &Path) -> String {
    let img = image::open(path).unwrap().to_luma8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| img.as_raw()[y * w + x]);
    let grids = prepared.detect_grids();
    assert_eq!(grids.len(), 1, "expected one code in {}", path.display());
    grids[0].decode().unwrap().1
}

#[test]
fn shapes_render_scannable_images_and_are_verified() {
    let dir = tempfile::tempdir().unwrap();
    for shape in ["square", "rounded", "dots"] {
        for ext in ["png", "jpeg", "webp", "svg", "pdf", "eps"] {
            let path = dir.path().join(format!("{shape}.{ext}"));
            let assert = qr()
                .args(["-v", "--shape", shape, "https://example.com/styled"])
                .arg("-o")
                .arg(&path)
                .assert()
                .success();
            let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
            // Shapes are checked automatically; square black on white is not.
            assert_eq!(
                stderr.contains("note: verified: the"),
                shape != "square",
                "{shape} {ext}: {stderr}"
            );
            if ["png", "jpeg", "webp"].contains(&ext) {
                assert_eq!(scan(&path), "https://example.com/styled", "{shape} {ext}");
            }
        }
    }
    let svg = fs::read_to_string(dir.path().join("dots.svg")).unwrap();
    assert!(svg.contains('C') && !svg.contains("crispEdges"), "{svg}");
}

#[test]
fn verified_note_says_how_the_code_was_read() {
    qr().args(["-v", "--shape", "dots", "-f", "svg", "hi"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "note: verified: the svg output reads back as the input (drawn at 290x290 px)",
        ));
    qr().args(["-v", "--verify", "-f", "text", "hi"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "note: verified: the text output reads back as the input",
        ));
}

#[test]
fn invisible_codes_fail_verification_and_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ghost.png");
    qr().args(["--fg", "white", "hello"])
        .arg("-o")
        .arg(&path)
        .assert()
        .code(6)
        .stderr(predicate::str::contains(
            "warning: the colours have a contrast ratio of 1.0:1 (#ffffff on #ffffff)",
        ))
        .stderr(predicate::str::contains(
            "error: no QR code could be read back from the rendered png\n  \
             hint: use a darker --fg or a lighter --bg (or --no-verify to write it anyway)\n",
        ));
    assert!(!path.exists(), "a failing code must not be written");

    // --no-verify writes it anyway, still with the warning.
    qr().args(["--fg", "white", "--no-verify", "hello"])
        .arg("-o")
        .arg(&path)
        .assert()
        .success()
        .stderr(predicate::str::contains("contrast ratio"));
    assert!(path.exists());

    // An explicit --verify fails the same way, with no --no-verify hint.
    let assert = qr()
        .args([
            "-q", "--verify", "--fg", "white", "-f", "pdf", "-o", "-", "x",
        ])
        .assert()
        .code(6)
        .stdout(predicate::str::is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert_eq!(
        stderr,
        "error: no QR code could be read back from the rendered pdf\n  \
         hint: use a darker --fg or a lighter --bg\n"
    );
}

#[test]
fn a_low_contrast_vector_code_gets_the_contrast_hint() {
    // Vector output is always read at 10 px per module, so --scale cannot
    // help, and --ec H cannot fix low contrast: the hint names the colours.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pale.svg");
    let assert = qr()
        .args(["--fg", "#ddd", "-f", "svg", "hello", "-o"])
        .arg(&path)
        .assert()
        .code(6);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert_eq!(
        stderr,
        "warning: the colours have a contrast ratio of 1.3:1 (#dddddd on #ffffff), below the \
         3:1 scanners need\n  hint: use a darker --fg or a lighter --bg\n\
         error: no QR code could be read back from the rendered svg\n  \
         hint: use a darker --fg or a lighter --bg (or --no-verify to write it anyway)\n"
    );
    assert!(!stderr.contains("--scale"));
    assert!(!path.exists());
}

#[test]
fn invert_warns_but_still_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("night.png");
    qr().args(["-v", "--invert", "night"])
        .arg("-o")
        .arg(&path)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: the modules are lighter than the background (#ffffff on #000000)",
        ))
        .stderr(predicate::str::contains("hint: drop --invert"))
        .stderr(predicate::str::contains("read as light on dark"));
    // Light modules on black: our scan helper only reads dark-on-light, so
    // check the corner is the dark background.
    let img = image::open(&path).unwrap().to_luma8();
    assert_eq!(img.get_pixel(0, 0).0, [0]);
}

#[test]
fn json_reports_verification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.svg");
    let out = qr()
        .args(["--json", "--shape", "rounded", "x"])
        .arg("-o")
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["verified"], true);

    // Unchecked codes have no `verified` field.
    let out = qr()
        .args(["--json", "x"])
        .arg("-o")
        .arg(&path)
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(result.get("verified").is_none(), "{result}");

    let out = qr()
        .args(["--json", "--fg", "#fff", "--data-uri", "x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(6));
    assert_eq!(out.stdout, b"");
    let lines: Vec<serde_json::Value> = String::from_utf8_lossy(&out.stderr)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["warning"]["code"], "low_contrast");
    assert_eq!(
        lines.last().unwrap()["error"]["code"],
        "verification_failed"
    );
}

#[test]
fn shape_is_an_image_flag() {
    qr().args(["--shape", "dots", "-f", "text", "x"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: --shape only applies to image formats",
        ));
    qr().args(["--shape", "hexagon", "x"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value 'hexagon'"));
}
