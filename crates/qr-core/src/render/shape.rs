//! Module shapes: how each dark module is drawn.
//!
//! The geometry is shared by every graphical renderer, so a raster image
//! and a vector document of the same code show the same shapes:
//!
//! - [`ModuleShape::Square`] fills the whole module.
//! - [`ModuleShape::Rounded`] rounds a corner of a dark module, with a
//!   radius of half a module, when both modules beside that corner (the
//!   horizontal and the vertical neighbour) are light. Connected modules
//!   stay joined, so lines read as pills and isolated modules as circles.
//! - [`ModuleShape::Dots`] draws every module as a circle of diameter
//!   [`DOT_DIAMETER`] modules.
//!
//! The three finder patterns (the 7x7 "eyes") are always drawn square:
//! scanners locate a symbol by their 1:1:3:1:1 ratio, and custom eyes are
//! deferred (DECISIONS section 3).

use std::fmt;
use std::str::FromStr;

use crate::matrix::Matrix;

/// The diameter of a dot, as a fraction of the module side.
pub const DOT_DIAMETER: f64 = 0.9;

/// The side of a finder pattern, in modules.
const FINDER: usize = 7;

/// The smallest symbol (version 1) is 21 modules across; narrower matrices
/// (as in unit tests) have no finder patterns.
const MIN_SYMBOL_WIDTH: usize = 21;

/// How dark modules are drawn by the graphical renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModuleShape {
    /// Full squares: the standard look, and the most robust.
    #[default]
    Square,
    /// Squares whose exposed corners are rounded.
    Rounded,
    /// Circles.
    Dots,
}

impl ModuleShape {
    /// Every shape.
    pub const ALL: [Self; 3] = [Self::Square, Self::Rounded, Self::Dots];

    /// The name accepted by [`FromStr`] and the CLI's `--shape`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Rounded => "rounded",
            Self::Dots => "dots",
        }
    }
}

impl fmt::Display for ModuleShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The name was not a module shape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown module shape '{0}' (expected square, rounded or dots)")]
pub struct UnknownModuleShape(pub String);

impl FromStr for ModuleShape {
    type Err = UnknownModuleShape;

    /// Parses `square`, `rounded` or `dots`, ignoring ASCII case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|shape| shape.name().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnknownModuleShape(s.chars().take(32).collect()))
    }
}

/// A set of module corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Corners(u8);

impl Corners {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const TOP_LEFT: Self = Self(1);
    pub(crate) const TOP_RIGHT: Self = Self(2);
    pub(crate) const BOTTOM_RIGHT: Self = Self(4);
    pub(crate) const BOTTOM_LEFT: Self = Self(8);

    /// Whether every corner in `other` is in this set.
    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// This set plus `other` when `include` is true.
    #[must_use]
    pub(crate) const fn with(self, other: Self, include: bool) -> Self {
        if include {
            Self(self.0 | other.0)
        } else {
            self
        }
    }
}

/// How one dark module is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Glyph {
    /// The full module.
    Square,
    /// The module with the given corners rounded (radius half a module).
    Rounded(Corners),
    /// A centred circle of diameter [`DOT_DIAMETER`].
    Dot,
}

impl Glyph {
    /// The glyph for the dark module at `(x, y)` of `matrix`.
    pub(crate) fn for_module(matrix: &Matrix, shape: ModuleShape, x: usize, y: usize) -> Self {
        if shape == ModuleShape::Square || in_finder(matrix.width(), x, y) {
            return Self::Square;
        }
        match shape {
            ModuleShape::Square => Self::Square,
            ModuleShape::Dots => Self::Dot,
            ModuleShape::Rounded => match exposed_corners(matrix, x, y) {
                Corners::NONE => Self::Square,
                corners => Self::Rounded(corners),
            },
        }
    }

