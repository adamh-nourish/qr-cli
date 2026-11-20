//! In-process tests of `qr batch` through [`run`](crate::run).

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{Context, Env, ExitStatus, run};

struct Outcome {
    status: ExitStatus,
    out: String,
    err: String,
}

fn run_with(args: &[&str], stdin: &[u8], stdout: &mut dyn Write) -> (ExitStatus, String) {
    let mut stdin = stdin;
    let mut err = Vec::new();
    let mut command_line = vec!["qr", "batch"];
    command_line.extend_from_slice(args);
    let status = run(
        command_line,
        &mut Context {
            stdin: &mut stdin,
            stdout,
            stderr: &mut err,
            env: Env::default(),
        },
    );
    (status, String::from_utf8(err).unwrap())
}

fn batch(args: &[&str], stdin: &[u8]) -> Outcome {
    let mut out = Vec::new();
    let (status, err) = run_with(args, stdin, &mut out);
    Outcome {
        status,
        out: String::from_utf8(out).unwrap(),
        err,
    }
}

/// A temporary directory holding `rows` as `rows.csv`.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(rows: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rows.csv"), rows).unwrap();
        Self { dir }
    }

    fn input(&self) -> String {
        self.path("rows.csv")
    }

    fn out(&self) -> String {
        self.path("out")
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).to_str().unwrap().to_owned()
    }

    /// The files in `out`, sorted.
    fn written(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir.path().join("out"))
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn run(&self, extra: &[&str]) -> Outcome {
        let input = self.input();
        let out = self.out();
        let mut args = vec![input.as_str(), "--out-dir", out.as_str()];
        args.extend_from_slice(extra);
        batch(&args, b"")
    }
}

