//! End-to-end tests of `--logo` and `--logo-scale` against the real `qr`
//! binary: the formats that carry a logo, error correction H, automatic
//! verification, and every way a logo file can be refused.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use image::{ImageFormat, Rgba, RgbaImage};
use predicates::prelude::*;

const URL: &str = "https://example.com/menu?table=12";

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

/// Writes a 64 x 48 logo (a red disc on transparency) as `name` in `dir`,
/// in the format its extension names.
fn logo(dir: &Path, name: &str) -> PathBuf {
    let image = RgbaImage::from_fn(64, 48, |x, y| {
        let (dx, dy) = (f64::from(x) - 32.0, f64::from(y) - 24.0);
        if dx.hypot(dy) < 22.0 {
            Rgba([220, 20, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let path = dir.join(name);
    let format = ImageFormat::from_path(&path).unwrap();
    if format == ImageFormat::Jpeg {
        image::DynamicImage::ImageRgba8(image)
            .to_rgb8()
            .save_with_format(&path, format)
            .unwrap();
    } else {
        image.save_with_format(&path, format).unwrap();
    }
    path
}

#[test]
fn logos_on_every_raster_format_scan_and_are_verified() {
    let dir = tempfile::tempdir().unwrap();
    for logo_name in ["logo.png", "logo.jpg", "logo.webp"] {
        let logo = logo(dir.path(), logo_name);
        for ext in ["png", "jpeg", "webp"] {
            let out = dir.path().join(format!("code-{logo_name}.{ext}"));
            let assert = qr()
                .args(["-v", URL, "--logo"])
                .arg(&logo)
                .arg("-o")
                .arg(&out)
                .assert()
                .success();
            let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
            assert!(
                stderr.contains("note: --logo sets error correction to H"),
                "{stderr}"
            );
            assert!(stderr.contains("error correction H"), "{stderr}");
            assert!(
                stderr.contains(&format!("note: verified: the {ext} output reads back")),
                "{stderr}"
            );
            assert_eq!(scan(&out), URL, "{logo_name} on {ext}");
        }
    }
}

#[test]
fn without_v_a_logo_code_is_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let logo = logo(dir.path(), "l.png");
    let out = dir.path().join("c.png");
    qr().args([URL, "--logo-scale", "0.3", "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stderr("");
    assert_eq!(scan(&out), URL);
}

#[test]
fn svg_embeds_the_logo_and_payload_commands_take_it() {
    let dir = tempfile::tempdir().unwrap();
    let logo = logo(dir.path(), "l.webp");
    let out = dir.path().join("c.svg");
    qr().args([URL, "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    let svg = fs::read_to_string(&out).unwrap();
    assert_eq!(svg.matches("<image ").count(), 1, "{svg}");
    assert!(svg.contains("href=\"data:image/png;base64,"), "{svg}");
    roxmltree::Document::parse(&svg).unwrap();

    let out = dir.path().join("wifi.png");
    qr().args(["wifi", "--ssid", "Cafe", "--auth", "nopass", "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&out)
        .assert()
        .success();
    assert_eq!(scan(&out), "WIFI:T:nopass;S:Cafe;;");
}

#[test]
fn a_lower_ec_is_overridden_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let logo = logo(dir.path(), "l.png");
    let out = dir.path().join("c.png");
    let assert = qr()
        .args([URL, "--ec", "L", "--json", "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "--logo needs error correction H; using H instead of --ec L",
        ));
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(stdout.contains("\"ec\":\"H\""), "{stdout}");
    assert!(stdout.contains("\"verified\":true"), "{stdout}");
    // --ec H is what --logo wants: no warning.
    qr().args([URL, "--ec", "H", "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&out)
        .assert()
        .success()
        .stderr("");
}

#[test]
fn formats_that_cannot_carry_a_logo_are_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let logo = logo(dir.path(), "l.png");
    for format in ["pdf", "eps", "text", "ascii"] {
        qr().args(["hi", "-f", format, "-o"])
            .arg(dir.path().join(format!("c.{format}")))
            .arg("--logo")
            .arg(&logo)
            .assert()
            .code(2)
            .stderr(predicate::str::contains(format!(
                "--logo needs png, jpeg, webp or svg output, not {format}"
            )));
    }
    // --logo-scale without --logo, and out of range.
    qr().args(["hi", "--logo-scale", "0.2"]).assert().code(2);
    qr().args(["hi", "--logo-scale", "0.5", "--logo"])
        .arg(&logo)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("from 0.05 to 0.3"));
}

/// The CRC-32 a PNG chunk carries.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn bad_logo_files_are_refused_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let good = fs::read(logo(dir.path(), "good.png")).unwrap();

    // A decompression bomb: a valid PNG whose header claims 50,000 px.
    let mut bomb = good.clone();
    bomb[16..20].copy_from_slice(&50_000_u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&50_000_u32.to_be_bytes());
    let crc = crc32(&bomb[12..29]);
    bomb[29..33].copy_from_slice(&crc.to_be_bytes());

    // A sparse 11 MiB file: refused from its size, before it is read.
    let huge = dir.path().join("huge.png");
    File::create(&huge)
        .unwrap()
        .set_len(11 * 1024 * 1024)
        .unwrap();

    let cases: [(&str, Vec<u8>, &str); 5] = [
        ("bomb.png", bomb, "exceeds the 8192x8192 pixel limit"),
        (
            "gif.png",
            b"GIF89a\x01\x00\x01\x00".to_vec(),
            "not a png, jpeg or webp image",
        ),
        (
            "cut.png",
            good[..good.len() / 2].to_vec(),
            "cannot decode the png image",
        ),
        ("empty.png", Vec::new(), "the image file is empty"),
        (
            "logo.svg",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
            "not a png",
        ),
    ];
    for (name, bytes, reason) in cases {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        qr().args(["hi", "-f", "png", "-o", "-", "--logo"])
            .arg(&path)
            .assert()
            .code(2)
            .stdout("")
            .stderr(predicate::str::contains(format!(
                "error: cannot use logo '{}': ",
                path.display()
            )))
            .stderr(predicate::str::contains(reason));
    }
    qr().args(["hi", "-f", "png", "-o", "-", "--logo"])
        .arg(&huge)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "the image is 11534336 bytes, over the 10485760-byte (10 MiB) limit",
        ))
        .stderr(predicate::str::contains("hint: use a smaller logo image"));
    // --json names the error.
    qr().args(["hi", "--json", "-f", "png", "-o"])
        .arg(dir.path().join("x.png"))
        .arg("--logo")
        .arg(dir.path().join("gif.png"))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"code\":\"invalid_logo\""));
}

#[test]
fn unreadable_logos_are_io_errors_and_stdin_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    qr().args(["-f", "png", "-o", "-", "--logo"])
        .arg(dir.path().join("missing.png"))
        .write_stdin("data")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("cannot read logo"))
        .stderr(predicate::str::contains("no such file or directory"))
        .stderr(predicate::str::contains(
            "hint: check the path passed to --logo",
        ));
    qr().args(["hi", "-f", "png", "-o", "-", "--logo"])
        .arg(dir.path())
        .assert()
        .code(4)
        .stderr(predicate::str::contains("it is a directory"));
}
