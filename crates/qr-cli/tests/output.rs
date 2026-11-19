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

/// Decodes the QR code in an image file's bytes, flattening any
/// transparency onto white as a scanner would see it on a page.
fn scan(bytes: &[u8]) -> String {
    let img = image::load_from_memory(bytes).unwrap().to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| {
        let [red, green, blue, alpha] = img
            .get_pixel(u32::try_from(x).unwrap(), u32::try_from(y).unwrap())
            .0
            .map(u32::from);
        let luma = (299 * red + 587 * green + 114 * blue) / 1000;
        u8::try_from((luma * alpha + 255 * (255 - alpha)) / 255).unwrap()
    });
    let grids = prepared.detect_grids();
    assert_eq!(grids.len(), 1, "expected one symbol");
    grids[0].decode().unwrap().1
}

#[test]
fn raster_files_decode_back_to_the_input() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("code.png", &b"\x89PNG\r\n\x1a\n"[..]),
        ("code.PNG", b"\x89PNG\r\n\x1a\n"),
        ("code.jpg", b"\xff\xd8\xff"),
        ("code.jpeg", b"\xff\xd8\xff"),
        ("code.webp", b"RIFF"),
    ];
    for (name, magic) in cases {
        let path = dir.path().join(name);
        qr().args(["https://example.com/raster", "-o"])
            .arg(&path)
            .assert()
            .success()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::is_empty());
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(magic), "{name}");
        assert_eq!(scan(&bytes), "https://example.com/raster", "{name}");
    }
    assert_eq!(
        leftovers(
            dir.path(),
            &["code.png", "code.PNG", "code.jpg", "code.jpeg", "code.webp"]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn styled_raster_output_decodes() {
    for format in ["png", "jpeg", "webp"] {
        for style in [
            &["--fg", "#1a237e", "--bg", "ivory"][..],
            &["--transparent", "--fg", "darkgreen"],
            &["--invert"],
            &["--size", "257", "--margin", "2"],
        ] {
            let out = qr()
                .args(["-q", "-f", format, "styled"])
                .args(style)
                .output()
                .unwrap();
            assert!(out.status.success(), "{format} {style:?}: {out:?}");
            if style == ["--invert"] {
                // rqrr reads dark modules as dark, so an inverted code is
                // checked for its colours instead.
                let img = image::load_from_memory(&out.stdout).unwrap().to_luma8();
                assert!(
                    img.get_pixel(0, 0).0[0] < 32,
                    "{format}: quiet zone is dark"
                );
                continue;
            }
            assert_eq!(scan(&out.stdout), "styled", "{format} {style:?}");
        }
    }
}

#[test]
fn size_sets_the_exact_image_dimensions() {
    for format in ["png", "jpeg", "webp"] {
        let out = qr()
            .args(["-f", format, "--size", "300", "hello"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let img = image::load_from_memory(&out.stdout).unwrap();
        assert_eq!((img.width(), img.height()), (300, 300), "{format}");
    }
    qr().args(["-f", "png", "--size", "300", "--scale", "3", "x"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn png_data_uri_holds_the_image() {
    let out = qr()
        .args(["--data-uri", "-f", "png", "hello"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let uri = String::from_utf8(out.stdout).unwrap();
    let b64 = uri
        .strip_prefix("data:image/png;base64,")
        .unwrap()
        .trim_end();
    let png = qr().args(["-f", "png", "hello"]).output().unwrap().stdout;
    assert_eq!(b64, base64(&png));
    assert_eq!(uri.lines().count(), 1);
}

/// Standard padded base64, to check the CLI's own encoder independently.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn dpi_is_recorded_in_png_and_jpeg() {
    let png = qr()
        .args(["-f", "png", "--dpi", "300", "hello"])
        .output()
        .unwrap()
        .stdout;
    // pHYs: 11,811 pixels per metre on both axes, unit 1 (metre).
    let at = png.windows(4).position(|w| w == b"pHYs").unwrap();
    assert_eq!(
        &png[at + 4..at + 13],
        &[0, 0, 0x2e, 0x23, 0, 0, 0x2e, 0x23, 1]
    );
    let jpeg = qr()
        .args(["-f", "jpeg", "--dpi", "300", "hello"])
        .output()
        .unwrap()
        .stdout;
    let at = jpeg.windows(5).position(|w| w == b"JFIF\0").unwrap();
    assert_eq!(&jpeg[at + 7..at + 12], &[1, 1, 44, 1, 44]);
}

#[test]
fn raster_bytes_go_to_a_piped_stdout() {
    // Not a terminal here, so stdout gets the bytes.
    let out = qr().args(["-f", "webp", "hello"]).output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"RIFF"));
}

#[cfg(unix)]
#[test]
fn overwriting_a_private_file_keeps_it_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keep.svg");
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    qr().arg("hello").arg("-o").arg(&path).assert().success();
    assert!(fs::read_to_string(&path).unwrap().contains("<svg"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_is_refused_not_replaced() {
    use std::os::unix::fs::FileTypeExt as _;
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe.svg");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    qr().arg("hello")
        .arg("-o")
        .arg(&fifo)
        .assert()
        .code(4)
        .stderr(predicate::str::contains("is not a regular file"))
        .stderr(predicate::str::contains("use -o - for stdout"));
    assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo());
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
fn data_uri_into_a_txt_file_needs_no_format() {
    let svg = qr().args(["-f", "svg", "hello"]).output().unwrap().stdout;
    let expected = format!(
        "data:image/svg+xml;base64,{}\n",
        qr_cli::output::base64(&svg)
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("uri.txt");
    qr().args(["hello", "--data-uri", "-o"])
        .arg(&path)
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
}

#[test]
fn data_uri_into_an_image_file_warns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code.svg");
    qr().args(["hello", "--data-uri", "-o"])
        .arg(&path)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "which will not be a valid svg file",
        ))
        .stderr(predicate::str::contains(
            "hint: use -o code.txt, or drop --data-uri",
        ));
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .starts_with("data:image/svg+xml;base64,")
    );

    // The warning has a stable code in --json mode.
    let out = qr()
        .args(["hello", "--data-uri", "--json", "-o"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(v["warning"]["code"], "data_uri_in_image_file");
}

#[test]
fn no_clobber_to_stdout_warns() {
    qr().args(["hello", "--no-clobber", "-o", "-"])
        .assert()
        .success()
        .stderr("warning: --no-clobber has no effect when writing to stdout\n");
}

#[cfg(unix)]
#[test]
fn json_escapes_terminal_controls_in_the_output_path() {
    use std::os::unix::ffi::OsStrExt as _;
    let dir = tempfile::tempdir().unwrap();
    // CSI (C1), DEL and a right-to-left override in a file name.
    let name = "a\u{9b}31m\u{7f}\u{202e}gvs.svg";
    let path = dir.path().join(name);
    let out = qr()
        .args(["hello", "--json", "-o"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let text = std::str::from_utf8(&out.stdout).unwrap();
    assert!(
        !text
            .chars()
            .any(|c| matches!(c, '\u{7f}'..='\u{9f}' | '\u{202a}'..='\u{202e}')),
        "{text:?}"
    );
    assert!(
        text.contains("\\u009b") && text.contains("\\u202e"),
        "{text}"
    );
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        v["output"].as_str().unwrap().as_bytes(),
        path.as_os_str().as_bytes()
    );
    assert!(path.exists());
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
