//! Process exit statuses.
//!
//! The numeric values are part of the public CLI contract (see
//! `docs/design/DECISIONS.md` section 2) and must never be renumbered.

/// Every exit status the `qr` binary can return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExitStatus {
    /// The command completed successfully.
    Success = 0,
    /// An unexpected internal error occurred.
    Internal = 1,
    /// The command line was invalid.
    Usage = 2,
    /// The input does not fit in a QR code with the requested settings.
    DataTooLong = 3,
    /// Reading input or writing output failed.
    Io = 4,
    /// `qr decode` found no QR code in the image.
    NoCodeFound = 5,
    /// The rendered code did not decode back to the input.
    VerificationFailed = 6,
}

impl ExitStatus {
    /// The numeric process exit code.
    pub const fn code(self) -> u8 {
        self as u8
    }
}

impl From<ExitStatus> for std::process::ExitCode {
    fn from(status: ExitStatus) -> Self {
        Self::from(status.code())
    }
}

#[cfg(test)]
mod tests {
    use super::ExitStatus;

    #[test]
    fn codes_match_the_documented_contract() {
        let table = [
            (ExitStatus::Success, 0),
            (ExitStatus::Internal, 1),
            (ExitStatus::Usage, 2),
            (ExitStatus::DataTooLong, 3),
            (ExitStatus::Io, 4),
            (ExitStatus::NoCodeFound, 5),
            (ExitStatus::VerificationFailed, 6),
        ];
        for (status, code) in table {
            assert_eq!(status.code(), code, "{status:?}");
        }
    }

    #[test]
    fn converts_to_process_exit_code() {
        assert_eq!(
            std::process::ExitCode::from(ExitStatus::Usage),
            std::process::ExitCode::from(2)
        );
        assert_eq!(
            std::process::ExitCode::from(ExitStatus::Success),
            std::process::ExitCode::SUCCESS
        );
    }
}
