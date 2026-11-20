//! Safe file names for `qr batch` (DECISIONS section 4.7).
//!
//! A batch file name is built from a template whose fields come from the
//! input, which is untrusted. Three layers keep every file inside
//! `--out-dir`:
//!
//! 1. [`sanitize_field`] cleans each field value: path separators (and
//!    look-alikes such as U+2215 DIVISION SLASH), `:`, NUL, control and
//!    bidi characters, Unicode line and paragraph separators and the other
//!    characters Windows forbids (`<>"|?*`) are removed, and leading `.`,
//!    `-` and white space are stripped.
//! 2. [`file_name`] checks the whole name: leading `.`/`-` and trailing
//!    `.`/space are stripped, Windows reserved device names (`CON`,
//!    `NUL`, `COM1`, ...) are rejected, and the name is capped at
//!    [`MAX_NAME_BYTES`] bytes, extension included.
//! 3. [`OutDir::resolve`] joins the name to the canonicalised `--out-dir`
//!    and refuses anything that is not exactly one plain path component
//!    directly inside it.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The longest file name `qr batch` writes, in bytes, extension included.
/// Most file systems allow 255; the margin leaves room for the temporary
/// file's affixes and for longer encodings of the same name.
pub(crate) const MAX_NAME_BYTES: usize = 200;

/// Why a row's file name was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// Nothing usable was left once the fields were cleaned.
    #[error("the file name is empty once unsafe characters are removed")]
    Empty,
    /// The name is a Windows device name, such as `CON` or `com1.png`.
    #[error("'{0}' is a reserved device name on Windows")]
    Reserved(String),
    /// The name would leave `--out-dir`. `sanitize_field` and
    /// `file_name` never produce such a name; this is the last check.
    #[error("'{0}' is not a plain file name inside the output directory")]
    Escapes(String),
}

/// Whether `c` is removed from template field values: path separators and
/// their look-alikes, colons (drive letters, NTFS streams), NUL and other
/// control characters, bidi and zero-width format characters, Unicode
/// line and paragraph separators, and the remaining characters Windows
/// forbids in names.
pub(crate) fn is_forbidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            // Separators and Windows-reserved punctuation.
            '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
            // Slash, backslash and colon look-alikes.
            | '\u{2044}' // FRACTION SLASH
            | '\u{2215}' // DIVISION SLASH
            | '\u{2216}' // SET MINUS
            | '\u{2236}' // RATIO
            | '\u{29F5}' // REVERSE SOLIDUS OPERATOR
            | '\u{29F8}' // BIG SOLIDUS
            | '\u{29F9}' // BIG REVERSE SOLIDUS
            | '\u{A789}' // MODIFIER LETTER COLON
            | '\u{FE13}' // PRESENTATION FORM FOR VERTICAL COLON
            | '\u{FE55}' // SMALL COLON
            | '\u{FE68}' // SMALL REVERSE SOLIDUS
            | '\u{FF0F}' // FULLWIDTH SOLIDUS
            | '\u{FF1A}' // FULLWIDTH COLON
            | '\u{FF3C}' // FULLWIDTH REVERSE SOLIDUS
            // Line and paragraph separators.
            | '\u{2028}' | '\u{2029}'
            // Bidi controls and zero-width characters, which can disguise
            // a name (`gpj.exe` shown as `exe.jpg`).
            | '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        )
}

/// Whether `c` may not start a name: a dot (hidden files, `.` and `..`),
/// a dash (read as an option by other tools) or white space.
fn is_bad_start(c: char) -> bool {
    c == '.' || c == '-' || c.is_whitespace()
}

/// Whether `c` may not end a name: Windows drops trailing dots and
/// spaces, so `a.` and `a` would be the same file.
fn is_bad_end(c: char) -> bool {
    c == '.' || c.is_whitespace()
}

/// Cleans one template field value (see the module documentation).
pub(crate) fn sanitize_field(value: &str) -> String {
    let kept: String = value.chars().filter(|&c| !is_forbidden(c)).collect();
    kept.trim_start_matches(is_bad_start).to_owned()
}

/// The final file name for a rendered template `stem` and `extension`
/// (without the dot).
///
/// A stem that already ends in `.extension` (ignoring ASCII case) does not
/// get it twice, so a CSV `filename` of `alice.png` gives `alice.png`.
///
/// # Errors
///
/// [`NameError::Empty`] when nothing is left, [`NameError::Reserved`] for a
/// Windows device name.
pub(crate) fn file_name(stem: &str, extension: &str) -> Result<String, NameError> {
    let mut stem = stem.trim_start_matches(is_bad_start);
    if let Some(dot) = stem.len().checked_sub(extension.len() + 1) {
        let (head, tail) = (stem.get(..dot), stem.get(dot..));
        if let (Some(head), Some(tail)) = (head, tail) {
            if tail.starts_with('.') && tail[1..].eq_ignore_ascii_case(extension) {
                stem = head;
            }
        }
    }
    let budget = MAX_NAME_BYTES.saturating_sub(extension.len() + 1);
    let stem = truncate(stem, budget).trim_end_matches(is_bad_end);
    if stem.is_empty() {
        return Err(NameError::Empty);
    }
    let name = format!("{stem}.{extension}");
    if is_reserved(stem) {
        return Err(NameError::Reserved(name));
    }
    Ok(name)
}

