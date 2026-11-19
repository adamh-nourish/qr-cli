//! Colours: parsed once into a typed [`Color`] and written back out by each
//! renderer, so a user-supplied string never reaches an output file.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// The longest colour string echoed back in a [`ColorError`], in characters.
/// Longer input is cut short so an error message stays one readable line.
const MAX_ECHO_CHARS: usize = 32;

/// A colour string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ColorError {
    /// The string was empty.
    #[error("empty colour (expected #RGB, #RRGGBB, #RRGGBBAA or a CSS colour name)")]
    Empty,
    /// The string started with `#` but was not 3, 4, 6 or 8 hex digits.
    #[error("invalid hex colour '{0}' (expected #RGB, #RGBA, #RRGGBB or #RRGGBBAA)")]
    Hex(String),
    /// The string was not a CSS named colour.
    #[error("unknown colour name '{0}' (expected a CSS colour name such as black or navy)")]
    Name(String),
}

/// An sRGB colour with 8-bit channels and straight (not premultiplied) alpha.
///
/// Parse one with [`str::parse`] from `#RGB`, `#RGBA`, `#RRGGBB`,
/// `#RRGGBBAA` (case-insensitive hex) or any CSS named colour, including
/// `transparent`. [`Display`](fmt::Display) prints the canonical lower-case
/// hex form, with the alpha pair only when the colour is not opaque.
///
/// ```
/// use qr_core::Color;
/// let c: Color = "#1A73E8".parse()?;
/// assert_eq!(c, Color::rgb(0x1a, 0x73, 0xe8));
/// assert_eq!(c.to_string(), "#1a73e8");
/// assert_eq!("RebeccaPurple".parse::<Color>()?, Color::rgb(0x66, 0x33, 0x99));
/// assert_eq!("#0008".parse::<Color>()?.to_string(), "#00000088");
/// # Ok::<(), qr_core::ColorError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel: 0 is fully transparent, 255 fully opaque.
    pub a: u8,
}

impl Color {
    /// Opaque black, the default foreground.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Opaque white, the default background.
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// Fully transparent black (CSS `transparent`).
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// A colour with the given alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// This colour with its alpha replaced.
    #[must_use]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Whether the alpha channel is 255.
    pub const fn is_opaque(self) -> bool {
        self.a == u8::MAX
    }

    /// Whether the alpha channel is 0, so painting the colour has no effect.
    pub const fn is_invisible(self) -> bool {
        self.a == 0
    }

    /// The opaque colour seen when this colour is painted over `backdrop`,
    /// which is treated as opaque. Used by formats with no alpha support.
    ///
    /// ```
    /// use qr_core::Color;
    /// let half_black = Color::rgba(0, 0, 0, 128);
    /// assert_eq!(half_black.over(Color::WHITE), Color::rgb(127, 127, 127));
    /// assert_eq!(Color::BLACK.over(Color::WHITE), Color::BLACK);
    /// ```
    #[must_use]
    pub fn over(self, backdrop: Self) -> Self {
        let a = u32::from(self.a);
        let blend = |top: u8, bottom: u8| {
            let mixed = (u32::from(top) * a + u32::from(bottom) * (255 - a) + 127) / 255;
            // `mixed` is a weighted mean of two u8 values, so it fits.
            u8::try_from(mixed).unwrap_or(u8::MAX)
        };
        Self::rgb(
            blend(self.r, backdrop.r),
            blend(self.g, backdrop.g),
            blend(self.b, backdrop.b),
        )
    }

