//! Facts about each [`OutputFormat`]: its name, its file extensions, whether
//! it is binary and which renderer draws it.

use std::path::Path;

use qr_core::{RasterFormat, TextFormat, VectorFormat};

use crate::cli::OutputFormat;

/// The renderer family that draws a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    /// One of the text renderers. `Terminal` resolves to ANSI or plain
    /// Unicode once colour is decided.
    Text,
    /// One of the vector renderers.
    Vector(VectorFormat),
    /// One of the raster renderers.
    Raster(RasterFormat),
}

/// Extensions recognised by `-o` inference, lower case, without the dot.
const EXTENSIONS: [(&str, OutputFormat); 8] = [
    ("png", OutputFormat::Png),
    ("svg", OutputFormat::Svg),
    ("jpg", OutputFormat::Jpeg),
    ("jpeg", OutputFormat::Jpeg),
    ("webp", OutputFormat::Webp),
    ("pdf", OutputFormat::Pdf),
    ("eps", OutputFormat::Eps),
    ("txt", OutputFormat::Text),
];

impl OutputFormat {
    /// The name used by `-f`, in messages and in `--json` output.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Text => "text",
            Self::Ascii => "ascii",
            Self::Png => "png",
            Self::Svg => "svg",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
            Self::Pdf => "pdf",
            Self::Eps => "eps",
        }
    }

    /// Which renderer family draws this format.
    pub const fn renderer(self) -> Renderer {
        match self {
            Self::Terminal | Self::Text | Self::Ascii => Renderer::Text,
            Self::Svg => Renderer::Vector(VectorFormat::Svg),
            Self::Pdf => Renderer::Vector(VectorFormat::Pdf),
            Self::Eps => Renderer::Vector(VectorFormat::Eps),
            Self::Png => Renderer::Raster(RasterFormat::Png),
            Self::Jpeg => Renderer::Raster(RasterFormat::Jpeg),
            Self::Webp => Renderer::Raster(RasterFormat::Webp),
        }
    }

    /// Whether the output is binary, and so must not be written to a
    /// terminal. SVG and EPS are text; PDF is treated as binary because its
    /// header marks it so and viewers expect the bytes untouched.
    pub const fn is_binary(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Pdf)
    }

    /// Whether this is an image (as opposed to a text rendering), which is
    /// what `--data-uri` and the image styling flags apply to.
    pub const fn is_image(self) -> bool {
        !matches!(self.renderer(), Renderer::Text)
    }

    /// The IANA media type of an image format, for `--data-uri`.
    pub const fn media_type(self) -> Option<&'static str> {
        match self {
            Self::Png => Some(RasterFormat::Png.media_type()),
            Self::Jpeg => Some(RasterFormat::Jpeg.media_type()),
            Self::Webp => Some(RasterFormat::Webp.media_type()),
            Self::Svg => Some(VectorFormat::Svg.media_type()),
            Self::Pdf => Some(VectorFormat::Pdf.media_type()),
            Self::Eps => Some(VectorFormat::Eps.media_type()),
            Self::Terminal | Self::Text | Self::Ascii => None,
        }
    }

    /// The text renderer for this format, given whether colour is on.
    /// `None` for image formats.
    pub const fn text_format(self, color: bool) -> Option<TextFormat> {
        match self {
            Self::Terminal if color => Some(TextFormat::Ansi),
            Self::Terminal | Self::Text => Some(TextFormat::Unicode),
            Self::Ascii => Some(TextFormat::Ascii),
            Self::Png | Self::Svg | Self::Jpeg | Self::Webp | Self::Pdf | Self::Eps => None,
        }
    }

    /// The extension (without the dot) of a file holding this format, as
    /// `qr batch` names them; `None` for `terminal`, which is not written
    /// to files.
    pub const fn extension(self) -> Option<&'static str> {
        match self {
            Self::Png => Some("png"),
            Self::Svg => Some("svg"),
            Self::Jpeg => Some("jpg"),
            Self::Webp => Some("webp"),
            Self::Pdf => Some("pdf"),
            Self::Eps => Some("eps"),
            Self::Text | Self::Ascii => Some("txt"),
            Self::Terminal => None,
        }
    }

    /// The format a file extension names, ignoring ASCII case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        EXTENSIONS
            .iter()
            .find(|(ext, _)| ext.eq_ignore_ascii_case(extension))
            .map(|&(_, format)| format)
    }

    /// The format named by `path`'s extension, if it has a known one.
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }
}

