//! What the vector renderers draw for each module shape: rectangles for
//! square runs, and closed outlines of lines and cubic Bézier curves for
//! rounded runs and dots.
//!
//! The geometry matches the raster renderer's [`Glyph`](super::shape::Glyph)
//! rules. A horizontal run of dark modules is one rectangle whose only
//! exposed corners are at its two ends, so a rounded run is a rectangle with
//! up to four rounded corners, and drawing whole runs avoids seams between
//! neighbouring modules. Coordinates are in modules, quiet zone included,
//! with the origin at the top-left and `y` growing downwards.

use std::fmt;

use super::runs::{Run, dark_runs};
use super::shape::{Corners, DOT_DIAMETER, ModuleShape, exposed_corners, in_finder};
use crate::matrix::Matrix;

/// The distance of a cubic Bézier control point from the end of a
/// quarter-circle arc of radius 1: `4/3 * (sqrt 2 - 1)`.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// The radius of a rounded corner, in modules.
const CORNER_RADIUS: f64 = 0.5;

/// A point in module units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Point {
    const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// One step of a closed outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Segment {
    /// Starts a new subpath.
    Move(Point),
    /// A straight line to the point.
    Line(Point),
    /// A cubic Bézier curve through two control points to the last point.
    Cubic(Point, Point, Point),
    /// Closes the subpath.
    Close,
}

/// One thing to fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Primitive {
    /// A run of square modules, offset by the margin.
    Rect(Run),
    /// A closed outline, made of the segments [`Primitive::outline`] yields.
    Shape(ShapeKind),
}

/// A shaped primitive, in module units with the margin applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ShapeKind {
    /// A run with some corners rounded.
    RoundedRun {
        /// The run, offset by the margin.
        run: Run,
        /// The rounded corners.
        corners: Corners,
    },
    /// A dot in the module whose top-left corner is `(x, y)`.
    Dot {
        /// Column, offset by the margin.
        x: u32,
        /// Row, offset by the margin.
        y: u32,
    },
}

impl ShapeKind {
    /// Calls `segment` for each step of the outline, which starts with a
    /// [`Segment::Move`] and ends with a [`Segment::Close`].
    pub(crate) fn outline(self, mut segment: impl FnMut(Segment)) {
        match self {
            Self::RoundedRun { run, corners } => rounded_rect(run, corners, &mut segment),
            Self::Dot { x, y } => {
                let r = DOT_DIAMETER / 2.0;
                let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let k = r * KAPPA;
                segment(Segment::Move(Point::new(cx + r, cy)));
                segment(Segment::Cubic(
                    Point::new(cx + r, cy + k),
                    Point::new(cx + k, cy + r),
                    Point::new(cx, cy + r),
                ));
                segment(Segment::Cubic(
                    Point::new(cx - k, cy + r),
                    Point::new(cx - r, cy + k),
                    Point::new(cx - r, cy),
                ));
                segment(Segment::Cubic(
                    Point::new(cx - r, cy - k),
                    Point::new(cx - k, cy - r),
                    Point::new(cx, cy - r),
                ));
                segment(Segment::Cubic(
                    Point::new(cx + k, cy - r),
                    Point::new(cx + r, cy - k),
                    Point::new(cx + r, cy),
                ));
                segment(Segment::Close);
            }
        }
    }
}

/// The outline of a one-module-high run with the given corners rounded,
/// clockwise on screen from the top edge.
fn rounded_rect(run: Run, corners: Corners, segment: &mut impl FnMut(Segment)) {
    let (x0, y0) = (f64::from(run.x), f64::from(run.y));
    let (x1, y1) = (x0 + f64::from(run.len), y0 + 1.0);
    let r = CORNER_RADIUS;
    let k = r * KAPPA;
    let radius = |corner| if corners.contains(corner) { r } else { 0.0 };
    let (tl, tr, br, bl) = (
        radius(Corners::TOP_LEFT),
        radius(Corners::TOP_RIGHT),
        radius(Corners::BOTTOM_RIGHT),
        radius(Corners::BOTTOM_LEFT),
    );
    segment(Segment::Move(Point::new(x0 + tl, y0)));
    segment(Segment::Line(Point::new(x1 - tr, y0)));
    if tr > 0.0 {
        segment(Segment::Cubic(
            Point::new(x1 - r + k, y0),
            Point::new(x1, y0 + r - k),
            Point::new(x1, y0 + r),
        ));
    }
    segment(Segment::Line(Point::new(x1, y1 - br)));
    if br > 0.0 {
        segment(Segment::Cubic(
            Point::new(x1, y1 - r + k),
            Point::new(x1 - r + k, y1),
            Point::new(x1 - r, y1),
        ));
    }
    segment(Segment::Line(Point::new(x0 + bl, y1)));
    if bl > 0.0 {
        segment(Segment::Cubic(
            Point::new(x0 + r - k, y1),
            Point::new(x0, y1 - r + k),
            Point::new(x0, y1 - r),
        ));
    }
    segment(Segment::Line(Point::new(x0, y0 + tl)));
    if tl > 0.0 {
        segment(Segment::Cubic(
            Point::new(x0, y0 + r - k),
            Point::new(x0 + r - k, y0),
            Point::new(x0 + r, y0),
        ));
    }
    segment(Segment::Close);
}

