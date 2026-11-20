//! Release packaging contract (DECISIONS.md section 6).
//!
//! These tests pin what a release must contain and how it is built, so a
//! change to `dist-workspace.toml`, the release workflows or the helper
//! scripts that drifts from the owner's v1 distribution decision fails CI
//! instead of surfacing on a tag: six targets, GitHub Releases plus the shell
//! and PowerShell installers only, `cargo auditable`, a static MSVC runtime,
//! completions and the man page in every archive, SHA-pinned actions, and a
//! CHANGELOG.md section for the version being released.

// Test helpers outside #[test] functions are not covered by clippy.toml's
// allow-*-in-tests settings; failing loudly is the point here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

/// The six release targets, in the order dist-workspace.toml lists them.
const TARGETS: [&str; 6] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
];

/// Every shell `qr completions` supports, with the file name
/// scripts/dist-extras.sh gives its script.
const COMPLETIONS: [(&str, &str); 5] = [
    ("bash", "qr.bash"),
    ("zsh", "_qr"),
    ("fish", "qr.fish"),
    ("powershell", "_qr.ps1"),
    ("elvish", "qr.elv"),
];

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/qr-cli sits two levels below the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The `[dist]` table of dist-workspace.toml, without comments.
fn dist_table() -> String {
    let config = read("dist-workspace.toml");
    let start = config
        .find("\n[dist]\n")
        .expect("dist-workspace.toml has a [dist] table");
    config[start..]
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The string items of the array `key = [ ... ]` in `table`.
fn string_array(table: &str, key: &str) -> Vec<String> {
    let start = table
        .find(&format!("\n{key} = ["))
        .unwrap_or_else(|| panic!("no `{key} = [` in [dist]"));
    let body = &table[start..];
    let open = body.find('[').unwrap();
    let close = body.find(']').unwrap();
    body[open + 1..close]
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// The value of the scalar `key = value` in `table`, without quotes.
fn scalar(table: &str, key: &str) -> Option<String> {
    table.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

#[test]
fn dist_builds_exactly_the_six_release_targets() {
    assert_eq!(string_array(&dist_table(), "targets"), TARGETS);
}

#[test]
fn dist_ships_only_github_releases_and_the_two_installers() {
    let table = dist_table();
    assert_eq!(string_array(&table, "installers"), ["shell", "powershell"]);
    // Owner decision for v1: no Homebrew tap, Scoop, winget, npm, MSI or
    // crates.io, so nothing needs a secret.
    for key in ["publish-jobs", "tap", "npm-scope", "github-attestations"] {
        assert!(scalar(&table, key).is_none(), "{key} must stay unset in v1");
    }
    for channel in ["homebrew", "npm", "msi"] {
        assert!(
            !table.contains(&format!("\"{channel}\"")),
            "{channel} is not a v1 channel"
        );
    }
    assert_eq!(scalar(&table, "ci").as_deref(), Some("github"));
    assert_eq!(scalar(&table, "pr-run-mode").as_deref(), Some("plan"));
}

#[test]
fn dist_builds_auditable_static_binaries_with_checksums() {
    let table = dist_table();
    assert_eq!(scalar(&table, "cargo-auditable").as_deref(), Some("true"));
    assert_eq!(scalar(&table, "msvc-crt-static").as_deref(), Some("true"));
    assert_eq!(scalar(&table, "checksum").as_deref(), Some("sha256"));
    assert_eq!(scalar(&table, "unix-archive").as_deref(), Some(".tar.xz"));
    assert_eq!(scalar(&table, "windows-archive").as_deref(), Some(".zip"));
    assert_eq!(scalar(&table, "install-updater").as_deref(), Some("false"));
    assert_eq!(
        string_array(&table, "include"),
        ["dist-extras/completions", "dist-extras/man"]
    );
}

#[test]
fn dist_uses_the_release_profile() {
    let manifest = read("Cargo.toml");
    let dist = manifest
        .split("\n[profile.dist]\n")
        .nth(1)
        .expect("Cargo.toml defines [profile.dist]");
    assert!(dist.trim_start().starts_with("inherits = \"release\""));
}

#[test]
fn dist_version_is_the_same_in_the_config_and_the_installer_action() {
    let configured = scalar(&dist_table(), "cargo-dist-version").unwrap();
    let action = read(".github/actions/install-dist/action.yml");
    assert!(
        action.contains(&format!("DIST_VERSION: \"{configured}\"")),
        "install-dist must install dist {configured}"
    );
}

#[test]
fn every_target_is_built_and_smoke_tested() {
    let workflow = read(".github/workflows/dist-build.yml");
    for target in TARGETS {
        let entries = workflow
            .matches(&format!("{{ target: {target}, runner: "))
            .count();
        assert_eq!(entries, 2, "{target}: one build and one smoke matrix entry");
    }
}

#[test]
fn custom_runners_match_the_build_matrix() {
    let config = read("dist-workspace.toml");
    let runners = config
        .split("[dist.github-custom-runners]")
        .nth(1)
        .expect("dist-workspace.toml sets github-custom-runners");
    let workflow = read(".github/workflows/dist-build.yml");
    let build = workflow
        .split("\n  build:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n  global:\n").next())
        .expect("dist-build.yml has a build job");
    for target in TARGETS {
        let runner = runners
            .lines()
            .find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == target).then(|| v.trim().trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| panic!("no custom runner for {target}"));
        assert!(
            build.contains(&format!("{{ target: {target}, runner: {runner} }}")),
            "{target} builds on {runner} in dist-build.yml"
        );
    }
}

#[test]
fn release_runs_only_on_version_tags_and_only_publish_can_write() {
    let release = read(".github/workflows/release.yml");
    let triggers = release
        .split("\non:\n")
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .unwrap();
    assert_eq!(
        triggers.split_whitespace().collect::<Vec<_>>(),
        ["push:", "tags:", "-", "\"v*\""]
    );
    assert!(release.contains("\npermissions: {}\n"));
    assert_eq!(release.matches("contents: write").count(), 1);
    let publish = release.split("\n  publish:\n").nth(1).unwrap();
    let publish = publish.split("\n  installers:\n").next().unwrap();
    assert!(publish.contains("contents: write"));
    assert!(
        !publish.contains("actions/checkout"),
        "the job that can write must not run repository code"
    );
}

#[test]
fn workflows_pin_actions_and_drop_checkout_credentials() {
    let dir = workspace_root().join(".github/workflows");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.push(workspace_root().join(".github/actions/install-dist/action.yml"));
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let name = path.display();
        for line in text.lines().map(str::trim) {
            let Some(uses) = line
                .strip_prefix("- uses: ")
                .or_else(|| line.strip_prefix("uses: "))
            else {
                continue;
            };
            if uses.starts_with("./") {
                continue;
            }
            let sha = uses
                .split_once('@')
                .map(|(_, rest)| rest.split_whitespace().next().unwrap_or_default())
                .unwrap_or_default();
            assert!(
                sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name}: `{uses}` is not pinned to a full commit SHA"
            );
        }
        let checkouts = text.matches("uses: actions/checkout@").count();
        let dropped = text.matches("persist-credentials: false").count();
        assert_eq!(
            checkouts, dropped,
            "{name}: every checkout drops credentials"
        );
    }
}