/// The extensions `-o` recognises, for hints: `.png, .svg, ...`.
pub fn known_extensions() -> String {
    EXTENSIONS
        .iter()
        .map(|(ext, _)| format!(".{ext}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum as _;

    use super::*;

    #[test]
    fn names_match_the_clap_values() {
        for format in OutputFormat::value_variants() {
            let value = format.to_possible_value().unwrap();
            assert_eq!(value.get_name(), format.name());
        }
    }

    #[test]
    fn extensions_are_case_insensitive() {
        assert_eq!(OutputFormat::from_extension("SVG"), Some(OutputFormat::Svg));
        assert_eq!(
            OutputFormat::from_extension("Jpg"),
            Some(OutputFormat::Jpeg)
        );
        assert_eq!(OutputFormat::from_extension("gif"), None);
        assert_eq!(
            OutputFormat::from_path(Path::new("dir.d/code.PDF")),
            Some(OutputFormat::Pdf)
        );
        assert_eq!(OutputFormat::from_path(Path::new("dir.svg/code")), None);
        assert_eq!(OutputFormat::from_path(Path::new(".svg")), None);
        assert_eq!(
            known_extensions(),
            ".png, .svg, .jpg, .jpeg, .webp, .pdf, .eps, .txt"
        );
    }

    #[test]
    fn file_extensions_name_a_format_drawn_the_same_way() {
        for format in OutputFormat::value_variants() {
            match format.extension() {
                Some(ext) => {
                    let back = OutputFormat::from_extension(ext).unwrap();
                    assert_eq!(back.renderer(), format.renderer(), "{format:?}");
                }
                None => assert_eq!(*format, OutputFormat::Terminal),
            }
        }
    }

    #[test]
    fn every_extension_maps_back_to_a_format_with_that_extension() {
        for (ext, format) in EXTENSIONS {
            assert_eq!(OutputFormat::from_extension(ext), Some(format));
        }
    }

    #[test]
    fn classification_table() {
        use OutputFormat as F;
        // (format, renderer, binary, image, media type)
        let table = [
            (F::Terminal, Renderer::Text, false, None),
            (F::Text, Renderer::Text, false, None),
            (F::Ascii, Renderer::Text, false, None),
            (
                F::Png,
                Renderer::Raster(RasterFormat::Png),
                true,
                Some("image/png"),
            ),
            (
                F::Jpeg,
                Renderer::Raster(RasterFormat::Jpeg),
                true,
                Some("image/jpeg"),
            ),
            (
                F::Webp,
                Renderer::Raster(RasterFormat::Webp),
                true,
                Some("image/webp"),
            ),
            (
                F::Svg,
                Renderer::Vector(VectorFormat::Svg),
                false,
                Some("image/svg+xml"),
            ),
            (
                F::Pdf,
                Renderer::Vector(VectorFormat::Pdf),
                true,
                Some("application/pdf"),
            ),
            (
                F::Eps,
                Renderer::Vector(VectorFormat::Eps),
                false,
                Some("application/postscript"),
            ),
        ];
        for (format, renderer, binary, media) in table {
            assert_eq!(format.renderer(), renderer, "{format:?}");
            assert_eq!(format.is_binary(), binary, "{format:?}");
            assert_eq!(format.media_type(), media, "{format:?}");
            assert_eq!(format.is_image(), media.is_some(), "{format:?}");
        }
    }

    #[test]
    fn text_formats_resolve_colour() {
        use OutputFormat as F;
        assert_eq!(F::Terminal.text_format(true), Some(TextFormat::Ansi));
        assert_eq!(F::Terminal.text_format(false), Some(TextFormat::Unicode));
        assert_eq!(F::Text.text_format(true), Some(TextFormat::Unicode));
        assert_eq!(F::Ascii.text_format(false), Some(TextFormat::Ascii));
        assert_eq!(F::Svg.text_format(true), None);
    }
}
