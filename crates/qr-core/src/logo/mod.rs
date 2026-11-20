//! Logos drawn over the centre of a code (`--logo`, DECISIONS section 3).
//!
//! A [`Logo`] is a decoded [`Bitmap`] (load it with the bounded
//! [`loader`](crate::loader)) and a [`LogoScale`]: its width as a fraction
//! of the symbol's width, from [`LogoScale::MIN`] (0.05) to
//! [`LogoScale::MAX`] (0.3, about 9% of the area; DECISIONS section 0,
//! item 4). Set it with
//! [`RenderOptions::with_logo`](crate::RenderOptions::with_logo).
//!
//! Every renderer places it the same way, in module units, so a PNG, an
//! SVG and the drawing that verification reads all agree:
//!
//! - the logo keeps its aspect ratio inside a square box of `scale` times
//!   the symbol width, centred on the symbol;
//! - the modules under the logo and a pad of half a module around it are
//!   cleared to the background colour (whole modules, so no module is
//!   half covered), and the logo is painted over them;
//! - nothing within [`KEEP_OUT`] modules of the symbol's edge is ever
//!   cleared: the finder patterns, separators, timing patterns and format
//!   information stay intact, which caps the box on small symbols.
//!
//! Clearing modules relies on error correction to recover them, so a logo
//! needs EC level H; the CLI forces it and verifies every logo code.

mod geometry;
mod resample;

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use thiserror::Error;

pub(crate) use self::geometry::{Geometry, PixelRect};
pub(crate) use self::resample::resample;
use crate::loader::Bitmap;
use crate::matrix::Matrix;
use crate::render::{RenderError, RenderOptions};

/// The modules along each edge of the symbol that a logo never clears:
/// the 7-module finder patterns, their separator and the format
/// information next to them (which ends at module 8), plus one more.
pub const KEEP_OUT: usize = 9;

/// The width of a logo as a fraction of the symbol's width, held exactly
/// in thousandths.
///
/// ```
/// use qr_core::logo::LogoScale;
/// let scale: LogoScale = "0.25".parse()?;
/// assert_eq!(scale.permille(), 250);
/// assert_eq!(scale.to_string(), "0.25");
/// assert!("0.4".parse::<LogoScale>().is_err());
/// assert_eq!(LogoScale::default(), LogoScale::DEFAULT);
/// # Ok::<(), qr_core::logo::LogoScaleError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LogoScale(u16);

impl LogoScale {
    /// The smallest scale: 0.05.
    pub const MIN: Self = Self(50);
    /// The largest scale: 0.3, about 9% of the symbol's area.
    pub const MAX: Self = Self(300);
    /// The default scale: 0.2, about 4% of the area.
    pub const DEFAULT: Self = Self(200);

    /// The scale of `permille` thousandths of the symbol width.
    ///
    /// # Errors
    ///
    /// [`LogoScaleError`] outside [`Self::MIN`] to [`Self::MAX`].
    pub fn from_permille(permille: u16) -> Result<Self, LogoScaleError> {
        if (Self::MIN.0..=Self::MAX.0).contains(&permille) {
            Ok(Self(permille))
        } else {
            Err(LogoScaleError(Self(permille).to_string()))
        }
    }

    /// The scale in thousandths of the symbol width.
    pub const fn permille(self) -> u16 {
        self.0
    }
}

impl Default for LogoScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for LogoScale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (whole, mut frac) = (self.0 / 1000, self.0 % 1000);
        write!(f, "{whole}")?;
        if frac == 0 {
            return Ok(());
        }
        let mut digits = 3;
        while frac % 10 == 0 {
            frac /= 10;
            digits -= 1;
        }
        write!(f, ".{frac:0digits$}")
    }
}

/// A logo scale was not a number from 0.05 to 0.3.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("logo scale '{0}' is out of range (expected a fraction of the width from 0.05 to 0.3)")]
pub struct LogoScaleError(String);

impl FromStr for LogoScale {
    type Err = LogoScaleError;

    /// Parses a decimal fraction such as `0.2`. At most three decimals
    /// count; the value must lie from 0.05 to 0.3.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let shown = || LogoScaleError(s.chars().take(32).collect());
        let value: f64 = s.trim().parse().map_err(|_| shown())?;
        let (min, max) = (f64::from(Self::MIN.0), f64::from(Self::MAX.0));
        let permille = value * 1000.0;
        if !permille.is_finite() || !(min - 1e-9..=max + 1e-9).contains(&permille) {
            return Err(shown());
        }
        // In range, so the rounded value fits.
        let rounded = permille.round().clamp(min, max);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let permille = rounded as u16;
        Self::from_permille(permille).map_err(|_| shown())
    }
}

