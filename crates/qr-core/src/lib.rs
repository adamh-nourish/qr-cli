//! Core QR code library for the `qr` command-line tool.
//!
//! This crate is deliberately free of I/O. It holds the module [`Matrix`],
//! the [`QrEncoder`] abstraction, exact [`capacity`] limits, the typed
//! [`Color`], the text, vector (SVG, PDF, EPS) and raster (PNG, JPEG,
//! WebP) [`render`]ers with their [`ModuleShape`]s, colour [`Contrast`],
//! read-back [`verify`]cation with `rqrr`, structured [`payload`]
//! builders (WiFi, vCard, `mailto:`, iCalendar and more),
//! [`sanitize_for_terminal`], [`logo`] overlays, the bounded image
//! [`loader`] they read images with and [`decode`], which finds and reads
//! every code in a loaded image for `qr decode`. The `qr-cli`
//! crate owns argument parsing, files, streams and exit codes; the loader
//! takes a reader and never opens a path itself.
//!
//! ```
//! use qr_core::{EcLevel, EncodeOptions, encode};
//!
//! let options = EncodeOptions::default().with_ec_level(EcLevel::H);
//! let code = encode(b"hello", &options)?;
//! assert_eq!(code.matrix().width(), 21);
//! # Ok::<(), qr_core::EncodeError>(())
//! ```

#![forbid(unsafe_code)]

pub mod capacity;
mod color;
mod contrast;
pub mod decode;
mod encode;
mod error;
pub mod loader;
pub mod logo;
mod matrix;
mod options;
pub mod payload;
pub mod render;
mod sanitize;
pub mod verify;

pub use crate::capacity::max_chars;
pub use crate::color::{Color, ColorError};
pub use crate::contrast::{Contrast, MIN_CONTRAST};
pub use crate::encode::{EncodePlan, QrCode, QrEncoder, QrcodeEncoder, Segment, encode, plan};
pub use crate::error::{CapacityError, CapacityUnit, EncodeError};
pub use crate::matrix::{Matrix, MatrixError};
pub use crate::options::{
    EcLevel, EncodeOptions, Mask, Mode, ModeChoice, OptionError, Version, VersionChoice,
};
pub use crate::render::{
    ModuleShape, RasterFormat, RenderError, RenderOptions, TextFormat, TextOptions,
    UnknownModuleShape, UnknownRasterFormat, UnknownVectorFormat, VectorFormat, render_eps,
    render_jpeg, render_pdf, render_png, render_raster, render_svg, render_text, render_vector,
    render_webp,
};
pub use crate::sanitize::{sanitize_for_terminal, sanitize_lines_for_terminal};

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
