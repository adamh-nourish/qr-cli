//! Encoding options: error correction level, symbol version, data mode and
//! mask pattern, plus [`EncodeOptions`] which bundles them.
//!
//! Every type parses from the spelling the CLI accepts (`FromStr`) and prints
//! back the same way (`Display`), so error messages can quote options exactly
//! as the user would type them.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// An option value that could not be parsed or is out of range.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OptionError {
    /// The error correction level is not one of `L`, `M`, `Q` or `H`.
    #[error("unknown error correction level '{0}' (expected L, M, Q or H)")]
    EcLevel(String),
    /// The symbol version is not a number from 1 to 40.
    #[error("invalid version '{0}' (expected a number from 1 to 40)")]
    Version(String),
    /// The data mode is not one of the supported names.
    #[error("unknown mode '{0}' (expected auto, numeric, alnum or byte)")]
    Mode(String),
    /// The mask pattern is not a number from 0 to 7.
    #[error("invalid mask '{0}' (expected a number from 0 to 7)")]
    Mask(String),
}

/// Error correction level: how much of the symbol can be damaged and still
/// decode. Higher levels hold less data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum EcLevel {
    /// Low: about 7% of codewords can be restored.
    L,
    /// Medium: about 15% of codewords can be restored. The default.
    #[default]
    M,
    /// Quartile: about 25% of codewords can be restored.
    Q,
    /// High: about 30% of codewords can be restored.
    H,
}

impl EcLevel {
    /// All levels, from lowest to highest redundancy.
    pub const ALL: [Self; 4] = [Self::L, Self::M, Self::Q, Self::H];

    /// Index into per-level tables, in `L, M, Q, H` order.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::L => 0,
            Self::M => 1,
            Self::Q => 2,
            Self::H => 3,
        }
    }
}

impl fmt::Display for EcLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::L => "L",
            Self::M => "M",
            Self::Q => "Q",
            Self::H => "H",
        })
    }
}

impl FromStr for EcLevel {
    type Err = OptionError;

    /// Parses `L`, `M`, `Q` or `H`, ignoring ASCII case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "L" => Ok(Self::L),
            "M" => Ok(Self::M),
            "Q" => Ok(Self::Q),
            "H" => Ok(Self::H),
            _ => Err(OptionError::EcLevel(s.to_owned())),
        }
    }
}

/// A QR code symbol version from 1 (21×21 modules) to 40 (177×177 modules).
///
/// Micro QR versions are not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version(u8);

impl Version {
    /// The smallest version, 21×21 modules.
    pub const MIN: Self = Self(1);
    /// The largest version, 177×177 modules.
    pub const MAX: Self = Self(40);

    /// Creates a version, or returns `None` if `n` is not in `1..=40`.
    ///
    /// ```
    /// use qr_core::Version;
    /// assert_eq!(Version::new(5).map(Version::number), Some(5));
    /// assert!(Version::new(0).is_none());
    /// assert!(Version::new(41).is_none());
    /// ```
    pub const fn new(n: u8) -> Option<Self> {
        if n >= 1 && n <= 40 {
            Some(Self(n))
        } else {
            None
        }
    }

    /// The version number, `1..=40`.
    pub const fn number(self) -> u8 {
        self.0
    }

    /// Width (and height) of the symbol in modules, excluding the quiet zone.
    ///
    /// ```
    /// use qr_core::Version;
    /// assert_eq!(Version::MIN.width(), 21);
    /// assert_eq!(Version::MAX.width(), 177);
    /// ```
    pub const fn width(self) -> usize {
        self.0 as usize * 4 + 17
    }

    /// Iterates over every version from `self` up to [`Version::MAX`].
    pub fn up_to_max(self) -> impl Iterator<Item = Self> {
        (self.0..=Self::MAX.0).map(Self)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Version {
    type Err = OptionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim()
            .parse::<u8>()
            .ok()
            .and_then(Self::new)
            .ok_or_else(|| OptionError::Version(s.to_owned()))
    }
}

/// How data is packed into the symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Digits `0-9` only; about 3.3 bits per character.
    Numeric,
    /// Digits, upper-case `A-Z`, space and `$%*+-./:`; 5.5 bits per character.
    Alphanumeric,
    /// Arbitrary bytes; 8 bits per byte.
    Byte,
}