/// Everything to fill for `matrix` drawn with `shape`, offset by `margin`
/// modules: square runs (the finder patterns, and every run of a square
/// symbol) as rectangles, and the rest as outlines.
pub(crate) fn primitives(
    matrix: &Matrix,
    shape: ModuleShape,
    margin: u32,
) -> impl Iterator<Item = Primitive> + '_ {
    let width = matrix.width();
    dark_runs(matrix).flat_map(move |run| {
        let shifted = Run {
            x: run.x + margin,
            y: run.y + margin,
            ..run
        };
        let (x, y) = (run.x as usize, run.y as usize);
        // Finder runs never touch other modules: a light separator
        // surrounds each finder pattern.
        let square = shape == ModuleShape::Square || in_finder(width, x, y);
        let (rect, rounded, dots) = match shape {
            _ if square => (Some(Primitive::Rect(shifted)), None, None),
            ModuleShape::Rounded => {
                let last = x + run.len as usize - 1;
                let first = exposed_corners(matrix, x, y);
                let end = exposed_corners(matrix, last, y);
                let corners = Corners::NONE
                    .with(Corners::TOP_LEFT, first.contains(Corners::TOP_LEFT))
                    .with(Corners::BOTTOM_LEFT, first.contains(Corners::BOTTOM_LEFT))
                    .with(Corners::TOP_RIGHT, end.contains(Corners::TOP_RIGHT))
                    .with(Corners::BOTTOM_RIGHT, end.contains(Corners::BOTTOM_RIGHT));
                if corners.is_empty() {
                    (Some(Primitive::Rect(shifted)), None, None)
                } else {
                    let kind = ShapeKind::RoundedRun {
                        run: shifted,
                        corners,
                    };
                    (None, Some(Primitive::Shape(kind)), None)
                }
            }
            ModuleShape::Square | ModuleShape::Dots => {
                let dots = (0..run.len).map(move |i| {
                    Primitive::Shape(ShapeKind::Dot {
                        x: shifted.x + i,
                        y: shifted.y,
                    })
                });
                (None, None, Some(dots))
            }
        };
        rect.into_iter()
            .chain(rounded)
            .chain(dots.into_iter().flatten())
    })
}

