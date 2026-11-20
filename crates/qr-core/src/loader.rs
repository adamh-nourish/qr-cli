//! The one bounded image loader (DECISIONS section 4.3).
//!
//! Every image the tool reads, a `--logo` or a `qr decode` input, goes
//! through [`load_image`] (from any reader) or
//! [`decode_image`] (from bytes already in memory). Both apply the same
//! limits, in this order, before any pixel is decoded:
//!
//! 1. the file holds at most [`MAX_IMAGE_FILE_BYTES`] (10 MiB); a declared
//!    length is checked before anything is read, and the read itself stops
//!    one byte past the limit;
//! 2. the header must start with a PNG, JPEG or WebP signature; the format
//!    comes from those bytes alone, never from a file name;
//! 3. the decoder runs under [`image::Limits`] of at most
//!    [`MAX_IMAGE_SIDE`] pixels per side and [`MAX_IMAGE_ALLOC`] bytes
//!    allocated, and the declared dimensions and decoded size are checked
//!    again here with checked arithmetic, because the allocation limit is
//!    advisory for some decoders.
//!
//! The result is a [`Bitmap`] of straight (not premultiplied) 8-bit RGBA
//! pixels, whatever the source layout.
//!
//! This module reads from a caller-supplied reader but opens nothing
//! itself: the CLI owns paths and files.

use std::fmt;
use std::io::{self, Cursor, Read};

use image::error::LimitErrorKind;
use image::{DynamicImage, ImageDecoder as _, ImageError, ImageFormat, ImageReader, Limits};
use thiserror::Error;

/// The largest image file accepted, in bytes: 10 MiB.
pub const MAX_IMAGE_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// The largest width and height accepted, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 8_192;

/// The most memory a decoder may allocate for one image, in bytes: 128 MiB.
/// The decoded RGBA pixels must fit in it too.
pub const MAX_IMAGE_ALLOC: u64 = 128 * 1024 * 1024;

/// An image file format the loader accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageKind {
    /// Portable Network Graphics.
    Png,
    /// JPEG (JFIF or Exif).
    Jpeg,
    /// WebP, lossy or lossless.
    Webp,
}

impl ImageKind {
    /// Every accepted format.
    pub const ALL: [Self; 3] = [Self::Png, Self::Jpeg, Self::Webp];

    /// The format's name, as used in messages.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    /// The bytes every file of this format starts with. A WebP file starts
    /// with `RIFF`, a four-byte length and then `WEBP`; the length is not
    /// part of the signature, see [`Self::sniff`].
    const fn signature(self) -> &'static [u8] {
        match self {
            Self::Png => b"\x89PNG\r\n\x1a\n",
            Self::Jpeg => b"\xff\xd8\xff",
            Self::Webp => b"RIFF\0\0\0\0WEBP",
        }
    }

    /// Whether `bytes` match this format's signature over their first
    /// `len` bytes (bytes 4 to 7 of a WebP header are a length and match
    /// anything).
    fn matches(self, bytes: &[u8], len: usize) -> bool {
        self.signature()
            .iter()
            .zip(bytes)
            .take(len)
            .enumerate()
            .all(|(i, (want, got))| (self == Self::Webp && (4..8).contains(&i)) || want == got)
    }

    /// The format whose signature `bytes` start with, if any.
    ///
    /// ```
    /// use qr_core::loader::ImageKind;
    /// assert_eq!(ImageKind::sniff(b"\x89PNG\r\n\x1a\n..."), Some(ImageKind::Png));
    /// assert_eq!(ImageKind::sniff(b"GIF89a"), None);
    /// ```
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| {
            let len = kind.signature().len();
            bytes.len() >= len && kind.matches(bytes, len)
        })
    }

    /// The format whose signature `bytes` are a proper prefix of: a file
    /// cut short inside its own signature.
    fn truncated(bytes: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| {
            let len = kind.signature().len();
            !bytes.is_empty() && bytes.len() < len && kind.matches(bytes, bytes.len())
        })
    }

    const fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Webp => ImageFormat::WebP,
        }
    }
}