impl Mode {
    /// Whether `byte` can be represented in this mode.
    pub const fn accepts(self, byte: u8) -> bool {
        match self {
            Self::Numeric => byte.is_ascii_digit(),
            Self::Alphanumeric => alphanumeric_value(byte).is_some(),
            Self::Byte => true,
        }
    }

    /// The narrowest mode that can represent every byte of `data`.
    ///
    /// ```
    /// use qr_core::Mode;
    /// assert_eq!(Mode::narrowest_for(b"0123"), Mode::Numeric);
    /// assert_eq!(Mode::narrowest_for(b"HELLO WORLD"), Mode::Alphanumeric);
    /// assert_eq!(Mode::narrowest_for(b"hello"), Mode::Byte);
    /// ```
    pub fn narrowest_for(data: &[u8]) -> Self {
        if data.iter().all(u8::is_ascii_digit) {
            Self::Numeric
        } else if data.iter().all(|&b| Self::Alphanumeric.accepts(b)) {
            Self::Alphanumeric
        } else {
            Self::Byte
        }
    }

    /// The unit a capacity in this mode is counted in.
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Numeric => "digits",
            Self::Alphanumeric => "characters",
            Self::Byte => "bytes",
        }
    }
}

/// The value of an alphanumeric-mode character (ISO/IEC 18004 table 5).
pub(crate) const fn alphanumeric_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'Z' => Some(byte - b'A' + 10),
        b' ' => Some(36),
        b'$' => Some(37),
        b'%' => Some(38),
        b'*' => Some(39),
        b'+' => Some(40),
        b'-' => Some(41),
        b'.' => Some(42),
        b'/' => Some(43),
        b':' => Some(44),
        _ => None,
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Numeric => "numeric",
            Self::Alphanumeric => "alnum",
            Self::Byte => "byte",
        })
    }
}

/// The user's choice of data mode: a fixed [`Mode`] or automatic selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModeChoice {
    /// Use the narrowest mode that represents all of the data.
    #[default]
    Auto,
    /// Use exactly this mode; data outside its character set is an error.
    Fixed(Mode),
}

impl fmt::Display for ModeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Fixed(mode) => mode.fmt(f),
        }
    }
}

impl FromStr for ModeChoice {
    type Err = OptionError;

    /// Parses `auto`, `numeric`, `alnum` (or `alphanumeric`) or `byte`,
    /// ignoring ASCII case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "numeric" => Ok(Self::Fixed(Mode::Numeric)),
            "alnum" | "alphanumeric" => Ok(Self::Fixed(Mode::Alphanumeric)),
            "byte" => Ok(Self::Fixed(Mode::Byte)),
            _ => Err(OptionError::Mode(s.to_owned())),
        }
    }
}

/// How the symbol version is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VersionChoice {
    /// The smallest version that fits the data.
    #[default]
    Auto,
    /// The smallest version that fits the data and is at least this one.
    AtLeast(Version),
    /// Exactly this version; data that does not fit is an error.
    ///
    /// No v1 CLI flag maps to this (`--qr-version` is [`Self::AtLeast`]).
    /// It exists for tests, benchmarks and library callers that need a
    /// fixed symbol size.
    Exact(Version),
}

impl VersionChoice {
    /// The smallest version this choice allows.
    pub const fn min(self) -> Version {
        match self {
            Self::Auto => Version::MIN,
            Self::AtLeast(v) | Self::Exact(v) => v,
        }
    }

    /// The largest version this choice allows.
    pub const fn max(self) -> Version {
        match self {
            Self::Auto | Self::AtLeast(_) => Version::MAX,
            Self::Exact(v) => v,
        }
    }
}

/// One of the eight data mask patterns (ISO/IEC 18004 table 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mask(u8);

