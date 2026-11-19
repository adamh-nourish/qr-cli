//! Horizontal runs of dark modules: the shape every vector renderer draws.
//!
//! Merging each row's adjacent dark modules into one rectangle keeps vector
//! output small (a version 40 symbol has about 31,000 modules but only a few
//! thousand runs) and avoids hairline seams between neighbouring modules
//! when a viewer anti-aliases rectangle edges.

use crate::matrix::Matrix;

/// A horizontal run of dark modules, in matrix coordinates (no quiet zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Run {
    /// Column of the first module.
    pub(crate) x: u32,
    /// Row.
    pub(crate) y: u32,
    /// Number of modules, at least 1.
    pub(crate) len: u32,
}

/// The dark runs of `matrix`, row by row from the top, left to right.
///
/// A matrix wider than `u32::MAX` cannot be drawn on a bounded canvas, so
/// callers check the layout first; any coordinate that does not fit is
/// skipped rather than wrapped.
pub(crate) fn dark_runs(matrix: &Matrix) -> impl Iterator<Item = Run> + '_ {
    matrix.rows().enumerate().flat_map(|(y, row)| {
        let y = u32::try_from(y).ok();
        let mut x = 0;
        std::iter::from_fn(move || {
            let y = y?;
            while row.get(x).is_some_and(|dark| !dark) {
                x += 1;
            }
            let start = x;
            while row.get(x).copied().unwrap_or(false) {
                x += 1;
            }
            if start == x {
                return None;
            }
            Some(Run {
                x: u32::try_from(start).ok()?,
                y,
                len: u32::try_from(x - start).ok()?,
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EncodeOptions, encode};

    #[test]
    fn merges_adjacent_modules_per_row() {
        // Row 0: ##.#  Row 1: ....  Row 2: ####  Row 3: .#..
        let bits = "##.#....####.#..";
        let m = Matrix::from_modules(4, bits.chars().map(|c| c == '#').collect()).unwrap();
        let runs: Vec<Run> = dark_runs(&m).collect();
        assert_eq!(
            runs,
            vec![
                Run { x: 0, y: 0, len: 2 },
                Run { x: 3, y: 0, len: 1 },
                Run { x: 0, y: 2, len: 4 },
                Run { x: 1, y: 3, len: 1 },
            ]
        );
    }

    #[test]
    fn runs_cover_exactly_the_dark_modules() {
        let code = encode(b"https://example.com/runs", &EncodeOptions::default()).unwrap();
        let m = code.matrix();
        let mut grid = vec![false; m.width() * m.width()];
        let mut runs = 0;
        for run in dark_runs(m) {
            runs += 1;
            for x in run.x..run.x + run.len {
                let i = run.y as usize * m.width() + x as usize;
                assert!(!grid[i], "module drawn twice");
                grid[i] = true;
            }
        }
        assert_eq!(grid, m.modules());
        assert!(runs < m.dark_count(), "runs should merge modules");
    }

    #[test]
    fn empty_rows_yield_nothing() {
        let m = Matrix::from_modules(3, vec![false; 9]).unwrap();
        assert_eq!(dark_runs(&m).count(), 0);
    }
}
