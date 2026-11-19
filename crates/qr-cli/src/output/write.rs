//! Atomic file output (DECISIONS section 4.6).
//!
//! The bytes go to a temporary file created in the destination's own
//! directory, which is flushed and then renamed over the destination. The
//! rename is atomic on one filesystem, so a reader sees the old file or the
//! complete new one, never a partial write, and a failed write leaves the
//! old file untouched. Renaming replaces a symlink at the destination rather
//! than writing through it to the file it points at.
//!
//! With `no_clobber` the final step refuses to replace anything
//! ([`tempfile::NamedTempFile::persist_noclobber`]), which closes the window
//! between an existence check and the write.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use tempfile::{Builder, NamedTempFile};

/// Why an output file could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// `no_clobber` was set and something already exists at the path.
    #[error("'{}' already exists", .0.display())]
    Exists(PathBuf),
    /// Creating, writing or renaming the file failed.
    #[error("cannot write '{}'", .path.display())]
    Io {
        /// The destination path.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        error: io::Error,
    },
}

/// Checks, before any work is done, that `path` can plausibly be written:
/// it must not be a directory, and with `no_clobber` nothing may exist there
/// (a dangling symlink counts). The write itself checks again atomically.
///
/// # Errors
///
/// [`WriteError::Exists`] or an [`io::ErrorKind::IsADirectory`] error.
pub fn precheck(path: &Path, no_clobber: bool) -> Result<(), WriteError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Err(WriteError::Io {
            path: path.to_owned(),
            error: io::ErrorKind::IsADirectory.into(),
        }),
        Ok(_) if no_clobber => Err(WriteError::Exists(path.to_owned())),
        Ok(_) | Err(_) => Ok(()),
    }
}

/// Writes `bytes` to `path` atomically.
///
/// # Errors
///
/// [`WriteError::Exists`] if `no_clobber` is set and the path exists, and
/// [`WriteError::Io`] for any other failure. On error the destination is
/// unchanged and the temporary file is removed.
pub fn write_atomic(path: &Path, bytes: &[u8], no_clobber: bool) -> Result<(), WriteError> {
    let io_error = |error| WriteError::Io {
        path: path.to_owned(),
        error,
    };
    let mut file = temp_file_beside(path).map_err(io_error)?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.as_file().sync_all())
        .map_err(io_error)?;
    let persisted = if no_clobber {
        file.persist_noclobber(path)
    } else {
        file.persist(path)
    };
    match persisted {
        Ok(_) => Ok(()),
        Err(e) if no_clobber && e.error.kind() == io::ErrorKind::AlreadyExists => {
            Err(WriteError::Exists(path.to_owned()))
        }
        // Dropping `e.file` removes the temporary file.
        Err(e) => Err(io_error(e.error)),
    }
}

/// A new temporary file in the same directory as `path`, so the final
/// rename never crosses a filesystem.
fn temp_file_beside(path: &Path) -> io::Result<NamedTempFile> {
    let mut builder = Builder::new();
    builder.prefix(".qr-").suffix(".tmp");
    // The default is 0600 for temporary files. Output files get the usual
    // 0666 less the umask, like any file a shell redirect creates.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    builder.tempfile_in(parent_dir(path))
}

/// The directory `path` is in: its parent, or `.` for a bare file name.
fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_files(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".qr-"))
            .collect()
    }

    #[test]
    fn writes_new_and_replaces_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.svg");
        write_atomic(&path, b"one", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"one");
        write_atomic(&path, b"two", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(temp_files(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn no_clobber_refuses_existing_files_and_leaves_them_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.svg");
        write_atomic(&path, b"first", true).unwrap();
        let err = write_atomic(&path, b"second", true).unwrap_err();
        assert!(matches!(&err, WriteError::Exists(p) if p == &path), "{err}");
        assert_eq!(
            err.to_string(),
            format!("'{}' already exists", path.display())
        );
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(temp_files(dir.path()), Vec::<String>::new());
        assert!(matches!(precheck(&path, true), Err(WriteError::Exists(_))));
        assert!(precheck(&path, false).is_ok());
    }

    #[test]
    fn missing_directories_are_io_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no/such/dir/out.svg");
        assert!(precheck(&path, true).is_ok());
        let err = write_atomic(&path, b"x", false).unwrap_err();
        let WriteError::Io { error, .. } = &err else {
            panic!("{err:?}");
        };
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().starts_with("cannot write '"));
    }

    #[test]
    fn directories_are_refused_up_front() {
        let dir = tempfile::tempdir().unwrap();
        let err = precheck(dir.path(), false).unwrap_err();
        let WriteError::Io { error, .. } = err else {
            panic!("{err:?}");
        };
        assert_eq!(error.kind(), io::ErrorKind::IsADirectory);
    }

    #[test]
    fn a_bare_file_name_is_written_in_the_current_directory() {
        assert_eq!(parent_dir(Path::new("bare.svg")), Path::new("."));
        assert_eq!(parent_dir(Path::new("a/b.svg")), Path::new("a"));
        assert_eq!(parent_dir(Path::new("/b.svg")), Path::new("/"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_replaced_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, b"precious").unwrap();
        let link = dir.path().join("out.svg");
        std::os::unix::fs::symlink(&victim, &link).unwrap();

        assert!(matches!(precheck(&link, true), Err(WriteError::Exists(_))));
        assert!(matches!(
            write_atomic(&link, b"new", true),
            Err(WriteError::Exists(_))
        ));
        write_atomic(&link, b"new", false).unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"precious");
        assert!(
            !fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&link).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn files_get_the_same_mode_as_a_plain_create() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.svg");
        write_atomic(&path, b"x", false).unwrap();
        let plain = dir.path().join("plain");
        fs::write(&plain, b"x").unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), mode(&plain));
    }
}