impl Mask {
    /// Creates a mask, or returns `None` if `n` is not in `0..=7`.
    ///
    /// ```
    /// use qr_core::Mask;
    /// assert_eq!(Mask::new(7).map(Mask::number), Some(7));
    /// assert!(Mask::new(8).is_none());
    /// ```
    pub const fn new(n: u8) -> Option<Self> {
        if n <= 7 { Some(Self(n)) } else { None }
    }

    /// The mask pattern number, `0..=7`.
    pub const fn number(self) -> u8 {
        self.0
    }

    /// All eight masks in pattern-number order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..=7).map(Self)
    }
}

impl fmt::Display for Mask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Mask {
    type Err = OptionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim()
            .parse::<u8>()
            .ok()
            .and_then(Self::new)
            .ok_or_else(|| OptionError::Mask(s.to_owned()))
    }
}

/// Everything an encoder needs besides the data itself.
///
/// The default is error correction level M, automatic mode, automatic
/// version and automatic (lowest penalty) mask.
///
/// ```
/// use qr_core::{EcLevel, EncodeOptions, Mask, Mode, ModeChoice, Version, VersionChoice};
///
/// let options = EncodeOptions::default()
///     .with_ec_level(EcLevel::H)
///     .with_mode(ModeChoice::Fixed(Mode::Byte))
///     .with_version(VersionChoice::AtLeast(Version::new(5).unwrap()))
///     .with_mask(Mask::new(3));
/// assert_eq!(options.ec_level, EcLevel::H);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct EncodeOptions {
    /// Error correction level.
    pub ec_level: EcLevel,
    /// Data mode selection.
    pub mode: ModeChoice,
    /// Version selection.
    pub version: VersionChoice,
    /// A forced mask pattern, or `None` to pick the lowest-penalty mask.
    pub mask: Option<Mask>,
}

impl EncodeOptions {
    /// Returns these options with the given error correction level.
    #[must_use]
    pub const fn with_ec_level(mut self, ec_level: EcLevel) -> Self {
        self.ec_level = ec_level;
        self
    }

    /// Returns these options with the given mode selection.
    #[must_use]
    pub const fn with_mode(mut self, mode: ModeChoice) -> Self {
        self.mode = mode;
        self
    }

    /// Returns these options with the given version selection.
    #[must_use]
    pub const fn with_version(mut self, version: VersionChoice) -> Self {
        self.version = version;
        self
    }

