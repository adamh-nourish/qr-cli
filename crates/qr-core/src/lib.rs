//! Core QR code library for the `qr` command-line tool.
//!
//! This crate is deliberately free of I/O: it will hold the module matrix,
//! the encoder abstraction, payload builders, renderers and resource limits.
//! The `qr-cli` crate owns argument parsing, files, streams and exit codes.
//!
//! The scaffold only exposes the crate version; encoding arrives with the
//! `QrEncoder` trait in a later change.

#![forbid(unsafe_code)]

/// Version of this crate, taken from its manifest at compile time.
///
/// The workspace versions `qr-core` and `qr-cli` in lockstep, so this always
/// matches the version reported by `qr --version`.
///
/// ```
/// assert!(!qr_core::VERSION.is_empty());
/// ```
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_semver_like() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {VERSION}");
        for part in parts {
            assert!(
                part.chars().next().is_some_and(|c| c.is_ascii_digit()),
                "non-numeric version component in {VERSION}"
            );
        }
    }
}