    /// Whether the pixel `(sx, sy)` of a module `scale` pixels across is
    /// dark, sampled at the pixel's centre.
    ///
    /// Integer arithmetic in half-pixel units keeps it exact: the module is
    /// `2 * scale` units across, its centre is at `scale` and pixel `i` has
    /// its centre at `2i + 1`.
    pub(crate) fn covers(self, scale: u32, sx: u32, sy: u32) -> bool {
        let s = i64::from(scale);
        let du = 2 * i64::from(sx) + 1 - s;
        let dv = 2 * i64::from(sy) + 1 - s;
        let distance = du * du + dv * dv;
        match self {
            Self::Square => true,
            // Radius 0.45 modules = 0.9 * scale units: 100 d^2 <= 81 s^2.
            Self::Dot => 100 * distance <= 81 * s * s,
            Self::Rounded(corners) => {
                let corner = match (du < 0, dv < 0) {
                    (true, true) => Corners::TOP_LEFT,
                    (false, true) => Corners::TOP_RIGHT,
                    (false, false) => Corners::BOTTOM_RIGHT,
                    (true, false) => Corners::BOTTOM_LEFT,
                };
                // A half-module radius puts every corner's arc centre at
                // the module centre.
                !corners.contains(corner) || distance <= s * s
            }
        }
    }

    /// The dark pixels of row `sy` of a module `scale` pixels across, as a
    /// half-open range of columns. Every glyph is convex, so each row is one
    /// interval, possibly empty (`start == end`).
    pub(crate) fn span(self, scale: u32, sy: u32) -> (u32, u32) {
        if self == Self::Square {
            return (0, scale);
        }
        let Some(start) = (0..scale).find(|&sx| self.covers(scale, sx, sy)) else {
            return (0, 0);
        };
        // Convex and not empty, so the last dark pixel exists.
        let end = (start..scale)
            .rev()
            .find(|&sx| self.covers(scale, sx, sy))
            .map_or(scale, |sx| sx + 1);
        (start, end)
    }
}

/// Whether `(x, y)` lies in one of the three finder patterns of a symbol
/// `width` modules across.
pub(crate) fn in_finder(width: usize, x: usize, y: usize) -> bool {
    if width < MIN_SYMBOL_WIDTH {
        return false;
    }
    let near = |v: usize| v < FINDER;
    let far = |v: usize| v >= width - FINDER;
    (near(x) && near(y)) || (far(x) && near(y)) || (near(x) && far(y))
}