    /// Looks up a CSS named colour, ignoring ASCII case.
    ///
    /// ```
    /// use qr_core::Color;
    /// assert_eq!(Color::from_name("Navy"), Some(Color::rgb(0, 0, 0x80)));
    /// assert_eq!(Color::from_name("transparent"), Some(Color::TRANSPARENT));
    /// assert_eq!(Color::from_name("not-a-colour"), None);
    /// ```
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        if lower == "transparent" {
            return Some(Self::TRANSPARENT);
        }
        NAMED_COLORS
            .binary_search_by(|(n, _)| (*n).cmp(lower.as_str()))
            .ok()
            .and_then(|i| NAMED_COLORS.get(i))
            .map(|&(_, [r, g, b])| Self::rgb(r, g, b))
    }

    /// Parses the hex digits after `#`.
    fn from_hex_digits(digits: &str) -> Option<Self> {
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let nibbles: Vec<u8> = digits
            .bytes()
            .filter_map(|b| char::from(b).to_digit(16))
            .filter_map(|d| u8::try_from(d).ok())
            .collect();
        let pair = |hi: u8, lo: u8| (hi << 4) | lo;
        match *nibbles.as_slice() {
            [r, g, b] => Some(Self::rgb(r * 17, g * 17, b * 17)),
            [r, g, b, a] => Some(Self::rgba(r * 17, g * 17, b * 17, a * 17)),
            [r1, r0, g1, g0, b1, b0] => Some(Self::rgb(pair(r1, r0), pair(g1, g0), pair(b1, b0))),
            [r1, r0, g1, g0, b1, b0, a1, a0] => Some(Self::rgba(
                pair(r1, r0),
                pair(g1, g0),
                pair(b1, b0),
                pair(a1, a0),
            )),
            _ => None,
        }
    }
}

impl Default for Color {
    /// Opaque black.
    fn default() -> Self {
        Self::BLACK
    }
}

