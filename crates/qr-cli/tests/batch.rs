//! End-to-end tests of `qr batch` against the real `qr` binary, including
//! the path-traversal acceptance tests of DECISIONS section 4.7: whatever a
//! row holds, every file lands directly inside `--out-dir` and nothing
//! outside it is created or changed.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn qr() -> Command {
    let mut cmd = cargo_bin_cmd!("qr");
    cmd.env_remove("NO_COLOR").env_remove("CLICOLOR_FORCE");
    cmd
}

/// Every path under `root`, relative to it.
fn tree(root: &Path) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                stack.push(path.clone());
            }
            paths.insert(path.strip_prefix(root).unwrap().to_owned());
        }
    }
    paths
}

/// Writes `csv` to `root/work/rows.csv`, runs `qr batch` into
/// `root/work/out` and returns the names written there. Fails if anything
/// appeared anywhere else under `root`.
fn contained(csv: &str, extra: &[&str]) -> (Vec<String>, String) {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("rows.csv"), csv).unwrap();
    let before = tree(root.path());
    let output = qr()
        .current_dir(&work)
        .args(["batch", "rows.csv", "--out-dir", "out", "-f", "svg"])
        .args(extra)
        .output()
        .unwrap();
    let after = tree(root.path());
    let mut names = Vec::new();
    for path in after.difference(&before) {
        assert!(
            path.starts_with("work/out"),
            "{} was created",
            path.display()
        );
        let rest = path.strip_prefix("work/out").unwrap();
        if rest.as_os_str().is_empty() {
            continue;
        }
        assert_eq!(rest.components().count(), 1, "{}", path.display());
        names.push(rest.to_string_lossy().into_owned());
    }
    (names, String::from_utf8(output.stderr).unwrap())
}

#[test]
fn dot_dot_and_absolute_names_stay_inside_out_dir() {
    let csv = "data,filename\n\
               a,../escaped\n\
               b,../../../../../../tmp/escaped\n\
               c,/etc/passwd\n\
               d,/\n\
               e,sub/dir/file\n\
               f,..\n\
               g,.\n\
               h,./x/../../y\n";
    let (names, err) = contained(csv, &[]);
    assert_eq!(
        names.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "escaped.svg",
            "tmpescaped.svg",
            "etcpasswd.svg",
            "subdirfile.svg",
            "x....y.svg",
        ]),
        "{err}"
    );
    // '/', '..' and '.' leave nothing: refused, not written elsewhere.
    assert_eq!(err.matches("the file name is empty").count(), 3, "{err}");
}

#[test]
fn windows_drive_letters_and_unc_paths_stay_inside_out_dir() {
    let csv = "data,filename\n\
               a,C:\\Windows\\System32\\evil\n\
               b,C:evil\n\
               c,\\\\server\\share\\evil\n\
               d,\\\\?\\C:\\evil\n\
               e,file.txt:ads\n";
    let (names, err) = contained(csv, &[]);
    assert_eq!(
        names.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "CWindowsSystem32evil.svg",
            "Cevil.svg",
            "servershareevil.svg",
            "file.txtads.svg",
        ]),
        "{err}"
    );
    assert!(
        err.contains("row 4 (line 5)") && err.contains("is also the file of row 2"),
        "{err}"
    );
}

#[test]
fn windows_reserved_names_are_refused() {
    let csv = "data,filename\na,CON\nb,nul.txt\nc,com1\nd,LPT9\ne,aux \nf,console\n";
    let (names, err) = contained(csv, &[]);
    assert_eq!(names, ["console.svg"], "{err}");
    assert_eq!(
        err.matches("is a reserved device name on Windows").count(),
        5,
        "{err}"
    );
}

#[test]
fn unicode_separators_and_controls_cannot_build_a_path() {
    let csv = "data,filename\n\
               a,..\u{2215}..\u{2215}evil1\n\
               b,..\u{FF0F}evil2\n\
               c,..\u{29F8}evil3\n\
               d,..\u{2044}evil4\n\
               e,..\u{FF3C}evil5\n\
               f,a\u{2028}b\u{2029}c\n\
               g,\u{202E}gnp.exe\n\
               h,\"new\nline\"\n";
    let (names, err) = contained(csv, &[]);
    assert_eq!(
        names.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "evil1.svg",
            "evil2.svg",
            "evil3.svg",
            "evil4.svg",
            "evil5.svg",
            "abc.svg",
            "gnp.exe.svg",
            "newline.svg",
        ]),
        "{err}"
    );
}

