//! Finding and reading every QR code in an image (`qr decode`, DECISIONS
//! section 2).
//!
//! The image comes from the bounded [`loader`](crate::loader), so its
//! size is already within limits. [`decode_bitmap`] flattens it to luma
//! (transparent pixels on white, as a page would show them) and hands it
//! to `rqrr`, which finds and decodes every symbol. An image in which
//! nothing decodes is read once more with its luma inverted, for
//! light-on-dark codes, as phone scanners do.
//!
//! # The `rqrr` guard
//!
//! `rqrr` 0.9 trips an internal assertion when a module is a single pixel
//! wide, which would abort a release build (`panic = "abort"`). The
//! verifier found this and enlarges what it reads to at least three pixels
//! per module ([`crate::verify`]). Here the module size is unknown, so the
//! image is enlarged by the same factor (by mapping coordinates, never by
//! copying), which makes every module at least three pixels wide whatever
//! its size. Enlarging is bounded by [`DECODE_MAX_READ_PIXELS`]: a larger
//! image is enlarged by a smaller factor, or read at its own size. A QR
//! code with one-pixel modules is at most 185 pixels across (version 40
//! with a four-module quiet zone), so only such a tiny code inside a much
//! larger image is read without the full guard.
//!
//! # Memory
//!
//! The RGBA pixels are converted to luma in place, so at most the loaded
//! bitmap is held, plus `rqrr`'s own copy of the image it reads (one byte
//! per pixel, and a clone per candidate symbol).

use thiserror::Error;

use crate::color::Color;
use crate::finders;
pub use crate::finders::MAX_FINDER_CANDIDATES;
use crate::loader::Bitmap;
use crate::options::{EcLevel, Mask, Version};
use crate::render::raster::luma;
use crate::verify::MIN_READ_SCALE;

/// The most pixels an image may be enlarged to before `rqrr` reads it:
/// 4,096 x 4,096. An image of up to 1,365 x 1,365 pixels is read at three
/// times its size, up to 2,048 x 2,048 at twice its size, and anything
/// larger at its own size.
pub const DECODE_MAX_READ_PIXELS: u64 = 4_096 * 4_096;

/// A point in the source image, in pixels from the top-left corner. A
/// corner found by perspective mapping can lie slightly outside the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    /// Pixels from the left edge.
    pub x: i32,
    /// Pixels from the top edge.
    pub y: i32,
}

/// One code read from an image.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DecodedCode {
    /// The data bytes, exactly as encoded.
    pub payload: Vec<u8>,
    /// The symbol version.
    pub version: Version,
    /// The error correction level.
    pub ec_level: EcLevel,
    /// The mask pattern.
    pub mask: Mask,
    /// The symbol's corners in the source image: top-left, top-right,
    /// bottom-right and bottom-left, as the symbol is oriented (a rotated
    /// code has its top-left corner wherever its first finder pattern is).
    pub bounds: [Point; 4],
    /// Whether the code was only found with the image's luma inverted
    /// (light modules on a dark background).
    pub inverted: bool,
}

impl std::fmt::Debug for DecodedCode {
    // Payloads can be long; the length says enough in a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedCode")
            .field("bytes", &self.payload.len())
            .field("version", &self.version)
            .field("ec_level", &self.ec_level)
            .field("mask", &self.mask)
            .field("bounds", &self.bounds)
            .field("inverted", &self.inverted)
            .finish()
    }
}

/// Why an image could not be searched for codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// The image holds more finder-like patterns than are searched
    /// ([`MAX_FINDER_CANDIDATES`]). Grouping them would take time that
    /// grows faster than the square of their number, so the image is
    /// refused instead (DECISIONS section 4.3).
    #[error("the image holds more than {limit} finder-like patterns, too many to search")]
    TooManyFinders {
        /// The most finder-pattern candidates searched.
        limit: usize,
    },
}

/// Everything found in one image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// The codes that decoded, top to bottom, then left to right.
    pub codes: Vec<DecodedCode>,
    /// How many symbols were located but could not be decoded (too
    /// damaged, blurred or distorted). Counted in the pass that decoded
    /// codes, or the last pass when none did.
    pub undecodable: usize,
    /// How many times the image was enlarged before it was read (1 to 3).
    pub enlarged: u8,
}

impl Scan {
    /// Whether no code could be read.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

/// Finds and decodes every QR code in `image`.
///
/// The bitmap is consumed, because its pixels are converted to luma in
/// place. Finding nothing is not an error: the [`Scan`] is then empty, and
/// [`Scan::undecodable`] says whether something code-like was seen.
///
/// # Errors
///
/// [`DecodeError::TooManyFinders`] if the image (or, when nothing decodes,
/// its inverse) holds more than [`MAX_FINDER_CANDIDATES`] finder-like
/// patterns. The patterns are counted in one pass over the pixels before
/// `rqrr` groups them, so a hostile image is refused quickly.
///
/// ```
/// use qr_core::decode::decode_bitmap;
/// use qr_core::loader::decode_image;
/// use qr_core::{EcLevel, EncodeOptions, RenderOptions, encode, render_png};
///
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let png = render_png(code.matrix(), &RenderOptions::default())?;
/// let scan = decode_bitmap(decode_image(&png)?)?;
/// assert_eq!(scan.codes.len(), 1);
/// assert_eq!(scan.codes[0].payload, b"hello");
/// assert_eq!(scan.codes[0].ec_level, EcLevel::M);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn decode_bitmap(image: Bitmap) -> Result<Scan, DecodeError> {
    let grey = Grey::from_bitmap(image);
    let factor = read_factor(grey.width, grey.height);
    let normal = scan(&grey, factor, false)?;
    if !normal.is_empty() {
        return Ok(normal);
    }
    let inverted = scan(&grey, factor, true)?;
    Ok(if inverted.is_empty() {
        // Report what the normal pass saw if the inverse saw nothing.
        Scan {
            undecodable: normal.undecodable.max(inverted.undecodable),
            ..inverted
        }
    } else {
        inverted
    })
}