/// The longest prefix of `text` that is at most `max` bytes and ends on a
/// character boundary.
fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Whether `stem` is a Windows device name. Windows ignores everything
/// from the first dot, and trailing spaces, so `con.tar` and `NUL ` are
/// devices too.
fn is_reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).trim_end();
    let upper = base.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let (Some(prefix), Some(digit)) = (upper.get(..3), upper.get(3..)) else {
        return false;
    };
    matches!(prefix, "COM" | "LPT")
        && (matches!(digit, "¹" | "²" | "³")
            || (digit.len() == 1 && digit.bytes().all(|b| b.is_ascii_digit())))
}

/// The `--out-dir` directory, created if missing and canonicalised once.
#[derive(Debug, Clone)]
pub(crate) struct OutDir {
    /// The directory as given, for messages and `--json`.
    shown: PathBuf,
    /// The canonical directory every file is written in.
    canonical: PathBuf,
}

/// Where one file goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The path written: the canonical directory joined with the name.
    pub(crate) path: PathBuf,
    /// The path as the user would write it: `--out-dir` joined with the
    /// name.
    pub(crate) shown: PathBuf,
}

impl OutDir {
    /// Creates `path` (and its parents) if it does not exist, and
    /// canonicalises it.
    ///
    /// # Errors
    ///
    /// The I/O error if the directory cannot be created or resolved, or
    /// [`io::ErrorKind::NotADirectory`] if `path` is something else.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        // Checked first, so a file gives "not a directory" rather than
        // the "file exists" `create_dir_all` reports.
        if fs::metadata(path).is_ok_and(|meta| !meta.is_dir()) {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        fs::create_dir_all(path)?;
        let canonical = fs::canonicalize(path)?;
        if !fs::metadata(&canonical)?.is_dir() {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        Ok(Self {
            shown: path.to_owned(),
            canonical,
        })
    }

    /// The directory as given.
    pub(crate) fn shown(&self) -> &Path {
        &self.shown
    }

    /// The target for `name`, which must be exactly one plain path
    /// component.
    ///
    /// # Errors
    ///
    /// [`NameError::Escapes`] for anything else: separators, `.` or `..`,
    /// a root or drive prefix, or a forbidden character.
    pub(crate) fn resolve(&self, name: &str) -> Result<Target, NameError> {
        let escapes = || NameError::Escapes(name.to_owned());
        let mut components = Path::new(name).components();
        let plain = matches!(
            (components.next(), components.next()),
            (Some(Component::Normal(part)), None) if part == name
        );
        if !plain || name.chars().any(is_forbidden) || name.starts_with(is_bad_start) {
            return Err(escapes());
        }
        let path = self.canonical.join(name);
        if path.parent() != Some(self.canonical.as_path()) {
            return Err(escapes());
        }
        Ok(Target {
            path,
            shown: self.shown.join(name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(field: &str) -> Result<String, NameError> {
        file_name(&sanitize_field(field), "png")
    }

    #[test]
    fn path_traversal_is_flattened() {
        assert_eq!(name("../../etc/passwd").unwrap(), "etcpasswd.png");
        assert_eq!(name("..").unwrap_err(), NameError::Empty);
        assert_eq!(name("./.").unwrap_err(), NameError::Empty);
        assert_eq!(name("/etc/passwd").unwrap(), "etcpasswd.png");
        assert_eq!(name("..\\..\\win.ini").unwrap(), "win.ini.png");
        assert_eq!(name("a/../b").unwrap(), "a..b.png");
    }

    #[test]
    fn drive_letters_and_streams_lose_their_colon() {
        assert_eq!(name("C:\\Windows\\x").unwrap(), "CWindowsx.png");
        assert_eq!(name("C:x").unwrap(), "Cx.png");
        assert_eq!(name("file.txt:stream").unwrap(), "file.txtstream.png");
        assert_eq!(name("\\\\?\\C:\\x").unwrap(), "Cx.png");
    }

    #[test]
    fn unicode_separators_and_look_alikes_are_removed() {
        for sep in [
            '\u{2044}', '\u{2215}', '\u{2216}', '\u{2236}', '\u{29F5}', '\u{29F8}', '\u{29F9}',
            '\u{A789}', '\u{FE13}', '\u{FE55}', '\u{FE68}', '\u{FF0F}', '\u{FF1A}', '\u{FF3C}',
            '\u{2028}', '\u{2029}', '\u{0085}',
        ] {
            let field = format!("..{sep}..{sep}etc{sep}x");
            assert_eq!(
                name(&field).unwrap(),
                "etcx.png",
                "U+{:04X}",
                u32::from(sep)
            );
        }
    }

    #[test]
    fn controls_nul_and_bidi_are_removed() {
        assert_eq!(name("a\0b\nc\x1b[2Jd\x7f").unwrap(), "abc[2Jd.png");
        assert_eq!(name("\u{202e}gpj.exe").unwrap(), "gpj.exe.png");
        assert_eq!(name("a\u{200b}b\u{feff}").unwrap(), "ab.png");
        assert_eq!(name("q<u>o\"t|e?s*").unwrap(), "quotes.png");
    }

    #[test]
    fn leading_dots_and_dashes_and_trailing_dots_are_stripped() {
        assert_eq!(name(".hidden").unwrap(), "hidden.png");
        assert_eq!(name("--help").unwrap(), "help.png");
        assert_eq!(name(" -.x").unwrap(), "x.png");
        assert_eq!(file_name("x. . ", "png").unwrap(), "x.png");
        assert_eq!(file_name("-.-", "png").unwrap_err(), NameError::Empty);
        assert_eq!(file_name("", "png").unwrap_err(), NameError::Empty);
        // A leading dot can also come from a literal: stripped as well.
        assert_eq!(file_name(".x", "svg").unwrap(), "x.svg");
    }

    #[test]
    fn a_matching_extension_is_not_doubled() {
        assert_eq!(file_name("alice.png", "png").unwrap(), "alice.png");
        assert_eq!(file_name("alice.PNG", "png").unwrap(), "alice.png");
        assert_eq!(file_name("alice.svg", "png").unwrap(), "alice.svg.png");
        assert_eq!(file_name(".png", "png").unwrap(), "png.png");
        assert_eq!(file_name("a.png", "png").unwrap(), "a.png");
        assert_eq!(file_name("x.png.", "png").unwrap(), "x.png.png");
        assert_eq!(file_name("é.png", "png").unwrap(), "é.png");
    }

    #[test]
    fn windows_reserved_names_are_rejected() {
        for bad in [
            "CON", "con", "Prn", "aux", "NUL", "nul.tar", "COM1", "com9", "LPT1", "lpt0", "COM¹",
            "lpt³", "CONIN$", "conout$", "NUL ", "con.x.y",
        ] {
            let err = file_name(bad, "png").unwrap_err();
            assert!(matches!(err, NameError::Reserved(_)), "{bad}: {err:?}");
        }
        assert_eq!(
            file_name("con", "png").unwrap_err().to_string(),
            "'con.png' is a reserved device name on Windows"
        );
        for fine in [
            "CONSOLE", "COM10", "LPT", "nully", "COMA", "aux_1", "co", "COM¹²",
        ] {
            assert!(file_name(fine, "png").is_ok(), "{fine}");
        }
    }

    #[test]
    fn names_are_capped_at_200_bytes_on_a_character_boundary() {
        let long = "a".repeat(500);
        let name = file_name(&long, "png").unwrap();
        assert_eq!(name.len(), MAX_NAME_BYTES);
        // Two-byte characters: never cut in half.
        let long = "é".repeat(150);
        let name = file_name(&long, "webp").unwrap();
        assert!(
            name.len() <= MAX_NAME_BYTES && name.rsplit_once('.').map(|(_, e)| e) == Some("webp"),
            "{name}"
        );
        assert_eq!(name.len(), 194 + 5);
        // Trailing dots exposed by the cut are stripped too.
        let dotted = format!("{}{}", "b".repeat(195), "....x");
        assert_eq!(
            file_name(&dotted, "png").unwrap(),
            format!("{}.png", "b".repeat(195))
        );
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn resolve_keeps_files_directly_in_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let out = OutDir::open(&dir.path().join("new/nested")).unwrap();
        assert!(out.canonical.is_dir());
        assert_eq!(out.shown(), dir.path().join("new/nested"));
        let target = out.resolve("a.png").unwrap();
        assert_eq!(target.path, out.canonical.join("a.png"));
        assert_eq!(target.shown, dir.path().join("new/nested/a.png"));
        for bad in [
            "../a.png",
            "..",
            ".",
            "",
            "/etc/passwd",
            "a/b.png",
            "a\\b.png",
            "C:a.png",
            "C:\\a.png",
            "a\0b",
            "\u{2215}x",
            ".hidden",
            "-rf",
        ] {
            let err = out.resolve(bad).unwrap_err();
            assert_eq!(err, NameError::Escapes(bad.to_owned()), "{bad:?}");
        }
    }

    #[test]
    fn out_dir_must_be_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        fs::write(&file, b"x").unwrap();
        assert!(OutDir::open(&file).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_out_dir_is_resolved_once() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let out = OutDir::open(&link).unwrap();
        assert_eq!(out.canonical, fs::canonicalize(&real).unwrap());
        assert_eq!(out.resolve("x.png").unwrap().shown, link.join("x.png"));
    }
}