#[test]
fn release_workflows_restore_no_cache() {
    for file in ["dist-build.yml", "release.yml", "dist-nightly.yml"] {
        let text = read(&format!(".github/workflows/{file}"));
        for cache in ["rust-cache", "actions/cache", "sccache"] {
            assert!(!text.contains(cache), "{file} must not use {cache}");
        }
    }
}

#[test]
fn changelog_has_a_section_for_this_version() {
    let changelog = read("CHANGELOG.md");
    let unreleased = changelog
        .find("\n## [Unreleased]\n")
        .expect("Unreleased section");
    let this = changelog
        .find(&format!("\n## [{VERSION}]"))
        .unwrap_or_else(|| panic!("CHANGELOG.md has no section for {VERSION}"));
    assert!(
        unreleased < this,
        "Unreleased comes first (Keep a Changelog)"
    );
    assert!(changelog.contains(&format!("\n[{VERSION}]: https://github.com/")));
}

#[test]
fn version_flag_reports_the_workspace_version() {
    let output = cargo_bin_cmd!("qr").arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("qr {VERSION}\n")
    );
}

#[test]
fn completions_cover_every_shell_and_are_deterministic() {
    let script = read("scripts/dist-extras.sh");
    for (shell, file) in COMPLETIONS {
        let first = cargo_bin_cmd!("qr")
            .args(["completions", shell])
            .output()
            .unwrap();
        let second = cargo_bin_cmd!("qr")
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(first.status.success(), "{shell}");
        assert!(first.stdout.len() > 1_000, "{shell} script looks truncated");
        assert_eq!(
            first.stdout, second.stdout,
            "{shell} output must be reproducible"
        );
        assert!(
            script.contains(&format!("{shell}) echo {file} ;;")),
            "dist-extras.sh names the {shell} script {file}"
        );
    }
}

/// The helper scripts are bash; CI runs them on every OS, these tests run
/// them where bash is the system shell.
#[cfg(unix)]
mod scripts {
    use std::process::Command;

    use super::{COMPLETIONS, VERSION, workspace_root};

    fn qr_path() -> std::path::PathBuf {
        assert_cmd::cargo::cargo_bin!("qr").to_path_buf()
    }

    fn script(name: &str) -> Command {
        let mut cmd = Command::new("bash");
        cmd.arg(workspace_root().join("scripts").join(name));
        cmd
    }