/// The corners of the module at `(x, y)` whose two orthogonal neighbours
/// are both light. Modules outside the matrix are light.
pub(crate) fn exposed_corners(matrix: &Matrix, x: usize, y: usize) -> Corners {
    let dark = |x: Option<usize>, y: Option<usize>| match (x, y) {
        (Some(x), Some(y)) => matrix.is_dark(x, y),
        _ => false,
    };
    let left = dark(x.checked_sub(1), Some(y));
    let right = dark(x.checked_add(1), Some(y));
    let up = dark(Some(x), y.checked_sub(1));
    let down = dark(Some(x), y.checked_add(1));
    Corners::NONE
        .with(Corners::TOP_LEFT, !left && !up)
        .with(Corners::TOP_RIGHT, !right && !up)
        .with(Corners::BOTTOM_RIGHT, !right && !down)
        .with(Corners::BOTTOM_LEFT, !left && !down)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(glyph: Glyph, scale: u32) -> Vec<String> {
        (0..scale)
            .map(|sy| {
                (0..scale)
                    .map(|sx| {
                        if glyph.covers(scale, sx, sy) {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn names_round_trip() {
        for shape in ModuleShape::ALL {
            assert_eq!(shape.to_string().parse(), Ok(shape));
            assert_eq!(shape.name().to_uppercase().parse(), Ok(shape));
        }
        assert_eq!(ModuleShape::default(), ModuleShape::Square);
        let err = "star".parse::<ModuleShape>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown module shape 'star' (expected square, rounded or dots)"
        );
        let long = "x".repeat(100).parse::<ModuleShape>().unwrap_err();
        assert_eq!(long.0.len(), 32);
    }

    #[test]
    fn dots_are_circles() {
        assert_eq!(
            bitmap(Glyph::Dot, 10),
            [
                "..........",
                "..######..",
                ".########.",
                ".########.",
                ".########.",
                ".########.",
                ".########.",
                ".########.",
                "..######..",
                "..........",
            ]
        );
        // One pixel per module samples the centre, which is always dark.
        assert_eq!(bitmap(Glyph::Dot, 1), ["#"]);
    }

    #[test]
    fn rounded_corners_follow_the_flags() {
        let all = Corners::TOP_LEFT
            .with(Corners::TOP_RIGHT, true)
            .with(Corners::BOTTOM_RIGHT, true)
            .with(Corners::BOTTOM_LEFT, true);
        assert_eq!(
            bitmap(Glyph::Rounded(all), 8),
            [
                "..####..", ".######.", "########", "########", "########", "########", ".######.",
                "..####..",
            ]
        );
        assert_eq!(
            bitmap(Glyph::Rounded(Corners::TOP_RIGHT), 6),
            ["#####.", "######", "######", "######", "######", "######",]
        );
        assert_eq!(
            bitmap(Glyph::Rounded(Corners::BOTTOM_LEFT), 4),
            ["####", "####", "####", ".###"]
        );
        assert_eq!(bitmap(Glyph::Square, 3), ["###", "###", "###"]);
    }

    #[test]
    fn spans_match_the_bitmap() {
        for glyph in [
            Glyph::Square,
            Glyph::Dot,
            Glyph::Rounded(Corners::TOP_LEFT),
            Glyph::Rounded(Corners::TOP_RIGHT.with(Corners::BOTTOM_LEFT, true)),
        ] {
            for scale in 1..12 {
                for sy in 0..scale {
                    let (a, b) = glyph.span(scale, sy);
                    for sx in 0..scale {
                        assert_eq!(
                            glyph.covers(scale, sx, sy),
                            (a..b).contains(&sx),
                            "{glyph:?} scale {scale} ({sx}, {sy})"
                        );
                    }
                }
            }
        }
        // A tiny dot can leave a row empty.
        assert_eq!(Glyph::Dot.span(10, 0), (0, 0));
    }

    #[test]
    fn corners_are_exposed_only_between_two_light_neighbours() {
        // .#.
        // ###
        // .#.
        let plus = Matrix::from_modules(
            3,
            vec![false, true, false, true, true, true, false, true, false],
        )
        .unwrap();
        assert_eq!(exposed_corners(&plus, 1, 1), Corners::NONE);
        let top = exposed_corners(&plus, 1, 0);
        assert!(top.contains(Corners::TOP_LEFT) && top.contains(Corners::TOP_RIGHT));
        assert!(!top.contains(Corners::BOTTOM_LEFT) && !top.contains(Corners::BOTTOM_RIGHT));
        let left = exposed_corners(&plus, 0, 1);
        assert!(left.contains(Corners::TOP_LEFT) && left.contains(Corners::BOTTOM_LEFT));
        assert!(!left.contains(Corners::TOP_RIGHT));
        assert!(Corners::NONE.is_empty() && !top.is_empty());

        let lone = Matrix::from_modules(1, vec![true]).unwrap();
        assert_eq!(
            Glyph::for_module(&lone, ModuleShape::Rounded, 0, 0),
            Glyph::Rounded(Corners(15))
        );
        assert_eq!(
            Glyph::for_module(&plus, ModuleShape::Rounded, 1, 1),
            Glyph::Square
        );
        assert_eq!(
            Glyph::for_module(&plus, ModuleShape::Dots, 1, 1),
            Glyph::Dot
        );
        assert_eq!(
            Glyph::for_module(&plus, ModuleShape::Square, 1, 0),
            Glyph::Square
        );
    }

    #[test]
    fn finder_patterns_stay_square() {
        let width = 21;
        let m = Matrix::from_modules(width, vec![true; width * width]).unwrap();
        for (x, y, finder) in [
            (0, 0, true),
            (6, 6, true),
            (7, 0, false),
            (14, 0, true),
            (13, 6, false),
            (20, 6, true),
            (0, 14, true),
            (6, 20, true),
            (20, 20, false),
            (10, 10, false),
        ] {
            assert_eq!(in_finder(width, x, y), finder, "({x}, {y})");
            let glyph = Glyph::for_module(&m, ModuleShape::Dots, x, y);
            assert_eq!(glyph == Glyph::Square, finder, "({x}, {y})");
        }
        assert!(!in_finder(20, 0, 0), "too small to be a symbol");
    }
}
