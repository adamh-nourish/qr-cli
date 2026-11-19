//! One entry point for the vector formats, for callers that pick the format
//! at run time (the CLI's `-f` and `-o` inference).

use std::fmt;
use std::str::FromStr;

use super::options::{RenderError, RenderOptions};
use super::{eps, pdf, svg};
use crate::matrix::Matrix;

/// The vector output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorFormat {
    /// Scalable Vector Graphics.
    Svg,
    /// Portable Document Format, one page.
    Pdf,
    /// Encapsulated PostScript.
    Eps,
}

impl VectorFormat {
    /// All vector formats.
    pub const ALL: [Self; 3] = [Self::Svg, Self::Pdf, Self::Eps];

    /// The usual file extension, without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Eps => "eps",
        }
    }

    /// The IANA media type, for `--data-uri` and similar.
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Pdf => "application/pdf",
            Self::Eps => "application/postscript",
        }
    }
}

impl fmt::Display for VectorFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.extension())
    }
}

/// The name was not a vector format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown vector format '{0}' (expected svg, pdf or eps)")]
pub struct UnknownVectorFormat(pub String);

impl FromStr for VectorFormat {
    type Err = UnknownVectorFormat;

    /// Parses `svg`, `pdf` or `eps`, ignoring ASCII case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|f| f.extension().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnknownVectorFormat(s.chars().take(32).collect()))
    }
}

/// Renders `matrix` in `format` and returns the file contents.
///
/// # Errors
///
/// Returns [`RenderError`] as the format's renderer does.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, VectorFormat, encode, render_vector};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// for format in VectorFormat::ALL {
///     let bytes = render_vector(code.matrix(), format, &RenderOptions::default())?;
///     assert!(!bytes.is_empty());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_vector(
    matrix: &Matrix,
    format: VectorFormat,
    options: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    match format {
        VectorFormat::Svg => svg::render_svg(matrix, options).map(String::into_bytes),
        VectorFormat::Pdf => pdf::render_pdf(matrix, options),
        VectorFormat::Eps => eps::render_eps(matrix, options).map(String::into_bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for format in VectorFormat::ALL {
            assert_eq!(format.to_string().parse(), Ok(format));
            assert_eq!(format.extension().to_uppercase().parse(), Ok(format));
        }
        assert_eq!(VectorFormat::Svg.media_type(), "image/svg+xml");
        assert_eq!(VectorFormat::Pdf.media_type(), "application/pdf");
        assert_eq!(VectorFormat::Eps.media_type(), "application/postscript");
        let err = "png".parse::<VectorFormat>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown vector format 'png' (expected svg, pdf or eps)"
        );
    }

    #[test]
    fn dispatches_to_each_renderer() {
        let m = Matrix::from_modules(1, vec![true]).unwrap();
        let o = RenderOptions::default();
        assert_eq!(
            render_vector(&m, VectorFormat::Svg, &o).unwrap(),
            svg::render_svg(&m, &o).unwrap().into_bytes()
        );
        assert_eq!(
            render_vector(&m, VectorFormat::Pdf, &o).unwrap(),
            pdf::render_pdf(&m, &o).unwrap()
        );
        assert_eq!(
            render_vector(&m, VectorFormat::Eps, &o).unwrap(),
            eps::render_eps(&m, &o).unwrap().into_bytes()
        );
        let zero = o.with_scale(0);
        for format in VectorFormat::ALL {
            assert_eq!(
                render_vector(&m, format, &zero),
                Err(RenderError::ZeroScale)
            );
        }
    }
}
