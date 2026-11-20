//! End-to-end tests of `qr decode` against the real `qr` binary: every
//! image `qr` writes reads back, the formats it cannot read are refused
//! cleanly, decompression bombs are rejected before they are decoded, and
//! decoded data never reaches the terminal raw.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use image::{GrayImage, ImageFormat, Rgba, RgbaImage};
use predicates::prelude::*;

const URL: &str = "https://example.com/menu?table=12";

fn qr() -> Command {
    let mut cmd = cargo_bin_cmd!("qr");
    cmd.env_remove("NO_COLOR").env_remove("CLICOLOR_FORCE");
    cmd
}

/// Runs `qr` with `args` plus `-o path` and returns the path.
fn generate(dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let path = dir.join(name);
    qr().args(args).arg("-o").arg(&path).assert().success();
    path
}

/// `qr decode path`'s stdout, which must succeed.
fn decode(path: &Path) -> String {
    let out = qr().arg("decode").arg(path).output().unwrap();
    assert!(out.status.success(), "{}: {out:?}", path.display());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn every_image_format_qr_writes_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let logo = dir.path().join("logo.png");
    RgbaImage::from_pixel(32, 32, Rgba([200, 30, 30, 255]))
        .save_with_format(&logo, ImageFormat::Png)
        .unwrap();
    let logo = logo.to_str().unwrap();
    let styles: [&[&str]; 9] = [
        &[],
        &["--shape", "dots"],
        &["--shape", "rounded", "--fg", "navy", "--bg", "#ffe"],
        &["--transparent"],
        &["--invert"],
        &["--ec", "H", "--margin", "1", "--scale", "1"],
        &["--size", "1000", "--qr-version", "20"],
        &["--logo", logo, "--logo-scale", "0.25"],
        &["--fg", "white", "--bg", "black"],
    ];
    for format in ["png", "jpeg", "webp"] {
        for (i, style) in styles.iter().enumerate() {
            // JPEG needs at least 4 px per module.
            if format == "jpeg" && style.contains(&"--scale") {
                continue;
            }
            let name = format!("{i}.{format}");
            let mut args = vec![URL];
            args.extend_from_slice(style);
            let path = generate(dir.path(), &name, &args);
            assert_eq!(decode(&path), format!("{URL}\n"), "{format} {style:?}");
        }
    }
}

#[test]
fn json_metadata_matches_what_qr_reports_when_generating() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.png");
    let generated = qr()
        .args([URL, "--ec", "Q", "--mask", "5", "--json", "-o"])
        .arg(&path)
        .output()
        .unwrap();
    let generated: serde_json::Value = serde_json::from_slice(&generated.stdout).unwrap();
    let out = qr().args(["decode", "--json"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let decoded: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let payload = &decoded["images"][0]["payloads"][0];
    assert_eq!(payload["text"], URL);
    assert_eq!(payload["bytes"], generated["bytes"]);
    assert_eq!(payload["version"], generated["version"]);
    assert_eq!(payload["ec"], generated["ec"]);
    assert_eq!(payload["mask"], generated["mask"]);
    assert_eq!(decoded["summary"]["codes"], 1);
}

#[test]
fn payload_subcommands_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let wifi = generate(
        dir.path(),
        "wifi.png",
        &["wifi", "--ssid", "Home;Net", "--password", "pass:word"],
    );
    assert_eq!(decode(&wifi), "WIFI:T:WPA;S:Home\\;Net;P:pass\\:word;;\n");
    let geo = generate(dir.path(), "geo.webp", &["geo", "51.5", "-0.12"]);
    assert_eq!(decode(&geo), "geo:51.5,-0.12\n");
}

#[test]
fn stdin_and_binary_data_round_trip_exactly_with_raw() {
    let dir = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..=255).collect();
    let input = dir.path().join("all-bytes.bin");
    fs::write(&input, &data).unwrap();
    let png = qr()
        .args(["-f", "png", "-o", "-", "--keep-newline", "-i"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(png.status.success());
    let out = qr()
        .args(["decode", "-", "--raw"])
        .write_stdin(png.stdout)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let mut expected = data;
    expected.push(b'\n');
    assert_eq!(out.stdout, expected);
}

#[test]
fn escape_sequences_in_data_are_sanitised() {
    let dir = tempfile::tempdir().unwrap();
    // Clear the screen, set the title, and reverse the text that follows.
    let evil = "\x1b[2J\x1b]0;owned\x07\u{202e}txt.exe";
    let path = generate(dir.path(), "evil.png", &[evil]);
    qr().arg("decode")
        .arg(&path)
        .assert()
        .success()
        .stdout("\\u{1b}[2J\\u{1b}]0;owned\\u{7}\\u{202e}txt.exe\n");
    // JSON escapes them too, losslessly.
    let out = qr().args(["decode", "--json"]).arg(&path).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        !text.contains('\x1b') && !text.contains('\u{202e}'),
        "{text}"
    );
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["images"][0]["payloads"][0]["text"], evil);
}