    /// Returns these options with the given mask (`None` = automatic).
    #[must_use]
    pub const fn with_mask(mut self, mask: Option<Mask>) -> Self {
        self.mask = mask;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ec_level_round_trips_through_strings() {
        for level in EcLevel::ALL {
            assert_eq!(level.to_string().parse::<EcLevel>(), Ok(level));
            assert_eq!(
                level.to_string().to_ascii_lowercase().parse::<EcLevel>(),
                Ok(level)
            );
        }
        assert_eq!(
            "X".parse::<EcLevel>(),
            Err(OptionError::EcLevel("X".into()))
        );
        assert_eq!(EcLevel::default(), EcLevel::M);
    }

    #[test]
    fn ec_level_indices_follow_table_order() {
        let indices: Vec<usize> = EcLevel::ALL.iter().map(|l| l.index()).collect();
        assert_eq!(indices, [0, 1, 2, 3]);
    }

    #[test]
    fn version_bounds_and_parsing() {
        assert_eq!(Version::new(1), Some(Version::MIN));
        assert_eq!(Version::new(40), Some(Version::MAX));
        assert_eq!(" 7 ".parse::<Version>().map(Version::number), Ok(7));
        for bad in ["0", "41", "-1", "x", "", "300"] {
            assert_eq!(
                bad.parse::<Version>(),
                Err(OptionError::Version(bad.into()))
            );
        }
        assert_eq!(Version::MAX.to_string(), "40");
        assert_eq!(Version::MIN.up_to_max().count(), 40);
        assert_eq!(Version::MAX.up_to_max().count(), 1);
    }

    #[test]
    fn mode_choice_parsing_and_display() {
        let cases = [
            ("auto", ModeChoice::Auto),
            ("NUMERIC", ModeChoice::Fixed(Mode::Numeric)),
            ("alnum", ModeChoice::Fixed(Mode::Alphanumeric)),
            ("alphanumeric", ModeChoice::Fixed(Mode::Alphanumeric)),
            ("byte", ModeChoice::Fixed(Mode::Byte)),
        ];
        for (text, choice) in cases {
            assert_eq!(text.parse::<ModeChoice>(), Ok(choice));
        }
        for choice in [
            ModeChoice::Auto,
            ModeChoice::Fixed(Mode::Numeric),
            ModeChoice::Fixed(Mode::Alphanumeric),
            ModeChoice::Fixed(Mode::Byte),
        ] {
            assert_eq!(choice.to_string().parse::<ModeChoice>(), Ok(choice));
        }
        assert_eq!(
            "kanji".parse::<ModeChoice>(),
            Err(OptionError::Mode("kanji".into()))
        );
    }

    #[test]
    fn mode_character_sets() {
        assert!(Mode::Numeric.accepts(b'7'));
        assert!(!Mode::Numeric.accepts(b'A'));
        for b in b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:" {
            assert!(Mode::Alphanumeric.accepts(*b), "{}", *b as char);
        }
        for b in b"az#!,;?=\0\xff" {
            assert!(!Mode::Alphanumeric.accepts(*b), "{}", *b as char);
        }
        assert!((0..=255).all(|b| Mode::Byte.accepts(b)));
        assert_eq!(alphanumeric_value(b':'), Some(44));
        assert_eq!(Mode::narrowest_for(b""), Mode::Numeric);
        assert_eq!(Mode::Numeric.unit(), "digits");
        assert_eq!(Mode::Alphanumeric.unit(), "characters");
        assert_eq!(Mode::Byte.unit(), "bytes");
    }

    #[test]
    fn version_choice_bounds() {
        let v5 = Version::new(5).unwrap();
        assert_eq!(VersionChoice::Auto.min(), Version::MIN);
        assert_eq!(VersionChoice::Auto.max(), Version::MAX);
        assert_eq!(VersionChoice::AtLeast(v5).min(), v5);
        assert_eq!(VersionChoice::AtLeast(v5).max(), Version::MAX);
        assert_eq!(VersionChoice::Exact(v5).min(), v5);
        assert_eq!(VersionChoice::Exact(v5).max(), v5);
    }

    #[test]
    fn mask_bounds_and_parsing() {
        assert_eq!(Mask::all().count(), 8);
        assert_eq!("3".parse::<Mask>().map(Mask::number), Ok(3));
        assert_eq!("8".parse::<Mask>(), Err(OptionError::Mask("8".into())));
        assert_eq!(Mask::new(0).unwrap().to_string(), "0");
    }

    #[test]
    fn options_builder_sets_every_field() {
        let v = Version::new(9).unwrap();
        let m = Mask::new(2);
        let o = EncodeOptions::default()
            .with_ec_level(EcLevel::Q)
            .with_mode(ModeChoice::Fixed(Mode::Numeric))
            .with_version(VersionChoice::Exact(v))
            .with_mask(m);
        assert_eq!(o.ec_level, EcLevel::Q);
        assert_eq!(o.mode, ModeChoice::Fixed(Mode::Numeric));
        assert_eq!(o.version, VersionChoice::Exact(v));
        assert_eq!(o.mask, m);
    }

    #[test]
    fn option_errors_quote_the_input() {
        assert_eq!(
            OptionError::Version("99".into()).to_string(),
            "invalid version '99' (expected a number from 1 to 40)"
        );
        assert!(OptionError::Mask("9".into()).to_string().contains("'9'"));
        assert!(OptionError::EcLevel("Z".into()).to_string().contains("'Z'"));
        assert!(OptionError::Mode("k".into()).to_string().contains("'k'"));
    }
}
