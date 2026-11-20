//! The README is the user guide, so it is tested like code.
//!
//! - Every long option that `qr --help` shows, for the command and for each
//!   subcommand, is mentioned in README.md, and every subcommand is named, so
//!   an option cannot ship undocumented.
//! - Every relative link in README.md names a file that exists, and every
//!   `#anchor` names a heading.
//! - On Unix, every fenced block tagged exactly `sh` runs, in order, with
//!   `qr` on `PATH`, in one scratch directory seeded with the fixture files the
//!   examples name. Blocks tagged `sh norun` (installers, and commands that
//!   write outside the scratch directory) are not run.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

/// The repository root, two levels above this crate.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/qr-cli sits two levels below the workspace root")
        .to_path_buf()
}

fn readme() -> String {
    let path = workspace_root().join("README.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// One fenced code block of a Markdown file.
#[derive(Debug)]
struct Block {
    /// The whole info string after the opening fence, such as `sh norun`.
    info: String,
    /// The block's lines, without the fences, each ending in a newline.
    body: String,
    /// The line of the opening fence, for error messages (1-based).
    line: usize,
}

/// The headings (outside code blocks) and the fenced code blocks of a
/// Markdown document.
fn parse_markdown(text: &str) -> (Vec<String>, Vec<Block>) {
    let mut headings = Vec::new();
    let mut blocks = Vec::new();
    let mut open: Option<Block> = None;
    for (index, line) in text.lines().enumerate() {
        if let Some(block) = open.as_mut() {
            if line.trim_end() == "```" {
                blocks.push(open.take().unwrap());
            } else {
                block.body.push_str(line);
                block.body.push('\n');
            }
            continue;
        }
        if let Some(info) = line.strip_prefix("```") {
            open = Some(Block {
                info: info.trim().to_string(),
                body: String::new(),
                line: index + 1,
            });
        } else if line.starts_with('#') {
            headings.push(line.trim_start_matches('#').trim().to_string());
        }
    }
    assert!(open.is_none(), "README.md has an unclosed code fence");
    (headings, blocks)
}

/// The anchor GitHub gives a heading: lower case, spaces to hyphens, and
/// punctuation dropped.
fn anchor_of(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c.to_ascii_lowercase())
            } else if c == ' ' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// Every `](target)` link target, without titles.
fn link_targets(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else { break };
        let target = &after[..end];
        if !target.contains(' ') && !target.is_empty() {
            targets.push(target);
        }
        rest = &after[end..];
    }
    targets
}