#[test]
fn formats_that_are_not_images_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    for format in ["svg", "pdf", "eps", "text", "ascii"] {
        let path = dir.path().join(format!("code.{format}"));
        qr().args(["x", "-f", format, "-o"])
            .arg(&path)
            .assert()
            .success();
        qr().arg("decode")
            .arg(&path)
            .assert()
            .code(4)
            .stdout("")
            .stderr(predicate::str::contains(
                "not a png, jpeg or webp image (the header matches none of them)",
            ))
            .stderr(predicate::str::contains(
                "hint: qr decode reads png, jpeg and webp",
            ));
    }
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

/// `png` with its header rewritten to claim `width` x `height` pixels at
/// `depth` bits per channel and `color` type, with a correct CRC.
fn forged_header(png: &[u8], width: u32, height: u32, depth: u8, color: u8) -> Vec<u8> {
    let mut forged = png.to_vec();
    forged[16..20].copy_from_slice(&width.to_be_bytes());
    forged[20..24].copy_from_slice(&height.to_be_bytes());
    forged[24] = depth;
    forged[25] = color;
    let crc = crc32(&forged[12..29]);
    forged[29..33].copy_from_slice(&crc.to_be_bytes());
    forged
}

#[test]
fn decompression_bombs_are_rejected_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let good = fs::read(generate(dir.path(), "good.png", &["x"])).unwrap();

    // A real bomb: 8192 x 8192 of black compresses to a few tens of KiB
    // but would need 256 MiB as RGBA.
    let real = dir.path().join("real-bomb.png");
    GrayImage::new(8_192, 8_192)
        .save_with_format(&real, ImageFormat::Png)
        .unwrap();
    assert!(fs::metadata(&real).unwrap().len() < 1024 * 1024);

    // Sparse 11 MiB file: refused from its size before it is read.
    let huge = dir.path().join("huge.png");
    File::create(&huge)
        .unwrap()
        .set_len(11 * 1024 * 1024)
        .unwrap();

    let cases: [(&str, Option<Vec<u8>>, &str); 4] = [
        (
            "wide.png",
            Some(forged_header(&good, 50_000, 50_000, 8, 6)),
            "exceeds the 8192x8192 pixel limit",
        ),
        (
            "deep.png",
            Some(forged_header(&good, 8_000, 8_000, 16, 6)),
            "over the 134217728-byte (128 MiB) limit",
        ),
        (
            "real-bomb.png",
            None,
            "over the 134217728-byte (128 MiB) limit",
        ),
        (
            "huge.png",
            None,
            "the image is 11534336 bytes, over the 10485760-byte (10 MiB) limit",
        ),
    ];
    for (name, bytes, reason) in cases {
        let path = dir.path().join(name);
        if let Some(bytes) = bytes {
            fs::write(&path, bytes).unwrap();
        }
        qr().arg("decode")
            .arg(&path)
            .assert()
            .code(4)
            .stdout("")
            .stderr(predicate::str::contains(format!(
                "error: cannot decode '{}': ",
                path.display()
            )))
            .stderr(predicate::str::contains(reason))
            .stderr(predicate::str::contains("hint: scale the image down"));
    }
    // From stdin, an endless stream stops at the size limit.
    qr().args(["decode", "-"])
        .write_stdin(vec![0x89; 11 * 1024 * 1024])
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "the image is more than 10485760 bytes",
        ));
}

#[test]
fn several_codes_in_one_image_are_all_printed() {
    let dir = tempfile::tempdir().unwrap();
    let a = image::open(generate(dir.path(), "a.png", &["left"])).unwrap();
    let b = image::open(generate(dir.path(), "b.png", &["right"])).unwrap();
    let mut both = RgbaImage::from_pixel(600, 300, Rgba([255; 4]));
    image::imageops::overlay(&mut both, &a.to_rgba8(), 0, 0);
    image::imageops::overlay(&mut both, &b.to_rgba8(), 300, 0);
    let path = dir.path().join("both.png");
    both.save_with_format(&path, ImageFormat::Png).unwrap();
    assert_eq!(decode(&path), "left\nright\n");
}