impl fmt::Display for ImageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why an image could not be loaded.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoadError {
    /// Reading the image failed.
    #[error("cannot read the image: {0}")]
    Read(#[source] io::Error),
    /// The file is larger than [`MAX_IMAGE_FILE_BYTES`].
    #[error(
        "the image is {} bytes, over the {max}-byte (10 MiB) limit",
        .bytes.map_or_else(|| format!("more than {max}"), |b| b.to_string())
    )]
    FileTooLarge {
        /// The file size, when known; `None` when a read without a known
        /// size went past the limit and stopped.
        bytes: Option<u64>,
        /// [`MAX_IMAGE_FILE_BYTES`].
        max: u64,
    },
    /// The file is empty.
    #[error("the image file is empty")]
    Empty,
    /// The header is not a PNG, JPEG or WebP signature.
    #[error("not a png, jpeg or webp image (the header matches none of them)")]
    UnsupportedFormat,
    /// The file ends inside its own signature.
    #[error("the {kind} image is truncated after {bytes} bytes")]
    Truncated {
        /// The format the signature belongs to.
        kind: ImageKind,
        /// The file size.
        bytes: usize,
    },
    /// The image is wider or taller than [`MAX_IMAGE_SIDE`].
    #[error(
        "the image{} exceeds the {max}x{max} pixel limit",
        .dimensions.map(|(w, h)| format!(" is {w}x{h} pixels and")).unwrap_or_default()
    )]
    DimensionsTooLarge {
        /// The declared width and height, when the decoder reported them.
        dimensions: Option<(u32, u32)>,
        /// [`MAX_IMAGE_SIDE`].
        max: u32,
    },
    /// Decoding would allocate more than [`MAX_IMAGE_ALLOC`].
    #[error(
        "decoding the image needs {}, over the {max}-byte (128 MiB) limit",
        .bytes.map_or_else(|| "more memory".to_owned(), |b| format!("{b} bytes"))
    )]
    TooMuchMemory {
        /// The bytes needed, when known.
        bytes: Option<u64>,
        /// [`MAX_IMAGE_ALLOC`].
        max: u64,
    },
    /// The data is not a valid image of its format (corrupt or truncated).
    #[error("cannot decode the {kind} image: {message}")]
    Decode {
        /// The format named by the header.
        kind: ImageKind,
        /// The decoder's description of the problem.
        message: String,
    },
}

impl LoadError {
    /// Whether the image itself is unacceptable (as opposed to reading it
    /// failing): a usage error rather than an I/O error.
    pub const fn is_invalid_image(&self) -> bool {
        !matches!(self, Self::Read(_))
    }
}

/// A decoded image: straight 8-bit RGBA, row-major, `width * height * 4`
/// bytes.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Bitmap {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Bitmap {
    /// A bitmap from RGBA pixels, or `None` if a side is zero, a side is
    /// above [`MAX_IMAGE_SIDE`], or `pixels` is not `width * height * 4`
    /// bytes.
    ///
    /// ```
    /// use qr_core::loader::Bitmap;
    /// let red = Bitmap::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
    /// assert_eq!((red.width(), red.height()), (1, 1));
    /// assert!(Bitmap::from_rgba(2, 1, vec![0; 4]).is_none());
    /// ```
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        let sides_ok =
            (1..=MAX_IMAGE_SIDE).contains(&width) && (1..=MAX_IMAGE_SIDE).contains(&height);
        let len = rgba_len(width, height)?;
        (sides_ok && usize::try_from(len).ok()? == pixels.len()).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, four bytes (red, green, blue, alpha) each, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The pixels, giving up the bitmap, so they can be converted in place.
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Whether every pixel is fully opaque.
    pub fn is_opaque(&self) -> bool {
        self.pixels.chunks_exact(4).all(|px| px[3] == u8::MAX)
    }
}

impl fmt::Debug for Bitmap {
    // The pixels would flood any log; the size says what matters.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bitmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// `width * height * 4`, or `None` on overflow.
fn rgba_len(width: u32, height: u32) -> Option<u64> {
    u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)
}