    #[test]
    fn smoke_test_passes_against_the_built_binary() {
        let output = script("smoke-test.sh")
            .arg(qr_path())
            .arg(VERSION)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("smoke test passed"));
    }

    #[test]
    fn smoke_test_fails_on_a_version_mismatch() {
        let output = script("smoke-test.sh")
            .arg(qr_path())
            .arg("0.0.0-not-this")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("smoke test FAILED"));
    }

    #[test]
    fn scripts_reject_bad_usage() {
        for name in ["smoke-test.sh", "dist-extras.sh", "verify-archive.sh"] {
            let output = script(name).output().unwrap();
            assert_eq!(output.status.code(), Some(2), "{name}: {output:?}");
        }
        let output = script("dist-extras.sh")
            .arg("/nonexistent/qr")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }

    /// Packs the built binary and the release extras the way dist does, as
    /// `<dir>/qr-cli-<target>.tar.xz` plus its `.sha256` file.
    fn fake_archive(dir: &std::path::Path, target: &str) -> std::path::PathBuf {
        let name = format!("qr-cli-{target}");
        let root = dir.join("staging").join(&name);
        std::fs::create_dir_all(&root).unwrap();
        let extras = script("dist-extras.sh")
            .arg(qr_path())
            .arg(&root)
            .output()
            .unwrap();
        assert!(extras.status.success(), "{extras:?}");
        std::fs::copy(qr_path(), root.join("qr")).unwrap();
        for file in ["LICENSE-MIT", "LICENSE-APACHE", "README.md", "CHANGELOG.md"] {
            std::fs::copy(workspace_root().join(file), root.join(file)).unwrap();
        }
        pack(dir, &name)
    }

    /// Re-packs `<dir>/staging/<name>` into `<dir>/<name>.tar.xz` and
    /// writes its `.sha256` file in dist's format (with dist's trailing
    /// blank line).
    fn pack(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        let archive = dir.join(format!("{name}.tar.xz"));
        let tar = Command::new("tar")
            .arg("-cJf")
            .arg(&archive)
            .arg("-C")
            .arg(dir.join("staging"))
            .arg(name)
            .status()
            .unwrap();
        assert!(tar.success());
        let sum = Command::new("shasum")
            .args(["-a", "256"])
            .arg(&archive)
            .output()
            .or_else(|_| Command::new("sha256sum").arg(&archive).output())
            .unwrap();
        let hash = String::from_utf8(sum.stdout).unwrap();
        let hash = hash.split_whitespace().next().unwrap().to_owned();
        std::fs::write(
            dir.join(format!("{name}.tar.xz.sha256")),
            format!("{hash} *{name}.tar.xz\n\n"),
        )
        .unwrap();
        archive
    }

    #[test]
    fn verify_archive_accepts_a_complete_archive() {
        let dir = tempfile::tempdir().unwrap();
        fake_archive(dir.path(), "x86_64-apple-darwin");
        let output = script("verify-archive.sh")
            .arg(dir.path())
            .args(["x86_64-apple-darwin", VERSION])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{output:?}");
        assert!(stdout.contains("checksum OK"), "{stdout}");
        assert!(stdout.contains("./man/qr.1"), "{stdout}");
        assert!(stdout.contains("smoke test passed"), "{stdout}");
    }

    #[test]
    fn verify_archive_rejects_a_bad_checksum_or_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        fake_archive(dir.path(), "x86_64-apple-darwin");
        let sha = dir.path().join("qr-cli-x86_64-apple-darwin.tar.xz.sha256");
        std::fs::write(&sha, format!("{} *x\n", "0".repeat(64))).unwrap();
        let run = || {
            script("verify-archive.sh")
                .arg(dir.path())
                .arg("x86_64-apple-darwin")
                .output()
                .unwrap()
        };
        let output = run();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("SHA-256"));

        // A correct checksum over an archive without the man page.
        let staging = dir.path().join("staging/qr-cli-x86_64-apple-darwin");
        std::fs::remove_dir_all(staging.join("man")).unwrap();
        pack(dir.path(), "qr-cli-x86_64-apple-darwin");
        let output = run();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("man/qr.1"));
    }

    #[test]
    fn dist_extras_writes_what_qr_prints() {
        let out = tempfile::tempdir().unwrap();
        let status = script("dist-extras.sh")
            .arg(qr_path())
            .arg(out.path())
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        for (shell, file) in COMPLETIONS {
            let written = std::fs::read(out.path().join("completions").join(file)).unwrap();
            let printed = Command::new(qr_path())
                .args(["completions", shell])
                .output()
                .unwrap()
                .stdout;
            assert_eq!(written, printed, "{file}");
        }
        let man = std::fs::read_to_string(out.path().join("man/qr.1")).unwrap();
        assert!(man.contains(&format!(".TH qr 1  \"qr {VERSION}\"")));
    }
}