#[test]
fn exit_codes_and_the_summary_reflect_each_image() {
    let dir = tempfile::tempdir().unwrap();
    let good = generate(dir.path(), "good.png", &["ok"]);
    let blank = dir.path().join("blank.png");
    GrayImage::from_pixel(100, 100, image::Luma([255]))
        .save_with_format(&blank, ImageFormat::Png)
        .unwrap();
    qr().arg("decode")
        .arg(&blank)
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("error: no QR code found in"));
    qr().arg("decode")
        .arg(&good)
        .arg(&blank)
        .assert()
        .code(5)
        .stdout(predicate::str::ends_with("good.png: ok\n"))
        .stderr(predicate::str::ends_with(
            "error: 1 of 2 images failed (1 without a readable code); 1 code decoded\n",
        ));
    qr().arg("decode")
        .arg(&blank)
        .arg(dir.path().join("missing.png"))
        .arg(&good)
        .assert()
        .code(4)
        .stdout(predicate::str::ends_with("good.png: ok\n"))
        .stderr(predicate::str::contains("no such file or directory"))
        .stderr(predicate::str::ends_with(
            "error: 2 of 3 images failed (1 unreadable, 1 without a readable code); 1 code decoded\n",
        ));
}

#[test]
fn usage_errors_exit_2() {
    qr().arg("decode")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("<IMAGE>..."));
    qr().args(["decode", "a.png", "--raw", "--json"])
        .assert()
        .code(2);
    qr().args(["decode", "-", "-"])
        .write_stdin("x")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("can be read only once"));
    // Parse errors are JSON with --json.
    qr().args(["decode", "--json", "--bogus"])
        .assert()
        .code(2)
        .stderr(predicate::str::starts_with(
            "{\"error\":{\"code\":\"usage\"",
        ));
}

#[cfg(unix)]
#[test]
fn devices_and_fifos_are_not_read() {
    qr().args(["decode", "/dev/zero"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "cannot read '/dev/zero': not a regular file\n  hint: check the image path\n",
        ));
    // A FIFO with no writer is refused at once rather than blocking.
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe.png");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    qr().arg("decode")
        .arg(&fifo)
        .timeout(std::time::Duration::from_secs(30))
        .assert()
        .code(4)
        .stderr(predicate::str::contains("not a regular file"));
}

#[test]
fn every_unreadable_path_has_a_hint() {
    let dir = tempfile::tempdir().unwrap();
    qr().arg("decode")
        .arg(dir.path())
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "it is a directory\n  hint: name an image file, not a directory\n",
        ));
    let svg = generate(dir.path(), "code.svg", &["hello"]);
    qr().arg("decode")
        .arg(&svg)
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "hint: qr decode reads png, jpeg and webp; render the code with -f png to decode it",
        ));
    let empty = dir.path().join("empty.png");
    fs::write(&empty, b"").unwrap();
    qr().arg("decode")
        .arg(&empty)
        .assert()
        .code(4)
        .stderr(predicate::str::contains(
            "hint: check the file is a complete image",
        ));
}

/// A light `side` x `side` image holding an `n` x `n` grid of finder
/// patterns `module` pixels per module, `gap` modules apart.
fn finder_grid(n: u32, module: u32, gap: u32, side: u32) -> GrayImage {
    let period = (7 + gap) * module;
    GrayImage::from_fn(side, side, |x, y| {
        let (Some(x), Some(y)) = (x.checked_sub(4 * module), y.checked_sub(4 * module)) else {
            return image::Luma([255]);
        };
        let (gx, gy) = (x / period, y / period);
        let (mx, my) = ((x % period) / module, (y % period) / module);
        let ring = mx.abs_diff(3).max(my.abs_diff(3));
        let dark = gx < n && gy < n && mx < 7 && my < 7 && (ring == 3 || ring <= 1);
        image::Luma([if dark { 0 } else { 255 }])
    })
}

#[test]
fn a_dense_grid_of_finder_patterns_is_refused_quickly() {
    // The PR 11 security review's image: a few KB of PNG holding 40 x 40
    // finder patterns kept rqrr busy for minutes.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grid.png");
    finder_grid(40, 3, 4, 1_365)
        .save_with_format(&path, ImageFormat::Png)
        .unwrap();
    assert!(fs::metadata(&path).unwrap().len() < 64 * 1024);
    qr().arg("decode")
        .arg(&path)
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .code(4)
        .stdout("")
        .stderr(predicate::str::contains(
            "holds more than 64 finder-like patterns, too many to search\n  \
             hint: crop the image to the code you want to read\n",
        ));
    qr().args(["decode", "--json"])
        .arg(&path)
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .code(4)
        .stdout(predicate::str::contains("\"code\":\"too_many_finders\""));
}

#[test]
fn a_logo_tiled_with_finder_patterns_fails_verification_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let logo = dir.path().join("logo.png");
    finder_grid(12, 3, 1, 300)
        .save_with_format(&logo, ImageFormat::Png)
        .unwrap();
    qr().args([
        "--ec",
        "H",
        "--scale",
        "30",
        "--verify",
        "--logo-scale",
        "0.3",
        "--logo",
    ])
    .arg(&logo)
    .arg("-o")
    .arg(dir.path().join("out.png"))
    .arg("L".repeat(100))
    .timeout(std::time::Duration::from_secs(60))
    .assert()
    .code(6)
    .stderr(predicate::str::contains("too many to read back"));
}