/// A logo to draw over the centre of a code: the image and its size.
///
/// Cloning is cheap: the pixels are shared.
///
/// ```
/// use qr_core::loader::Bitmap;
/// use qr_core::logo::{Logo, LogoScale};
/// let image = Bitmap::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
/// let logo = Logo::new(image).with_scale(LogoScale::MAX);
/// assert_eq!(logo.scale(), LogoScale::MAX);
/// assert_eq!(logo.image().width(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Logo {
    image: Arc<Bitmap>,
    scale: LogoScale,
}

impl Logo {
    /// A logo of `image` at [`LogoScale::DEFAULT`].
    pub fn new(image: Bitmap) -> Self {
        Self {
            image: Arc::new(image),
            scale: LogoScale::DEFAULT,
        }
    }

    /// Returns this logo at `scale`.
    #[must_use]
    pub fn with_scale(mut self, scale: LogoScale) -> Self {
        self.scale = scale;
        self
    }

    /// The image.
    pub fn image(&self) -> &Bitmap {
        &self.image
    }

    /// The width as a fraction of the symbol's width.
    pub fn scale(&self) -> LogoScale {
        self.scale
    }
}

/// A logo planned onto one symbol: where it goes, and the symbol with the
/// modules under it cleared, which is what gets drawn.
#[derive(Debug, Clone)]
pub(crate) struct Planned<'o> {
    /// The logo.
    pub(crate) logo: &'o Logo,
    /// Where it goes.
    pub(crate) geometry: Geometry,
    /// The symbol with the hole cleared.
    pub(crate) matrix: Matrix,
}

impl<'o> Planned<'o> {
    /// The plan for `options.logo` on `matrix`, or `None` without a logo.
    ///
    /// # Errors
    ///
    /// [`RenderError::LogoNoRoom`] if the symbol is too small to clear
    /// anything outside [`KEEP_OUT`].
    pub(crate) fn new(
        matrix: &Matrix,
        options: &'o RenderOptions,
    ) -> Result<Option<Self>, RenderError> {
        let Some(logo) = &options.logo else {
            return Ok(None);
        };
        let geometry = Geometry::new(matrix.width(), logo).ok_or(RenderError::LogoNoRoom {
            modules: matrix.width(),
        })?;
        Ok(Some(Self {
            logo,
            geometry,
            matrix: geometry.clear(matrix),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_follow_the_options() {
        let code = crate::encode(b"plan", &crate::EncodeOptions::default()).unwrap();
        assert!(
            Planned::new(code.matrix(), &RenderOptions::default())
                .unwrap()
                .is_none()
        );
        let image = Bitmap::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
        let options = RenderOptions::default().with_logo(Some(Logo::new(image)));
        let plan = Planned::new(code.matrix(), &options).unwrap().unwrap();
        assert!(plan.matrix.dark_count() < code.matrix().dark_count());
        let tiny = Matrix::from_modules(3, vec![true; 9]).unwrap();
        assert_eq!(
            Planned::new(&tiny, &options).unwrap_err(),
            RenderError::LogoNoRoom { modules: 3 }
        );
    }

    #[test]
    fn scales_parse_and_print_exactly() {
        for (text, permille, shown) in [
            ("0.05", 50, "0.05"),
            ("0.2", 200, "0.2"),
            (".3", 300, "0.3"),
            (" 0.125 ", 125, "0.125"),
            ("0.30", 300, "0.3"),
            ("0.0500001", 50, "0.05"),
        ] {
            let scale: LogoScale = text.parse().unwrap();
            assert_eq!(scale.permille(), permille, "{text}");
            assert_eq!(scale.to_string(), shown, "{text}");
        }
        for bad in ["0.049", "0.31", "1", "-0.2", "NaN", "inf", "", "x", "0.2.1"] {
            let err = bad.parse::<LogoScale>().unwrap_err();
            assert!(err.to_string().contains("from 0.05 to 0.3"), "{bad}: {err}");
        }
        assert_eq!(
            LogoScale::from_permille(301).unwrap_err().to_string(),
            "logo scale '0.301' is out of range (expected a fraction of the width from 0.05 to 0.3)"
        );
        assert_eq!(LogoScale::from_permille(49).unwrap_err().0, "0.049");
        assert_eq!(LogoScale(1000).to_string(), "1");
        assert!(LogoScale::MIN < LogoScale::MAX);
    }

    #[test]
    fn logos_share_their_pixels() {
        let image = Bitmap::from_rgba(2, 1, vec![0; 8]).unwrap();
        let logo = Logo::new(image.clone());
        let copy = logo.clone().with_scale(LogoScale::MIN);
        assert!(Arc::ptr_eq(&logo.image, &copy.image));
        assert_eq!(*copy.image(), image);
        assert_ne!(logo, copy);
        assert_eq!(
            format!("{:?}", logo.image()),
            "Bitmap { width: 2, height: 1, .. }"
        );
    }
}