/// Reads an image from `reader` and decodes it within the limits.
///
/// `declared_len` is the size the caller expects, such as a file's
/// metadata length: when it is above [`MAX_IMAGE_FILE_BYTES`] nothing is
/// read. The read itself is bounded too, so a file that grows, or a
/// stream with no length, can never be read past the limit.
///
/// # Errors
///
/// [`LoadError::Read`] if reading fails; any other [`LoadError`] if the
/// image breaks a limit or does not decode (see [`decode_image`]).
///
/// ```
/// use qr_core::loader::{LoadError, load_image};
/// let err = load_image(&b"GIF89a"[..], Some(6)).unwrap_err();
/// assert!(matches!(err, LoadError::UnsupportedFormat));
/// let err = load_image(std::io::empty(), Some(11 << 20)).unwrap_err();
/// assert!(matches!(err, LoadError::FileTooLarge { .. }));
/// ```
pub fn load_image(reader: impl Read, declared_len: Option<u64>) -> Result<Bitmap, LoadError> {
    let too_large = |bytes| LoadError::FileTooLarge {
        bytes,
        max: MAX_IMAGE_FILE_BYTES,
    };
    if let Some(len) = declared_len.filter(|len| *len > MAX_IMAGE_FILE_BYTES) {
        return Err(too_large(Some(len)));
    }
    let mut bytes = Vec::new();
    // Reserve the declared size up front (it is within the limit), but
    // fallibly: a failed reservation just means growing as the read goes.
    if let Some(len) = declared_len.and_then(|len| usize::try_from(len).ok()) {
        let _ = bytes.try_reserve_exact(len);
    }
    reader
        .take(MAX_IMAGE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(LoadError::Read)?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read > MAX_IMAGE_FILE_BYTES {
        return Err(too_large(None));
    }
    decode_image(&bytes)
}

/// Decodes an image already in memory within the limits.
///
/// # Errors
///
/// - [`LoadError::FileTooLarge`] above [`MAX_IMAGE_FILE_BYTES`];
/// - [`LoadError::Empty`], [`LoadError::Truncated`] or
///   [`LoadError::UnsupportedFormat`] if the header is not a PNG, JPEG or
///   WebP signature;
/// - [`LoadError::DimensionsTooLarge`] or [`LoadError::TooMuchMemory`] if
///   the header declares an image beyond the limits, before any pixel is
///   decoded;
/// - [`LoadError::Decode`] if the data is corrupt or cut short.
///
/// ```
/// use qr_core::loader::{ImageKind, decode_image};
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_png};
/// let code = encode(b"hi", &EncodeOptions::default())?;
/// let png = render_png(code.matrix(), &RenderOptions::default())?;
/// let bitmap = decode_image(&png)?;
/// assert_eq!((bitmap.width(), bitmap.height()), (290, 290));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn decode_image(bytes: &[u8]) -> Result<Bitmap, LoadError> {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > MAX_IMAGE_FILE_BYTES {
        return Err(LoadError::FileTooLarge {
            bytes: Some(len),
            max: MAX_IMAGE_FILE_BYTES,
        });
    }
    if bytes.is_empty() {
        return Err(LoadError::Empty);
    }
    let Some(kind) = ImageKind::sniff(bytes) else {
        return Err(
            ImageKind::truncated(bytes).map_or(LoadError::UnsupportedFormat, |kind| {
                LoadError::Truncated {
                    kind,
                    bytes: bytes.len(),
                }
            }),
        );
    };
    let error = |e: ImageError| map_image_error(kind, e);

    let mut reader = ImageReader::with_format(Cursor::new(bytes), kind.image_format());
    reader.limits(limits());
    let decoder = reader.into_decoder().map_err(error)?;

    // The limits again, with the numbers: the allocation limit is only
    // advisory for some decoders.
    let (width, height) = decoder.dimensions();
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(LoadError::DimensionsTooLarge {
            dimensions: Some((width, height)),
            max: MAX_IMAGE_SIDE,
        });
    }
    if width == 0 || height == 0 {
        return Err(LoadError::Decode {
            kind,
            message: format!("the image is {width}x{height} pixels"),
        });
    }
    let rgba = rgba_len(width, height).unwrap_or(u64::MAX);
    let needed = decoder.total_bytes().max(rgba);
    if needed > MAX_IMAGE_ALLOC {
        return Err(LoadError::TooMuchMemory {
            bytes: Some(needed),
            max: MAX_IMAGE_ALLOC,
        });
    }

    let image = DynamicImage::from_decoder(decoder).map_err(error)?;
    // The JPEG decoder fills in a scan that is cut short rather than
    // failing, so a truncated JPEG is caught by its missing end marker.
    if kind == ImageKind::Jpeg && !jpeg_is_complete(bytes) {
        return Err(LoadError::Decode {
            kind,
            message: "the file is truncated (no end-of-image marker after the last scan)"
                .to_owned(),
        });
    }
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    Bitmap::from_rgba(width, height, rgba.into_raw()).ok_or_else(|| LoadError::Decode {
        kind,
        message: "the decoder returned an image of the wrong size".to_owned(),
    })
}

/// Whether a JPEG has an end-of-image marker (`FF D9`) after its last
/// start-of-scan marker (`FF DA`). Entropy-coded data escapes every `FF`
/// byte, so neither marker can appear inside a scan by accident.
fn jpeg_is_complete(bytes: &[u8]) -> bool {
    let marker = |m: u8| move |w: &[u8]| w == [0xff, m];
    let Some(scan) = bytes.windows(2).rposition(marker(0xda)) else {
        return false;
    };
    bytes[scan..].windows(2).any(marker(0xd9))
}

/// The decoder limits: [`MAX_IMAGE_SIDE`] per side, [`MAX_IMAGE_ALLOC`]
/// bytes.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC);
    limits
}

fn map_image_error(kind: ImageKind, error: ImageError) -> LoadError {
    match error {
        ImageError::Limits(limit) => match limit.kind() {
            LimitErrorKind::DimensionError => LoadError::DimensionsTooLarge {
                dimensions: None,
                max: MAX_IMAGE_SIDE,
            },
            _ => LoadError::TooMuchMemory {
                bytes: None,
                max: MAX_IMAGE_ALLOC,
            },
        },
        other => LoadError::Decode {
            kind,
            message: lowercase_first(&other.to_string()),
        },
    }
}

/// `text` with its first letter lowered, to match the CLI's message style.
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests;
