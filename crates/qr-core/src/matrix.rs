//! The module grid: the single representation every renderer draws from.

use thiserror::Error;

/// A [`Matrix`] could not be built from the given modules.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MatrixError {
    /// The width was zero.
    #[error("matrix width must be at least 1")]
    Empty,
    /// The number of modules was not `width * width`.
    #[error("expected {expected} modules for a {width}x{width} matrix, got {actual}")]
    WrongLength {
        /// The requested width.
        width: usize,
        /// `width * width`, or `usize::MAX` if that overflows.
        expected: usize,
        /// The number of modules supplied.
        actual: usize,
    },
}

/// A square grid of modules, `true` meaning dark.
///
/// Coordinates are `(x, y)` with the origin at the top-left corner, `x`
/// growing to the right and `y` growing downwards. The grid does not include
/// a quiet zone; renderers add the margin themselves, which
/// [`Matrix::is_dark`] supports by treating out-of-range coordinates as light.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Matrix {
    width: usize,
    modules: Vec<bool>,
}

impl Matrix {
    /// Builds a matrix from row-major modules (`modules[y * width + x]`).
    ///
    /// # Errors
    ///
    /// Returns [`MatrixError`] if `width` is zero or `modules` does not hold
    /// exactly `width * width` entries.
    ///
    /// ```
    /// use qr_core::Matrix;
    /// let m = Matrix::from_modules(2, vec![true, false, false, true]).unwrap();
    /// assert!(m.is_dark(1, 1));
    /// assert!(!m.is_dark(5, 5)); // outside the grid reads as light
    /// ```
    pub fn from_modules(width: usize, modules: Vec<bool>) -> Result<Self, MatrixError> {
        if width == 0 {
            return Err(MatrixError::Empty);
        }
        let expected = width.saturating_mul(width);
        if modules.len() != expected {
            return Err(MatrixError::WrongLength {
                width,
                expected,
                actual: modules.len(),
            });
        }
        Ok(Self { width, modules })
    }

    /// Width (and height) in modules.
    pub fn width(&self) -> usize {
        self.width
    }

    /// The module at `(x, y)`, or `None` if the coordinates are outside the
    /// grid.
    pub fn get(&self, x: usize, y: usize) -> Option<bool> {
        if x < self.width && y < self.width {
            self.modules.get(y * self.width + x).copied()
        } else {
            None
        }
    }

    /// Whether the module at `(x, y)` is dark. Coordinates outside the grid
    /// are light, matching the quiet zone around a symbol.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.get(x, y).unwrap_or(false)
    }

    /// The modules in row-major order.
    pub fn modules(&self) -> &[bool] {
        &self.modules
    }

    /// Iterates over the rows from top to bottom.
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &[bool]> {
        self.modules.chunks_exact(self.width)
    }

    /// The number of dark modules.
    pub fn dark_count(&self) -> usize {
        self.modules.iter().filter(|m| **m).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Matrix {
        // Row 0: dark, light, light; row 1: light, dark, light; row 2: all dark.
        Matrix::from_modules(
            3,
            vec![true, false, false, false, true, false, true, true, true],
        )
        .unwrap()
    }

    #[test]
    fn rejects_bad_shapes() {
        assert_eq!(Matrix::from_modules(0, vec![]), Err(MatrixError::Empty));
        assert_eq!(
            Matrix::from_modules(2, vec![true; 3]),
            Err(MatrixError::WrongLength {
                width: 2,
                expected: 4,
                actual: 3
            })
        );
        let overflow = Matrix::from_modules(usize::MAX, vec![]);
        assert!(matches!(
            overflow,
            Err(MatrixError::WrongLength {
                expected: usize::MAX,
                ..
            })
        ));
        assert_eq!(
            MatrixError::WrongLength {
                width: 2,
                expected: 4,
                actual: 3
            }
            .to_string(),
            "expected 4 modules for a 2x2 matrix, got 3"
        );
        assert_eq!(
            MatrixError::Empty.to_string(),
            "matrix width must be at least 1"
        );
    }

    #[test]
    fn accessors_use_row_major_xy() {
        let m = sample();
        assert_eq!(m.width(), 3);
        assert_eq!(m.get(0, 0), Some(true));
        assert_eq!(m.get(1, 0), Some(false));
        assert_eq!(m.get(1, 1), Some(true));
        assert_eq!(m.get(0, 2), Some(true));
        assert_eq!(m.get(3, 0), None);
        assert_eq!(m.get(0, 3), None);
        assert!(!m.is_dark(10, 10));
        assert_eq!(m.dark_count(), 5);
        assert_eq!(m.modules().len(), 9);
    }

    #[test]
    fn rows_iterate_top_to_bottom() {
        let m = sample();
        let rows: Vec<&[bool]> = m.rows().collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], &[true, false, false]);
        assert_eq!(rows[2], &[true, true, true]);
    }
}