/// An image as one luma byte per pixel, row-major.
struct Grey {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Grey {
    /// Converts RGBA to luma in place, flattening transparency onto white.
    fn from_bitmap(image: Bitmap) -> Self {
        let width = image.width() as usize;
        let height = image.height() as usize;
        let mut pixels = image.into_pixels();
        let count = pixels.len() / 4;
        for i in 0..count {
            // Pixel `i` starts at `4 * i >= i`, so it is read before
            // anything is written over it.
            let at = 4 * i;
            let color = Color::rgba(pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]);
            pixels[i] = luma(color.over(Color::WHITE));
        }
        pixels.truncate(count);
        pixels.shrink_to_fit();
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// How many times to enlarge a `width` x `height` image: the largest
/// factor up to [`MIN_READ_SCALE`] that stays within
/// [`DECODE_MAX_READ_PIXELS`], and at least 1.
fn read_factor(width: usize, height: usize) -> usize {
    let pixels = (width as u64).saturating_mul(height as u64);
    (1..=MIN_READ_SCALE)
        .rev()
        .find(|&f| pixels.saturating_mul((f * f) as u64) <= DECODE_MAX_READ_PIXELS)
        .unwrap_or(1)
}

/// Reads `grey` enlarged `factor` times (inverted if asked) and decodes
/// every symbol `rqrr` finds, unless it holds too many finder patterns.
fn scan(grey: &Grey, factor: usize, invert: bool) -> Result<Scan, DecodeError> {
    let Grey {
        width,
        height,
        pixels,
    } = grey;
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(width * factor, height * factor, |x, y| {
            let luma = pixels
                .get((y / factor) * width + x / factor)
                .copied()
                .unwrap_or(u8::MAX);
            if invert { !luma } else { luma }
        });
    // Pixels of a freshly prepared image are 0 (light) or 1 (dark).
    if finders::too_many(prepared.width(), prepared.height(), |x, y| {
        prepared.get_pixel_at(x, y) == 1
    }) {
        return Err(DecodeError::TooManyFinders {
            limit: MAX_FINDER_CANDIDATES,
        });
    }
    let mut codes = Vec::new();
    let mut undecodable = 0;
    for grid in prepared.detect_grids() {
        let mut payload = Vec::new();
        let decoded = grid
            .decode_to(&mut payload)
            .ok()
            .and_then(|meta| decoded_code(payload, &meta, &grid.bounds, factor, invert));
        match decoded {
            Some(code) => codes.push(code),
            None => undecodable += 1,
        }
    }
    codes.sort_by_key(|code| (code.bounds[0].y, code.bounds[0].x));
    Ok(Scan {
        codes,
        undecodable,
        enlarged: u8::try_from(factor).unwrap_or(u8::MAX),
    })
}

/// A decoded symbol with `rqrr`'s metadata mapped to this crate's types
/// and its corners mapped back to source pixels; `None` if the metadata
/// is out of range.
fn decoded_code(
    payload: Vec<u8>,
    meta: &rqrr::MetaData,
    bounds: &[rqrr::Point; 4],
    factor: usize,
    inverted: bool,
) -> Option<DecodedCode> {
    let version = Version::new(u8::try_from(meta.version.0).ok()?)?;
    let ec_level = ec_level(meta.ecc_level)?;
    let mask = Mask::new(u8::try_from(meta.mask).ok()?)?;
    Some(DecodedCode {
        payload,
        version,
        ec_level,
        mask,
        bounds: corners(bounds, version.width(), factor),
        inverted,
    })
}

/// The symbol's corners in source pixels.
///
/// `rqrr` 0.9 maps its far corners from grid coordinate `width + 1`
/// rather than `width`, one module beyond the symbol, so each is pulled
/// back towards the top-left corner by one part in `width + 1`. That is
/// exact for an image that is not in perspective, and within a fraction
/// of a module otherwise. The result is divided by the enlargement
/// `factor`.
fn corners(bounds: &[rqrr::Point; 4], width: usize, factor: usize) -> [Point; 4] {
    #[allow(clippy::cast_precision_loss)] // Both are small.
    let (keep, factor) = (width as f64 / (width as f64 + 1.0), factor.max(1) as f64);
    let origin = bounds[0];
    let map = |p: rqrr::Point| {
        let axis = |o: i32, v: i32| {
            let pulled = f64::from(o) + f64::from(v - o) * keep;
            // Within the i32 range: between two i32 values, then divided.
            #[allow(clippy::cast_possible_truncation)]
            let value = (pulled / factor).floor() as i32;
            value
        };
        Point {
            x: axis(origin.x, p.x),
            y: axis(origin.y, p.y),
        }
    };
    [
        map(bounds[0]),
        map(bounds[1]),
        map(bounds[2]),
        map(bounds[3]),
    ]
}

/// The level named by the two error correction bits of the format
/// information (ISO/IEC 18004 table 12), which is what `rqrr` reports.
const fn ec_level(bits: u16) -> Option<EcLevel> {
    match bits {
        0b00 => Some(EcLevel::M),
        0b01 => Some(EcLevel::L),
        0b10 => Some(EcLevel::H),
        0b11 => Some(EcLevel::Q),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