impl FromStr for Color {
    type Err = ColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(ColorError::Empty);
        }
        if let Some(digits) = s.strip_prefix('#') {
            return Self::from_hex_digits(digits).ok_or_else(|| ColorError::Hex(echo(s)));
        }
        Self::from_name(s).ok_or_else(|| ColorError::Name(echo(s)))
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if !self.is_opaque() {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

/// The part of rejected input that goes into an error message.
fn echo(s: &str) -> String {
    let mut out: String = s.chars().take(MAX_ECHO_CHARS).collect();
    if s.chars().nth(MAX_ECHO_CHARS).is_some() {
        out.push('…');
    }
    out
}

/// The CSS Color Module Level 4 named colours (`transparent` is handled
/// separately), sorted by name for binary search.
const NAMED_COLORS: [(&str, [u8; 3]); 148] = [
    ("aliceblue", [0xf0, 0xf8, 0xff]),
    ("antiquewhite", [0xfa, 0xeb, 0xd7]),
    ("aqua", [0x00, 0xff, 0xff]),
    ("aquamarine", [0x7f, 0xff, 0xd4]),
    ("azure", [0xf0, 0xff, 0xff]),
    ("beige", [0xf5, 0xf5, 0xdc]),
    ("bisque", [0xff, 0xe4, 0xc4]),
    ("black", [0x00, 0x00, 0x00]),
    ("blanchedalmond", [0xff, 0xeb, 0xcd]),
    ("blue", [0x00, 0x00, 0xff]),
    ("blueviolet", [0x8a, 0x2b, 0xe2]),
    ("brown", [0xa5, 0x2a, 0x2a]),
    ("burlywood", [0xde, 0xb8, 0x87]),
    ("cadetblue", [0x5f, 0x9e, 0xa0]),
    ("chartreuse", [0x7f, 0xff, 0x00]),
    ("chocolate", [0xd2, 0x69, 0x1e]),
    ("coral", [0xff, 0x7f, 0x50]),
    ("cornflowerblue", [0x64, 0x95, 0xed]),
    ("cornsilk", [0xff, 0xf8, 0xdc]),
    ("crimson", [0xdc, 0x14, 0x3c]),
    ("cyan", [0x00, 0xff, 0xff]),
    ("darkblue", [0x00, 0x00, 0x8b]),
    ("darkcyan", [0x00, 0x8b, 0x8b]),
    ("darkgoldenrod", [0xb8, 0x86, 0x0b]),
    ("darkgray", [0xa9, 0xa9, 0xa9]),
    ("darkgreen", [0x00, 0x64, 0x00]),
    ("darkgrey", [0xa9, 0xa9, 0xa9]),
    ("darkkhaki", [0xbd, 0xb7, 0x6b]),
    ("darkmagenta", [0x8b, 0x00, 0x8b]),
    ("darkolivegreen", [0x55, 0x6b, 0x2f]),
    ("darkorange", [0xff, 0x8c, 0x00]),
    ("darkorchid", [0x99, 0x32, 0xcc]),
    ("darkred", [0x8b, 0x00, 0x00]),
    ("darksalmon", [0xe9, 0x96, 0x7a]),
    ("darkseagreen", [0x8f, 0xbc, 0x8f]),
    ("darkslateblue", [0x48, 0x3d, 0x8b]),
    ("darkslategray", [0x2f, 0x4f, 0x4f]),
    ("darkslategrey", [0x2f, 0x4f, 0x4f]),
    ("darkturquoise", [0x00, 0xce, 0xd1]),
    ("darkviolet", [0x94, 0x00, 0xd3]),
    ("deeppink", [0xff, 0x14, 0x93]),
    ("deepskyblue", [0x00, 0xbf, 0xff]),
    ("dimgray", [0x69, 0x69, 0x69]),
    ("dimgrey", [0x69, 0x69, 0x69]),
    ("dodgerblue", [0x1e, 0x90, 0xff]),
    ("firebrick", [0xb2, 0x22, 0x22]),
    ("floralwhite", [0xff, 0xfa, 0xf0]),
    ("forestgreen", [0x22, 0x8b, 0x22]),
    ("fuchsia", [0xff, 0x00, 0xff]),
    ("gainsboro", [0xdc, 0xdc, 0xdc]),
    ("ghostwhite", [0xf8, 0xf8, 0xff]),
    ("gold", [0xff, 0xd7, 0x00]),
    ("goldenrod", [0xda, 0xa5, 0x20]),
    ("gray", [0x80, 0x80, 0x80]),
    ("green", [0x00, 0x80, 0x00]),
    ("greenyellow", [0xad, 0xff, 0x2f]),
    ("grey", [0x80, 0x80, 0x80]),
    ("honeydew", [0xf0, 0xff, 0xf0]),
    ("hotpink", [0xff, 0x69, 0xb4]),
    ("indianred", [0xcd, 0x5c, 0x5c]),
    ("indigo", [0x4b, 0x00, 0x82]),
    ("ivory", [0xff, 0xff, 0xf0]),
    ("khaki", [0xf0, 0xe6, 0x8c]),
    ("lavender", [0xe6, 0xe6, 0xfa]),
    ("lavenderblush", [0xff, 0xf0, 0xf5]),
    ("lawngreen", [0x7c, 0xfc, 0x00]),
    ("lemonchiffon", [0xff, 0xfa, 0xcd]),
    ("lightblue", [0xad, 0xd8, 0xe6]),
    ("lightcoral", [0xf0, 0x80, 0x80]),
    ("lightcyan", [0xe0, 0xff, 0xff]),
    ("lightgoldenrodyellow", [0xfa, 0xfa, 0xd2]),
    ("lightgray", [0xd3, 0xd3, 0xd3]),
    ("lightgreen", [0x90, 0xee, 0x90]),
    ("lightgrey", [0xd3, 0xd3, 0xd3]),
    ("lightpink", [0xff, 0xb6, 0xc1]),
    ("lightsalmon", [0xff, 0xa0, 0x7a]),
    ("lightseagreen", [0x20, 0xb2, 0xaa]),
    ("lightskyblue", [0x87, 0xce, 0xfa]),
    ("lightslategray", [0x77, 0x88, 0x99]),
    ("lightslategrey", [0x77, 0x88, 0x99]),
    ("lightsteelblue", [0xb0, 0xc4, 0xde]),
    ("lightyellow", [0xff, 0xff, 0xe0]),
    ("lime", [0x00, 0xff, 0x00]),
    ("limegreen", [0x32, 0xcd, 0x32]),
    ("linen", [0xfa, 0xf0, 0xe6]),
    ("magenta", [0xff, 0x00, 0xff]),
    ("maroon", [0x80, 0x00, 0x00]),
    ("mediumaquamarine", [0x66, 0xcd, 0xaa]),
    ("mediumblue", [0x00, 0x00, 0xcd]),
    ("mediumorchid", [0xba, 0x55, 0xd3]),
    ("mediumpurple", [0x93, 0x70, 0xdb]),
    ("mediumseagreen", [0x3c, 0xb3, 0x71]),
    ("mediumslateblue", [0x7b, 0x68, 0xee]),
    ("mediumspringgreen", [0x00, 0xfa, 0x9a]),
    ("mediumturquoise", [0x48, 0xd1, 0xcc]),
    ("mediumvioletred", [0xc7, 0x15, 0x85]),
    ("midnightblue", [0x19, 0x19, 0x70]),
    ("mintcream", [0xf5, 0xff, 0xfa]),
    ("mistyrose", [0xff, 0xe4, 0xe1]),
    ("moccasin", [0xff, 0xe4, 0xb5]),
    ("navajowhite", [0xff, 0xde, 0xad]),
    ("navy", [0x00, 0x00, 0x80]),
    ("oldlace", [0xfd, 0xf5, 0xe6]),
    ("olive", [0x80, 0x80, 0x00]),
    ("olivedrab", [0x6b, 0x8e, 0x23]),
    ("orange", [0xff, 0xa5, 0x00]),
    ("orangered", [0xff, 0x45, 0x00]),
    ("orchid", [0xda, 0x70, 0xd6]),
    ("palegoldenrod", [0xee, 0xe8, 0xaa]),
    ("palegreen", [0x98, 0xfb, 0x98]),
    ("paleturquoise", [0xaf, 0xee, 0xee]),
    ("palevioletred", [0xdb, 0x70, 0x93]),
    ("papayawhip", [0xff, 0xef, 0xd5]),
    ("peachpuff", [0xff, 0xda, 0xb9]),
    ("peru", [0xcd, 0x85, 0x3f]),
    ("pink", [0xff, 0xc0, 0xcb]),
    ("plum", [0xdd, 0xa0, 0xdd]),
    ("powderblue", [0xb0, 0xe0, 0xe6]),
    ("purple", [0x80, 0x00, 0x80]),
    ("rebeccapurple", [0x66, 0x33, 0x99]),
    ("red", [0xff, 0x00, 0x00]),
    ("rosybrown", [0xbc, 0x8f, 0x8f]),
    ("royalblue", [0x41, 0x69, 0xe1]),
    ("saddlebrown", [0x8b, 0x45, 0x13]),
    ("salmon", [0xfa, 0x80, 0x72]),
    ("sandybrown", [0xf4, 0xa4, 0x60]),
    ("seagreen", [0x2e, 0x8b, 0x57]),
    ("seashell", [0xff, 0xf5, 0xee]),
    ("sienna", [0xa0, 0x52, 0x2d]),
    ("silver", [0xc0, 0xc0, 0xc0]),
    ("skyblue", [0x87, 0xce, 0xeb]),
    ("slateblue", [0x6a, 0x5a, 0xcd]),
    ("slategray", [0x70, 0x80, 0x90]),
    ("slategrey", [0x70, 0x80, 0x90]),
    ("snow", [0xff, 0xfa, 0xfa]),
    ("springgreen", [0x00, 0xff, 0x7f]),
    ("steelblue", [0x46, 0x82, 0xb4]),
    ("tan", [0xd2, 0xb4, 0x8c]),
    ("teal", [0x00, 0x80, 0x80]),
    ("thistle", [0xd8, 0xbf, 0xd8]),
    ("tomato", [0xff, 0x63, 0x47]),
    ("turquoise", [0x40, 0xe0, 0xd0]),
    ("violet", [0xee, 0x82, 0xee]),
    ("wheat", [0xf5, 0xde, 0xb3]),
    ("white", [0xff, 0xff, 0xff]),
    ("whitesmoke", [0xf5, 0xf5, 0xf5]),
    ("yellow", [0xff, 0xff, 0x00]),
    ("yellowgreen", [0x9a, 0xcd, 0x32]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn named_table_is_sorted_unique_and_lower_case() {
        for pair in NAMED_COLORS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} !< {}", pair[0].0, pair[1].0);
        }
        for (name, [r, g, b]) in NAMED_COLORS {
            assert!(name.bytes().all(|b| b.is_ascii_lowercase()), "{name}");
            assert_eq!(Color::from_name(name), Some(Color::rgb(r, g, b)), "{name}");
        }
    }

    #[test]
    fn parses_every_hex_length() {
        assert_eq!("#fff".parse(), Ok(Color::WHITE));
        assert_eq!("#F00".parse(), Ok(Color::rgb(255, 0, 0)));
        assert_eq!("#f008".parse(), Ok(Color::rgba(255, 0, 0, 0x88)));
        assert_eq!("#1a73e8".parse(), Ok(Color::rgb(0x1a, 0x73, 0xe8)));
        assert_eq!("#1A73E880".parse(), Ok(Color::rgba(0x1a, 0x73, 0xe8, 0x80)));
        assert_eq!("#00000000".parse(), Ok(Color::TRANSPARENT));
    }

    #[test]
    fn parses_names_case_insensitively() {
        assert_eq!("black".parse(), Ok(Color::BLACK));
        assert_eq!("WHITE".parse(), Ok(Color::WHITE));
        assert_eq!("DarkSlateGrey".parse(), Ok(Color::rgb(0x2f, 0x4f, 0x4f)));
        assert_eq!("Transparent".parse(), Ok(Color::TRANSPARENT));
        assert_eq!("aliceblue".parse(), Ok(Color::rgb(0xf0, 0xf8, 0xff)));
        assert_eq!("yellowgreen".parse(), Ok(Color::rgb(0x9a, 0xcd, 0x32)));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!("".parse::<Color>(), Err(ColorError::Empty));
        for bad in [
            "#",
            "#ff",
            "#fffff",
            "#fffffff",
            "#fffffffff",
            "#ggg",
            "#+12",
            "# fff",
        ] {
            assert_eq!(
                bad.parse::<Color>(),
                Err(ColorError::Hex(bad.into())),
                "{bad}"
            );
        }
        for bad in [
            "blak",
            " black",
            "black ",
            "rgb(0,0,0)",
            "\"><script>",
            "ｂlack",
        ] {
            assert_eq!(
                bad.parse::<Color>(),
                Err(ColorError::Name(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn errors_quote_a_bounded_prefix() {
        let long = "x".repeat(1000);
        let err = long.parse::<Color>().unwrap_err();
        let ColorError::Name(echoed) = &err else {
            panic!("{err:?}")
        };
        assert_eq!(echoed.chars().count(), MAX_ECHO_CHARS + 1);
        assert!(echoed.ends_with('…'));
        assert_eq!(
            ColorError::Hex("#zz".into()).to_string(),
            "invalid hex colour '#zz' (expected #RGB, #RGBA, #RRGGBB or #RRGGBBAA)"
        );
        assert!(ColorError::Empty.to_string().starts_with("empty colour"));
        assert!(err.to_string().starts_with("unknown colour name 'xxx"));
    }

    #[test]
    fn display_is_canonical_hex() {
        assert_eq!(Color::BLACK.to_string(), "#000000");
        assert_eq!(Color::rgba(1, 2, 3, 4).to_string(), "#01020304");
        assert_eq!(Color::default(), Color::BLACK);
        assert_eq!(Color::WHITE.with_alpha(0), Color::rgba(255, 255, 255, 0));
        assert!(Color::BLACK.is_opaque() && !Color::BLACK.is_invisible());
        assert!(Color::TRANSPARENT.is_invisible() && !Color::TRANSPARENT.is_opaque());
    }

    #[test]
    fn over_blends_against_an_opaque_backdrop() {
        let red = Color::rgb(255, 0, 0);
        assert_eq!(Color::TRANSPARENT.over(red), red);
        assert_eq!(red.over(Color::WHITE), red);
        assert_eq!(
            Color::rgba(0, 0, 255, 51).over(Color::WHITE),
            Color::rgb(204, 204, 255)
        );
    }

    proptest! {
        #[test]
        fn display_round_trips(r: u8, g: u8, b: u8, a: u8) {
            let c = Color::rgba(r, g, b, a);
            prop_assert_eq!(c.to_string().parse::<Color>(), Ok(c));
        }

        #[test]
        fn parse_never_panics(s in ".{0,40}") {
            let _ = s.parse::<Color>();
        }

        #[test]
        fn over_is_always_opaque(fg: u8, a: u8, bg: u8) {
            let blended = Color::rgba(fg, fg, fg, a).over(Color::rgb(bg, bg, bg));
            prop_assert!(blended.is_opaque());
            prop_assert!(blended.r >= fg.min(bg) && blended.r <= fg.max(bg));
        }
    }
}
