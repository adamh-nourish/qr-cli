//! Typed errors for the CLI layer.

use std::io;

use crate::exit::ExitStatus;

/// Errors the CLI reports to the user before exiting with a non-zero status.
///
/// Messages are lowercase and contain no stack traces, following the UX rules
/// in the design record.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Writing to standard output failed (for example, a closed pipe).
    #[error("cannot write output: {0}")]
    Output(#[source] io::Error),
}

impl CliError {
    /// The exit status that this error maps to.
    pub const fn exit_status(&self) -> ExitStatus {
        match self {
            Self::Output(_) => ExitStatus::Io,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;
    use std::io;

    use super::CliError;
    use crate::exit::ExitStatus;

    #[test]
    fn output_error_maps_to_io_status_and_keeps_source() {
        let err = CliError::Output(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"));
        assert_eq!(err.exit_status(), ExitStatus::Io);
        assert_eq!(err.to_string(), "cannot write output: pipe closed");
        assert!(err.source().is_some());
    }
}