#[test]
fn hidden_and_option_like_names_lose_their_prefix() {
    let csv = "data,filename\na,.bashrc\nb,-rf\nc,--help\nd,...\n";
    let (names, err) = contained(csv, &[]);
    assert_eq!(
        names.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["bashrc.svg", "rf.svg", "help.svg"]),
        "{err}"
    );
}

#[test]
fn data_in_the_name_template_is_sanitised_too() {
    let (names, err) = contained("../../x\n/abs\n", &["--lines", "--name", "{data}"]);
    assert_eq!(
        names.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["x.svg", "abs.svg"]),
        "{err}"
    );
}

#[test]
fn templates_with_separators_are_refused_before_anything_is_written() {
    for template in [
        "../{n}",
        "/tmp/{n}",
        "a/b{n}",
        "C:{n}",
        "a\\{n}",
        "x\u{2215}{n}",
    ] {
        let (names, err) = contained("a\n", &["--lines", "--name", template]);
        assert!(names.is_empty(), "{template}");
        assert!(err.contains("--name cannot contain"), "{template}: {err}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_in_out_dir_are_never_followed() {
    let root = tempfile::tempdir().unwrap();
    let victim = root.path().join("victim.svg");
    fs::write(&victim, "precious").unwrap();
    let out = root.path().join("out");
    fs::create_dir(&out).unwrap();
    std::os::unix::fs::symlink(&victim, out.join("evil.svg")).unwrap();
    let rows = root.path().join("rows.csv");
    fs::write(&rows, "data,filename\nx,evil\n").unwrap();
    let run = |force: bool| {
        let mut cmd = qr();
        cmd.arg("batch").arg(&rows).arg("--out-dir").arg(&out);
        cmd.args(["-f", "svg"]);
        if force {
            cmd.arg("--force");
        }
        cmd.output().unwrap()
    };
    let refused = run(false);
    assert_eq!(refused.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("already exists"));
    let replaced = run(true);
    assert!(replaced.status.success(), "{replaced:?}");
    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
    assert!(
        !fs::symlink_metadata(out.join("evil.svg"))
            .unwrap()
            .is_symlink()
    );
}

#[test]
fn written_codes_decode_back_to_their_rows() {
    let dir = tempfile::tempdir().unwrap();
    let rows = dir.path().join("rows.csv");
    fs::write(
        &rows,
        "id,data\n1,https://example.com/a\n2,\"comma, and \"\"quote\"\"\"\n3,ünïcode\n",
    )
    .unwrap();
    let out = dir.path().join("out");
    qr().arg("batch")
        .arg(&rows)
        .arg("--out-dir")
        .arg(&out)
        .args(["--name", "row-{id}", "--shape", "dots", "-j", "2"])
        .assert()
        .success()
        .stdout("")
        .stderr("");
    for (id, data) in [
        (1, "https://example.com/a"),
        (2, "comma, and \"quote\""),
        (3, "ünïcode"),
    ] {
        qr().arg("decode")
            .arg(out.join(format!("row-{id}.png")))
            .assert()
            .success()
            .stdout(format!("{data}\n"));
    }
}

/// `qr batch --help`, pinned. clap turns a literal `{n}` in help text into
/// a newline, so the text must never contain one.
#[test]
fn batch_help_is_stable() {
    let output = qr()
        .args(["batch", "--help"])
        .env("COLUMNS", "100")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!("batch_help", help);
    let short = qr()
        .args(["batch", "-h"])
        .env("COLUMNS", "100")
        .output()
        .unwrap();
    let short = String::from_utf8(short.stdout).unwrap();
    for text in [&help, &short] {
        assert!(text.contains("{n:04} (the"), "{text}");
        assert!(!text.contains("otherwise \n"), "{text}");
        assert!(!text.contains("terminal:"), "{text}");
    }
}

#[test]
fn help_describes_templates_and_limits() {
    qr().args(["batch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("{n:04}"))
        .stdout(predicate::str::contains("1000 cells per row"))
        .stdout(predicate::str::contains("--fail-fast"))
        .stdout(predicate::str::contains("--shape"));
}