fn json_lines(out: &str) -> Vec<Value> {
    out.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn csv_rows_become_files_named_from_the_filename_column() {
    let f = Fixture::new("data,filename\nhttps://a.example,alpha\nhello,beta.png\n");
    let o = f.run(&[]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!((o.out.as_str(), o.err.as_str()), ("", ""));
    assert_eq!(f.written(), ["alpha.png", "beta.png"]);
    let png = fs::read(f.dir.path().join("out/alpha.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn lines_mode_with_a_template_and_json_report() {
    let f = Fixture::new("one\n\ntwo\r\nthree");
    let o = f.run(&["--lines", "--name", "code-{n:03}", "-f", "svg", "--json"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(
        f.written(),
        ["code-001.svg", "code-002.svg", "code-003.svg"]
    );
    let lines = json_lines(&o.out);
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["row"], 1);
    assert_eq!(lines[1]["line"], 3);
    assert_eq!(lines[2]["line"], 4);
    assert_eq!(lines[2]["format"], "svg");
    assert_eq!(lines[2]["bytes"], 5);
    let expected = Path::new(&f.out()).join("code-003.svg");
    assert_eq!(lines[2]["output"], expected.to_str().unwrap());
    assert!(lines[0].get("error").is_none());
    assert_eq!(
        lines[3],
        serde_json::json!({"summary": {"rows": 3, "written": 3, "failed": 0, "skipped": 0}})
    );
}

#[test]
fn stdin_is_read_with_a_dash_and_never_from_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("o");
    let out = out.to_str().unwrap();
    let o = batch(&["-", "--out-dir", out, "--lines"], b"a\nb\n");
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(fs::read_dir(out).unwrap().count(), 2);

    let mut stdin: &[u8] = b"a\n";
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = run(
        ["qr", "batch", "-", "--out-dir", out],
        &mut Context {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: Env {
                stdin_is_tty: true,
                ..Env::default()
            },
        },
    );
    assert_eq!(status, ExitStatus::Usage);
    assert!(
        String::from_utf8(stderr)
            .unwrap()
            .contains("stdin is a terminal")
    );
}

#[test]
fn text_formats_write_txt_files_and_terminal_is_refused() {
    let f = Fixture::new("hi\n");
    let o = f.run(&["-f", "ascii", "--margin", "1", "--verify"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(f.written(), ["1.txt"]);
    let text = fs::read_to_string(f.dir.path().join("out/1.txt")).unwrap();
    assert!(text.starts_with("  ") && text.contains("##"), "{text}");

    let o = f.run(&["-f", "text", "--force", "--fg", "navy"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert!(
        o.err.contains("--fg only applies to image formats"),
        "{}",
        o.err
    );
    assert!(o.err.contains("below the 4-module quiet zone") || !o.err.contains("margin"));

    // clap refuses it, and the batch help does not offer it.
    let o = f.run(&["-f", "terminal"]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert!(
        o.err
            .contains("invalid value 'terminal' for '--format <FORMAT>'"),
        "{}",
        o.err
    );
    assert!(!o.err.contains("[possible values: terminal"), "{}", o.err);
    let o = f.run(&["-f", "jpg", "--force"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
}

#[test]
fn settings_warnings_are_given_once_not_per_row() {
    let f = Fixture::new("a\nb\nc\n");
    let o = f.run(&[
        "--margin",
        "1",
        "--fg",
        "#aaa",
        "--no-verify",
        "--dpi",
        "300",
        "-f",
        "svg",
    ]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert_eq!(
        o.err.matches("warning: a margin of 1").count(),
        1,
        "{}",
        o.err
    );
    assert_eq!(o.err.matches("contrast ratio").count(), 1, "{}", o.err);
    assert_eq!(o.err.matches("--dpi has no effect").count(), 1, "{}", o.err);
}

#[test]
fn verbose_notes_cover_the_run_and_each_row() {
    let f = Fixture::new("a\nb\n");
    let o = f.run(&["-v", "--shape", "dots", "-j", "2"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert!(o.err.contains("note: encoding 2 rows from '"), "{}", o.err);
    assert!(o.err.contains("as png with 2 threads"), "{}", o.err);
    assert_eq!(o.err.matches("note: verified: the png output").count(), 2);
    assert!(o.err.contains("note: row 1: wrote '"), "{}", o.err);
    let verified = o.err.find("row 1: wrote").unwrap();
    let second = o.err.find("row 2: wrote").unwrap();
    assert!(verified < second, "{}", o.err);
    assert!(
        o.err
            .ends_with(&format!("note: wrote 2 files to '{}'\n", f.out()))
    );
}

#[test]
fn existing_files_need_force() {
    let f = Fixture::new("a\nb\n");
    fs::create_dir(f.out()).unwrap();
    fs::write(f.dir.path().join("out/2.png"), b"keep").unwrap();
    let o = f.run(&[]);
    assert_eq!(o.status, ExitStatus::Io);
    let shown = Path::new(&f.out()).join("2.png");
    assert_eq!(
        o.err,
        format!(
            "error: row 2 (line 2): '{}' already exists\n  \
             hint: pass --force to replace existing files\n\
             error: 1 of 2 rows failed; 1 written to '{}'\n",
            shown.display(),
            f.out()
        )
    );
    assert_eq!(fs::read(&shown).unwrap(), b"keep");
    let o = f.run(&["--force"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    assert!(fs::read(&shown).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn failing_rows_are_reported_and_the_rest_written() {
    let long = "x".repeat(3000);
    let f = Fixture::new(&format!("data\nok\n\"\"\n{long}\nfine\n"));
    let o = f.run(&["--json", "--ec", "H"]);
    // The first failure (an empty row) decides the status.
    assert_eq!(o.status, ExitStatus::Usage, "{}", o.err);
    assert_eq!(f.written(), ["1.png", "4.png"]);
    let lines = json_lines(&o.out);
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[1]["error"]["code"], "empty_input");
    assert!(lines[1].get("output").is_none());
    assert_eq!(lines[2]["error"]["code"], "data_too_long");
    assert!(lines[2]["output"].as_str().unwrap().ends_with("3.png"));
    assert_eq!(lines[4]["summary"]["failed"], 2);
    let errors = json_lines(&o.err);
    assert_eq!(errors.len(), 3);
    assert_eq!(
        errors[0]["error"]["message"],
        "row 2 (line 3): the data is empty"
    );
    assert_eq!(errors[2]["error"]["code"], "batch_incomplete");

    // Only the data-too-long row: exit 3.
    let f = Fixture::new(&format!("{long}\n"));
    let o = f.run(&["--ec", "H", "--lines"]);
    assert_eq!(o.status, ExitStatus::DataTooLong, "{}", o.err);
}

#[test]
fn fail_fast_stops_at_the_first_failure() {
    let mut rows = String::from("data,filename\n");
    for i in 0..10 {
        writeln!(rows, "d{i},f{i}").unwrap();
    }
    rows.push_str("dup,f3\n");
    for i in 10..3000 {
        writeln!(rows, "d{i},f{i}").unwrap();
    }
    let f = Fixture::new(&rows);
    let o = f.run(&["--fail-fast", "-j", "1", "-f", "svg", "--json"]);
    assert_eq!(o.status, ExitStatus::Usage, "{}", o.err);
    assert_eq!(f.written().len(), 10);
    let lines = json_lines(&o.out);
    assert_eq!(lines.len(), 12);
    assert_eq!(lines[10]["error"]["code"], "duplicate_name");
    assert_eq!(
        lines[11]["summary"],
        serde_json::json!({"rows": 3001, "written": 10, "failed": 1, "skipped": 2990})
    );
    // Without --fail-fast, the rest are written.
    let o = f.run(&["-f", "svg", "--force"]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert_eq!(f.written().len(), 3000);
    assert!(
        o.err.contains("1 of 3001 rows failed; 3000 written"),
        "{}",
        o.err
    );
}

#[test]
fn fail_fast_during_rendering_skips_rows_not_started() {
    let long = "x".repeat(3000);
    let mut rows = format!("a\n{long}\n");
    rows.push_str(&"b\n".repeat(50));
    let f = Fixture::new(&rows);
    let o = f.run(&[
        "--lines",
        "--ec",
        "H",
        "--fail-fast",
        "-j",
        "1",
        "--name",
        "{hash}",
    ]);
    assert_eq!(o.status, ExitStatus::DataTooLong, "{}", o.err);
    // Duplicate names after the failure are never reached.
    assert!(!o.err.contains("duplicate_name"));
    assert!(
        o.err
            .contains("and 50 were skipped (--fail-fast); 1 written"),
        "{}",
        o.err
    );
    assert_eq!(f.written().len(), 1);
}

#[test]
fn results_are_reported_in_row_order_across_chunks() {
    let rows: String = (0..2100).map(|i| i.to_string() + "\n").collect();
    let f = Fixture::new(&rows);
    let o = f.run(&[
        "--lines", "-f", "eps", "-j", "4", "--json", "--name", "{data}",
    ]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    let lines = json_lines(&o.out);
    assert_eq!(lines.len(), 2101);
    for (i, line) in lines[..2100].iter().enumerate() {
        assert_eq!(line["row"], i + 1);
        let output = line["output"].as_str().unwrap();
        assert!(output.ends_with(&format!("{i}.eps")), "{output}");
    }
    assert_eq!(f.written().len(), 2100);
}

#[test]
fn whole_batch_problems_stop_before_anything_is_written() {
    let f = Fixture::new("data,filename\nx,y\n");
    for (args, status, code) in [
        (
            &["--name", "{nope}"][..],
            ExitStatus::Usage,
            "unknown field '{nope}'",
        ),
        (
            &["--name", "a/{n}"],
            ExitStatus::Usage,
            "--name cannot contain '/'",
        ),
        (
            &["-f", "pdf", "--logo", "missing.png"],
            ExitStatus::Usage,
            "--logo needs",
        ),
    ] {
        let o = f.run(args);
        assert_eq!(o.status, status, "{args:?}: {}", o.err);
        assert!(o.err.contains(code), "{args:?}: {}", o.err);
        assert!(!Path::new(&f.out()).exists(), "{args:?}");
    }
    let o = f.run(&["--logo", "missing.png"]);
    assert_eq!(o.status, ExitStatus::Io, "{}", o.err);

    let empty = Fixture::new("data\n");
    let o = empty.run(&[]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert!(
        o.err.contains("rows.csv' has no rows to encode"),
        "{}",
        o.err
    );

    let missing = batch(&["/no/such/rows.csv", "--out-dir", "x"], b"");
    assert_eq!(missing.status, ExitStatus::Io);
    assert!(
        missing.err.contains("cannot read '/no/such/rows.csv'"),
        "{}",
        missing.err
    );
    assert!(
        missing.err.contains("hint: check the FILE path"),
        "{}",
        missing.err
    );
}

#[test]
fn an_out_dir_that_is_a_file_is_refused() {
    let f = Fixture::new("x\n");
    fs::write(f.out(), b"file").unwrap();
    let o = f.run(&[]);
    assert_eq!(o.status, ExitStatus::Io);
    assert!(o.err.contains("cannot use output directory"), "{}", o.err);
}

#[cfg(unix)]
#[test]
fn unwritable_directories_fail_each_row() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new("x\ny\n");
    fs::create_dir(f.out()).unwrap();
    fs::set_permissions(f.out(), fs::Permissions::from_mode(0o555)).unwrap();
    let o = f.run(&["-f", "svg"]);
    fs::set_permissions(f.out(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.status, ExitStatus::Io, "{}", o.err);
    assert_eq!(o.err.matches("permission denied").count(), 2, "{}", o.err);
}

/// A writer that accepts nothing.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn a_closed_stdout_is_an_output_error() {
    let f = Fixture::new("x\n");
    let (input, out) = (f.input(), f.out());
    for extra in [&["--json"][..], &[]] {
        let mut args = vec![input.as_str(), "--out-dir", out.as_str(), "--force"];
        args.extend_from_slice(extra);
        let (status, err) = run_with(&args, b"", &mut Closed);
        assert_eq!(status, ExitStatus::Io, "{extra:?}");
        assert!(err.contains("the pipe was closed"), "{err}");
    }
}

#[test]
fn csv_columns_feed_the_template() {
    let f = Fixture::new("id,data,team\n7,x,eng\n8,y,ops\n");
    let o = f.run(&["--name", "{team}-{id}-{hash}"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    let written = f.written();
    assert!(written[0].starts_with("eng-7-") && written[1].starts_with("ops-8-"));
    assert_eq!(written[0].len(), "eng-7-".len() + 16 + ".png".len());
    let p = PathBuf::from(f.out());
    assert!(p.join(&written[0]).is_file());
}

/// `e` with an acute accent, composed (NFC) and decomposed (NFD).
const E_NFC: &str = "\u{e9}";
const E_NFD: &str = "e\u{301}";

#[test]
fn names_that_differ_only_in_normalization_clash() {
    // APFS and HFS+ store both spellings as one file, so the second row
    // must fail, the same way every time, with or without --force.
    let rows = format!(
        "data,filename\none,{E_NFC}\ntwo,{E_NFD}\nthree,{}\n",
        "\u{c9}"
    );
    let f = Fixture::new(&rows);
    for extra in [&[][..], &["--force"][..]] {
        let mut args = vec!["-f", "svg", "--json"];
        args.extend_from_slice(extra);
        let o = f.run(&args);
        assert_eq!(o.status, ExitStatus::Usage, "{extra:?}: {}", o.err);
        let lines = json_lines(&o.out);
        assert_eq!(lines[0]["error"], Value::Null, "{extra:?}");
        for row in [1, 2] {
            assert_eq!(lines[row]["error"]["code"], "duplicate_name", "{extra:?}");
            let message = lines[row]["error"]["message"].as_str().unwrap();
            assert!(message.ends_with("is also the file of row 1"), "{message}");
        }
        assert_eq!(
            lines[3]["summary"],
            serde_json::json!({"rows": 3, "written": 1, "failed": 2, "skipped": 0})
        );
        assert_eq!(f.written().len(), 1, "{extra:?}");
        // The one file written holds row 1.
        let svg = fs::read(f.dir.path().join("out").join(&f.written()[0])).unwrap();
        assert!(svg.starts_with(b"<?xml") || svg.starts_with(b"<svg"));
    }
}

#[test]
fn name_keys_fold_case_and_normalization() {
    use super::name_key;
    assert_eq!(name_key("A.PNG"), "a.png");
    assert_eq!(name_key(E_NFD), E_NFC);
    assert_eq!(name_key("\u{c9}"), E_NFC);
    assert_eq!(name_key("E\u{301}.png"), format!("{E_NFC}.png"));
    assert_ne!(name_key("\u{e8}"), name_key(E_NFC));
}

#[test]
fn stdin_on_a_terminal_names_the_batch_remedy() {
    let mut stdin: &[u8] = b"";
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let status = run(
        ["qr", "batch", "-", "--out-dir", "unused"],
        &mut Context {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: Env {
                stdin_is_tty: true,
                ..Env::default()
            },
        },
    );
    assert_eq!(status, ExitStatus::Usage);
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "error: no input: stdin is a terminal\n  \
         hint: pipe the CSV in (... | qr batch - --out-dir DIR) or pass a FILE\n"
    );
    assert!(!Path::new("unused").exists());
}

#[test]
fn each_hint_is_given_once_per_batch() {
    let f = Fixture::new("a\nb\nc\n");
    fs::create_dir(f.out()).unwrap();
    for name in ["1.png", "2.png", "3.png"] {
        fs::write(f.dir.path().join("out").join(name), b"keep").unwrap();
    }
    let o = f.run(&["-j", "1"]);
    assert_eq!(o.status, ExitStatus::Io);
    assert_eq!(o.err.matches("already exists").count(), 3, "{}", o.err);
    assert_eq!(o.err.matches("hint: pass --force").count(), 1, "{}", o.err);
}

#[test]
fn an_out_dir_that_is_a_file_is_not_a_directory() {
    let f = Fixture::new("a\n");
    let o = batch(&[&f.input(), "--out-dir", &f.input()], b"");
    assert_eq!(o.status, ExitStatus::Io);
    assert!(
        o.err.starts_with(&format!(
            "error: cannot use output directory '{}': it is not a directory\n",
            f.input()
        )),
        "{}",
        o.err
    );
    assert!(!o.err.contains("os error"), "{}", o.err);
}

#[test]
fn rows_over_several_chunks_are_reported_in_order() {
    let rows = super::CHUNK * 2 + 7;
    let mut input = String::new();
    for i in 1..=rows {
        writeln!(input, "row {i}").unwrap();
    }
    let f = Fixture::new(&input);
    let o = f.run(&["--lines", "-f", "text", "--json", "-j", "4"]);
    assert_eq!(o.status, ExitStatus::Success, "{}", o.err);
    let lines = json_lines(&o.out);
    assert_eq!(lines.len(), rows + 1);
    for (i, line) in lines[..rows].iter().enumerate() {
        assert_eq!(line["row"], i + 1);
        assert!(
            line["output"]
                .as_str()
                .unwrap()
                .ends_with(&format!("{}.txt", i + 1))
        );
    }
    assert_eq!(f.written().len(), rows);
    let text = fs::read_to_string(f.dir.path().join(format!("out/{rows}.txt"))).unwrap();
    assert_ne!(text.trim(), "");
}

#[test]
fn rows_with_too_many_cells_are_refused_before_anything_is_written() {
    let mut rows = String::from("data,filename\nok,a\n");
    rows.push_str(&",".repeat(super::MAX_COLUMNS));
    rows.push('\n');
    let f = Fixture::new(&rows);
    let o = f.run(&[]);
    assert_eq!(o.status, ExitStatus::Usage);
    assert_eq!(
        o.err,
        format!(
            "error: '{}' has more than 1000 columns on line 3\n  \
             hint: keep only the columns the batch uses (data, filename and those --name names)\n",
            f.input()
        )
    );
    assert_eq!(f.written().len(), 0);
}