/// The long options (`--name`) in a help text.
fn long_options(help: &str) -> BTreeSet<&str> {
    let bytes = help.as_bytes();
    let is_name = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let at_word_start =
            i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-');
        if at_word_start
            && bytes[i] == b'-'
            && bytes[i + 1] == b'-'
            && bytes[i + 2].is_ascii_lowercase()
        {
            let mut end = i + 2;
            while end < bytes.len() && is_name(bytes[end]) {
                end += 1;
            }
            found.insert(&help[i..end]);
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// Whether `text` contains `token` as a whole option, not as the start of a
/// longer one (`--logo` is not mentioned by `--logo-scale` alone).
fn mentions(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(at, _)| {
        text[at + token.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-'))
    })
}

/// The output of `qr ARGS --help`, wrapped at a fixed width like the other
/// integration tests.
fn help_of(args: &[&str]) -> String {
    let output = cargo_bin_cmd!("qr")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env("COLUMNS", "100")
        .args(args)
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "qr {args:?} --help failed: {output:?}"
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The subcommand names listed under `Commands:` in the top-level help,
/// except `help`, which clap adds itself.
fn subcommands(top_help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in top_help.lines() {
        if line == "Commands:" {
            in_commands = true;
        } else if in_commands {
            if line.trim().is_empty() {
                break;
            }
            if let (true, Some(name)) = (line.starts_with("  "), line.split_whitespace().next()) {
                names.push(name.to_string());
            }
        }
    }
    names.retain(|name| name != "help");
    names
}

#[test]
fn every_option_in_the_help_is_documented() {
    let readme = readme();
    let top = help_of(&[]);
    let mut pages = vec![("qr".to_string(), top.clone())];
    for name in subcommands(&top) {
        pages.push((format!("qr {name}"), help_of(&[name.as_str()])));
    }
    assert!(
        pages.len() >= 14,
        "expected the top level and at least thirteen subcommands"
    );

    let mut missing = Vec::new();
    for (page, help) in &pages {
        for option in long_options(help) {
            if option == "--help" || option == "--version" {
                continue;
            }
            if !mentions(&readme, option) {
                missing.push(format!("{option} (from `{page} --help`)"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "README.md does not document: {missing:#?}"
    );
}

#[test]
fn every_subcommand_has_an_example_in_the_readme() {
    let readme = readme();
    let top = help_of(&[]);
    let missing: Vec<String> = subcommands(&top)
        .into_iter()
        .filter(|name| !readme.contains(&format!("qr {name}")))
        .collect();
    assert!(
        missing.is_empty(),
        "README.md has no `qr NAME` example for: {missing:?}"
    );
}

#[test]
fn readme_links_resolve() {
    let text = readme();
    let (headings, _) = parse_markdown(&text);
    let anchors: BTreeSet<String> = headings.iter().map(|h| anchor_of(h)).collect();
    let root = workspace_root();

    for target in link_targets(&text) {
        if target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
        {
            continue;
        }
        if let Some(anchor) = target.strip_prefix('#') {
            assert!(
                anchors.contains(anchor),
                "README.md links to #{anchor}, which is not a heading"
            );
            continue;
        }
        let path = target.split_once('#').map_or(target, |(path, _)| path);
        assert!(
            root.join(path).is_file(),
            "README.md links to {path}, which is not a file in the repository"
        );
    }
}

#[test]
fn readme_states_the_exit_codes_the_binary_documents() {
    // The exit-code line of the top-level help is the contract; the README
    // table must give each code the same meaning.
    let readme = readme();
    let top = help_of(&[]);
    assert!(
        top.contains("6 verification failed"),
        "help text changed: {top}"
    );
    for line in [
        "| 0 | OK |",
        "| 1 | Internal error |",
        "| 2 | Usage error",
        "| 3 | Data too long",
        "| 4 | I/O error",
        "| 5 | No code found",
        "| 6 | Verification failed",
    ] {
        assert!(
            readme.contains(line),
            "README.md exit-code table lacks `{line}`"
        );
    }
}

#[test]
fn anchor_of_matches_github_slugs() {
    assert_eq!(
        anchor_of("Error correction, version, mode and mask"),
        "error-correction-version-mode-and-mask"
    );
    assert_eq!(
        anchor_of("The name `qr` is shared"),
        "the-name-qr-is-shared"
    );
    assert_eq!(anchor_of("Not in 0.1.0"), "not-in-010");
    assert_eq!(
        anchor_of("JSON output and exit codes"),
        "json-output-and-exit-codes"
    );
}

#[test]
fn mentions_requires_a_whole_option() {
    assert!(mentions("use --logo here", "--logo"));
    assert!(!mentions("only --logo-scale here", "--logo"));
    assert!(mentions("--logo-scale and --logo", "--logo"));
    assert!(mentions("end of text --logo", "--logo"));
}

#[test]
fn long_options_finds_only_option_names() {
    let found =
        long_options("  -o, --output <PATH>  and --no-clobber, not a-b or x--y, but --dpi=2");
    let expected: BTreeSet<&str> = ["--output", "--no-clobber", "--dpi"].into_iter().collect();
    assert_eq!(found, expected);
}

#[test]
fn parse_markdown_separates_headings_from_code_blocks() {
    let (headings, blocks) =
        parse_markdown("# Top\n\n```sh\n# not a heading\nqr hi\n```\n\n## Next\n");
    assert_eq!(headings, ["Top", "Next"]);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].info, "sh");
    assert_eq!(blocks[0].body, "# not a heading\nqr hi\n");
    assert_eq!(blocks[0].line, 3);
}

#[test]
fn link_targets_skip_titles_and_empty_targets() {
    let found = link_targets("[a](one.md) [b](two.md \"title\") [c]() [d](#top)");
    assert_eq!(found, ["one.md", "#top"]);
}

/// Fixture files that the README's examples name, created in the scratch
/// directory before any example runs.
#[cfg(unix)]
fn seed_fixtures(dir: &Path) {
    fs::write(dir.join("pw.txt"), "correct horse 42\n").unwrap();
    fs::write(dir.join("key.txt"), "abcde\n").unwrap();
    fs::write(
        dir.join("urls.csv"),
        "data,filename\nhttps://example.com/a,a\nhttps://example.com/b,b\n",
    )
    .unwrap();
    fs::write(
        dir.join("urls.txt"),
        "https://example.com/1\nhttps://example.com/2\n",
    )
    .unwrap();
    fs::write(
        dir.join("rows.csv"),
        "data,name\nhttps://example.com/a,alpha\nhttps://example.com/b,beta\n",
    )
    .unwrap();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 64, 128, 255]))
        .save(dir.join("logo.png"))
        .unwrap();
}

/// Replaces the byte count in `"written":N` with `N`, because the size of an
/// encoded file can change between releases while the format does not.
fn normalise_written(text: &str) -> String {
    const KEY: &str = "\"written\":";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(KEY) {
        let (head, tail) = rest.split_at(at + KEY.len());
        out.push_str(head);
        out.push('N');
        rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

#[test]
fn normalise_written_masks_only_the_count() {
    assert_eq!(
        normalise_written(r#"{"written":428,"x":1} {"written":7}"#),
        r#"{"written":N,"x":1} {"written":N}"#
    );
    assert_eq!(normalise_written("no key here"), "no key here");
}

#[cfg(unix)]
#[test]
fn readme_examples_run() {
    use std::os::unix::fs::symlink;
    use std::process::Command as Shell;

    let (_, blocks) = parse_markdown(&readme());
    let runnable = blocks.iter().filter(|block| block.info == "sh").count();
    assert!(runnable >= 30, "only {runnable} runnable examples found");

    let work = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let scripts = tempfile::tempdir().unwrap();
    symlink(env!("CARGO_BIN_EXE_qr"), bin.path().join("qr")).unwrap();
    seed_fixtures(work.path());
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    // The output (stdout, then stderr) of the most recent `sh` block. A block
    // tagged `... expected` must directly follow an `sh` block, and its body
    // must equal that output, with the `written` sizes normalised.
    let mut last_output = String::new();
    for (index, block) in blocks.iter().enumerate() {
        if block.info == "sh" {
            let script = scripts.path().join(format!("example-{index}.sh"));
            fs::write(&script, format!("set -e\n{}", block.body)).unwrap();
            let output = Shell::new("sh")
                .arg(&script)
                .current_dir(work.path())
                .env("PATH", &path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "the README example at line {} failed with {}:\n{}\nstderr:\n{}",
                block.line,
                output.status,
                block.body,
                String::from_utf8_lossy(&output.stderr)
            );
            last_output = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        } else if block.info.ends_with(" expected") {
            let follows_sh = index > 0 && blocks[index - 1].info == "sh";
            assert!(
                follows_sh,
                "the expected output at line {} must directly follow an `sh` block",
                block.line
            );
            assert_eq!(
                normalise_written(last_output.trim_end()),
                normalise_written(block.body.trim_end()),
                "the README output at line {} differs from the real output",
                block.line
            );
        }
    }
}