/// A coordinate written with at most four decimals and no trailing zeros,
/// as `2`, `0.5` or `3.2761`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Num(pub(crate) f64);

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Fixed point with four decimals. Coordinates are bounded by the
        // canvas (at most 16,384 modules), far inside i64.
        #[allow(clippy::cast_possible_truncation)]
        let units = (self.0 * 10_000.0).round() as i64;
        if units < 0 {
            f.write_str("-")?;
        }
        let units = units.unsigned_abs();
        let (whole, mut frac) = (units / 10_000, units % 10_000);
        write!(f, "{whole}")?;
        if frac == 0 {
            return Ok(());
        }
        let mut digits = 4;
        while frac % 10 == 0 {
            frac /= 10;
            digits -= 1;
        }
        write!(f, ".{frac:0digits$}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(kind: ShapeKind) -> Vec<Segment> {
        let mut out = Vec::new();
        kind.outline(|s| out.push(s));
        out
    }

    fn end(segment: Segment) -> Option<Point> {
        match segment {
            Segment::Move(p) | Segment::Line(p) | Segment::Cubic(_, _, p) => Some(p),
            Segment::Close => None,
        }
    }

    /// The outline is closed: it starts with a move, ends with a close and
    /// its last point is its first.
    fn assert_closed(segments: &[Segment]) {
        assert!(matches!(segments.first(), Some(Segment::Move(_))));
        assert_eq!(segments.last(), Some(&Segment::Close));
        let first = end(segments[0]).unwrap();
        let last = end(segments[segments.len() - 2]).unwrap();
        assert!(
            (first.x - last.x).abs() < 1e-9 && (first.y - last.y).abs() < 1e-9,
            "{segments:?}"
        );
    }

    #[test]
    fn numbers_are_short() {
        let cases = [
            (2.0, "2"),
            (0.5, "0.5"),
            (3.276_142, "3.2761"),
            (-0.000_01, "0"),
            (10.25, "10.25"),
            (-1.5, "-1.5"),
        ];
        for (value, text) in cases {
            assert_eq!(Num(value).to_string(), text);
        }
    }

    #[test]
    fn dots_are_closed_circles_of_the_right_size() {
        let segments = collect(ShapeKind::Dot { x: 3, y: 1 });
        assert_closed(&segments);
        assert_eq!(segments.len(), 6);
        assert_eq!(segments[0], Segment::Move(Point::new(3.95, 1.5)));
        let ends: Vec<Point> = segments.iter().filter_map(|s| end(*s)).collect();
        for p in ends {
            let d = ((p.x - 3.5).powi(2) + (p.y - 1.5).powi(2)).sqrt();
            assert!((d - 0.45).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn rounded_runs_round_only_the_given_corners() {
        let run = Run { x: 1, y: 2, len: 3 };
        let square = collect(ShapeKind::RoundedRun {
            run,
            corners: Corners::NONE,
        });
        assert_closed(&square);
        assert!(square.iter().all(|s| !matches!(s, Segment::Cubic(..))));
        assert_eq!(square.len(), 6);

        let all = Corners::TOP_LEFT
            .with(Corners::TOP_RIGHT, true)
            .with(Corners::BOTTOM_RIGHT, true)
            .with(Corners::BOTTOM_LEFT, true);
        let pill = collect(ShapeKind::RoundedRun { run, corners: all });
        assert_closed(&pill);
        assert_eq!(
            pill.iter()
                .filter(|s| matches!(s, Segment::Cubic(..)))
                .count(),
            4
        );
        assert_eq!(pill[0], Segment::Move(Point::new(1.5, 2.0)));
        assert_eq!(pill[1], Segment::Line(Point::new(3.5, 2.0)));

        let one = collect(ShapeKind::RoundedRun {
            run,
            corners: Corners::BOTTOM_LEFT,
        });
        assert_closed(&one);
        assert!(one.contains(&Segment::Line(Point::new(1.5, 3.0))));
        // Every point stays inside the run's box.
        for p in one.iter().filter_map(|s| end(*s)) {
            assert!((1.0..=4.0).contains(&p.x) && (2.0..=3.0).contains(&p.y));
        }
    }

    #[test]
    fn primitives_cover_every_dark_module_once() {
        let code = crate::encode(
            b"https://example.com/shapes",
            &crate::EncodeOptions::default(),
        )
        .unwrap();
        let m = code.matrix();
        for shape in ModuleShape::ALL {
            let mut covered = vec![0_u32; m.width() * m.width()];
            for primitive in primitives(m, shape, 2) {
                let (x, y, len) = match primitive {
                    Primitive::Rect(run) | Primitive::Shape(ShapeKind::RoundedRun { run, .. }) => {
                        (run.x, run.y, run.len)
                    }
                    Primitive::Shape(ShapeKind::Dot { x, y }) => (x, y, 1),
                };
                for x in x..x + len {
                    covered[(y as usize - 2) * m.width() + x as usize - 2] += 1;
                }
            }
            let expected: Vec<u32> = m.modules().iter().map(|&d| u32::from(d)).collect();
            assert_eq!(covered, expected, "{shape}");
        }
    }

    #[test]
    fn square_and_finder_runs_are_rectangles() {
        let code = crate::encode(b"finder", &crate::EncodeOptions::default()).unwrap();
        let m = code.matrix();
        assert!(primitives(m, ModuleShape::Square, 0).all(|p| matches!(p, Primitive::Rect(_))));
        let first = primitives(m, ModuleShape::Dots, 4).next().unwrap();
        // The top-left finder's top edge: seven modules at (4, 4).
        assert_eq!(first, Primitive::Rect(Run { x: 4, y: 4, len: 7 }));
        assert!(
            primitives(m, ModuleShape::Dots, 0)
                .any(|p| matches!(p, Primitive::Shape(ShapeKind::Dot { .. })))
        );
        assert!(
            primitives(m, ModuleShape::Rounded, 0)
                .any(|p| matches!(p, Primitive::Shape(ShapeKind::RoundedRun { .. })))
        );
    }

    #[test]
    fn fully_joined_rounded_runs_stay_rectangles() {
        // ###
        // ###  The middle row has dark modules above and below.
        // ###
        let m = Matrix::from_modules(3, vec![true; 9]).unwrap();
        let prims: Vec<Primitive> = primitives(&m, ModuleShape::Rounded, 0).collect();
        assert_eq!(prims[1], Primitive::Rect(Run { x: 0, y: 1, len: 3 }));
        assert!(matches!(prims[0], Primitive::Shape(_)));
    }
}
