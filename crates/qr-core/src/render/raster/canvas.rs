//! The checked pixel geometry of a raster image and its rows.

use super::MAX_PIXELS;
use crate::matrix::Matrix;
use crate::render::options::{MAX_CANVAS_SIDE, MAX_DPI, RenderError, RenderOptions};

/// A square bitmap of light and dark pixels drawn from a [`Matrix`].
///
/// The image is `side` pixels across: `pad` light pixels, then the symbol
/// and its quiet zone at `scale` pixels per module, then the rest of the
/// side as light pixels. `pad` is non-zero only when a target size is not a
/// whole multiple of the module count.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Canvas<'m> {
    matrix: &'m Matrix,
    side: u32,
    pad: u32,
    margin: u32,
    scale: u32,
}

impl<'m> Canvas<'m> {
    /// Checks `options` for `format` and returns the canvas to draw.
    ///
    /// Every limit from DECISIONS section 4.2 is applied here, with checked
    /// arithmetic: at most [`MAX_CANVAS_SIDE`] pixels per side and
    /// [`MAX_PIXELS`] in total, and at least `min_scale` pixels per module.
    pub(crate) fn new(
        matrix: &'m Matrix,
        options: &RenderOptions,
        format: &'static str,
        min_scale: u32,
    ) -> Result<Self, RenderError> {
        if let Some(dpi) = options.dpi.filter(|d| !(1..=MAX_DPI).contains(d)) {
            return Err(RenderError::DpiOutOfRange { dpi, max: MAX_DPI });
        }
        let layout = options.layout(matrix, format, MAX_CANVAS_SIDE)?;
        if layout.scale < min_scale {
            return Err(RenderError::ScaleTooSmall {
                format,
                scale: layout.scale,
                min: min_scale,
            });
        }
        // `layout` guarantees the symbol fits inside a requested size.
        let side = options.size.unwrap_or(layout.side);
        if side > MAX_CANVAS_SIDE {
            return Err(RenderError::TooLarge {
                format,
                side: u64::from(side),
                max: MAX_CANVAS_SIDE,
            });
        }
        let pixels = u64::from(side) * u64::from(side);
        if pixels > MAX_PIXELS {
            return Err(RenderError::TooManyPixels {
                format,
                pixels,
                max: MAX_PIXELS,
            });
        }
        let extra = side.checked_sub(layout.side).ok_or(RenderError::TooLarge {
            format,
            side: u64::from(layout.side),
            max: side,
        })?;
        Ok(Self {
            matrix,
            side,
            pad: extra / 2,
            margin: layout.margin,
            scale: layout.scale,
        })
    }

    /// Width and height in pixels.
    pub(crate) fn side(&self) -> u32 {
        self.side
    }

    /// Pixels per module.
    #[cfg(test)]
    pub(crate) fn scale(&self) -> u32 {
        self.scale
    }

    /// The matrix row drawn on pixel row `y`, or `None` for a row of the
    /// quiet zone or padding. Rows with the same key are identical.
    pub(crate) fn source_row(&self, y: u32) -> Option<usize> {
        let module = y.checked_sub(self.pad)? / self.scale;
        let row = usize::try_from(module.checked_sub(self.margin)?).ok()?;
        (row < self.matrix.width()).then_some(row)
    }

    /// Calls `run(dark, len)` for each run of equal pixels in row `y`, from
    /// left to right. Adjacent runs differ and the lengths add up to
    /// [`Self::side`].
    pub(crate) fn row_runs(&self, y: u32, mut run: impl FnMut(bool, u32)) {
        let Some(row) = self.source_row(y) else {
            run(false, self.side);
            return;
        };
        let mut current = (false, self.pad + self.margin * self.scale);
        let mut drawn = current.1;
        for x in 0..self.matrix.width() {
            let dark = self.matrix.is_dark(x, row);
            if dark == current.0 {
                current.1 += self.scale;
            } else {
                if current.1 > 0 {
                    run(current.0, current.1);
                }
                current = (dark, self.scale);
            }
            drawn += self.scale;
        }
        // The right-hand quiet zone and padding.
        let rest = self.side.saturating_sub(drawn);
        if current.0 {
            run(true, current.1);
            if rest > 0 {
                run(false, rest);
            }
        } else if current.1 + rest > 0 {
            run(false, current.1 + rest);
        }
    }

    /// Replaces `out` with row `y`, one `light` or `dark` value per pixel.
    pub(crate) fn fill_row<T: Copy>(&self, y: u32, light: T, dark: T, out: &mut Vec<T>) {
        out.clear();
        self.row_runs(y, |is_dark, len| {
            let value = if is_dark { dark } else { light };
            out.extend(std::iter::repeat_n(value, len as usize));
        });
    }

    /// Replaces `out` with row `y` packed one bit per pixel, most
    /// significant bit first and dark as 1, as PNG stores 1-bit rows.
    pub(crate) fn fill_bits(&self, y: u32, out: &mut Vec<u8>) {
        out.clear();
        let (mut byte, mut bits) = (0_u8, 0_u32);
        self.row_runs(y, |dark, len| {
            for _ in 0..len {
                byte = (byte << 1) | u8::from(dark);
                bits += 1;
                if bits == 8 {
                    out.push(byte);
                    (byte, bits) = (0, 0);
                }
            }
        });
        if bits > 0 {
            out.push(byte << (8 - bits));
        }
    }

    /// Calls `emit` with every row from top to bottom. `render` builds a row
    /// into the buffer; it runs once per distinct row, not once per pixel
    /// row, because a module row repeats `scale` times.
    pub(crate) fn for_each_row<T, E>(
        &self,
        mut render: impl FnMut(u32, &mut Vec<T>),
        mut emit: impl FnMut(&[T]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut row = Vec::new();
        let mut last = None;
        for y in 0..self.side {
            let key = Some(self.source_row(y));
            if key != last {
                render(y, &mut row);
                last = key;
            }
            emit(&row)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x2: dark, dark / light, dark.
    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    fn rows(canvas: &Canvas<'_>) -> Vec<String> {
        let mut out = Vec::new();
        let mut row = Vec::new();
        for y in 0..canvas.side() {
            canvas.fill_row(y, '.', '#', &mut row);
            out.push(row.iter().collect());
        }
        out
    }

    #[test]
    fn draws_modules_and_quiet_zone() {
        let m = two_by_two();
        let options = RenderOptions::default().with_margin(1).with_scale(2);
        let canvas = Canvas::new(&m, &options, "png", 1).unwrap();
        assert_eq!((canvas.side(), canvas.scale()), (8, 2));
        assert_eq!(
            rows(&canvas),
            [
                "........", "........", "..####..", "..####..", "....##..", "....##..", "........",
                "........",
            ]
        );
        assert_eq!(canvas.source_row(0), None);
        assert_eq!(canvas.source_row(2), Some(0));
        assert_eq!(canvas.source_row(5), Some(1));
        assert_eq!(canvas.source_row(6), None);
    }

    #[test]
    fn a_target_size_pads_the_quiet_zone() {
        let m = two_by_two();
        // 4 modules; 11 px fits a scale of 2 with 3 px left over.
        let options = RenderOptions::default().with_margin(1).with_size(Some(11));
        let canvas = Canvas::new(&m, &options, "png", 1).unwrap();
        assert_eq!((canvas.side(), canvas.scale()), (11, 2));
        let rows = rows(&canvas);
        assert_eq!(rows[3], "...####....");
        assert_eq!(rows[5], ".....##....");
        assert_eq!(rows[7], "...........");
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn runs_cover_the_row_exactly() {
        let m = two_by_two();
        for margin in [0, 1, 3] {
            for size in [None, Some(9), Some(13)] {
                let options = RenderOptions::default()
                    .with_margin(margin)
                    .with_scale(1)
                    .with_size(size);
                let canvas = Canvas::new(&m, &options, "png", 1).unwrap();
                for y in 0..canvas.side() {
                    let mut runs = Vec::new();
                    canvas.row_runs(y, |dark, len| runs.push((dark, len)));
                    assert_eq!(runs.iter().map(|r| r.1).sum::<u32>(), canvas.side());
                    assert!(runs.iter().all(|r| r.1 > 0), "{runs:?}");
                    assert!(runs.windows(2).all(|w| w[0].0 != w[1].0), "{runs:?}");
                }
            }
        }
        // With no margin, a row can start and end dark.
        let full = Matrix::from_modules(1, vec![true]).unwrap();
        let options = RenderOptions::default().with_margin(0).with_scale(3);
        let canvas = Canvas::new(&full, &options, "png", 1).unwrap();
        let mut runs = Vec::new();
        canvas.row_runs(0, |dark, len| runs.push((dark, len)));
        assert_eq!(runs, [(true, 3)]);
    }

    #[test]
    fn packs_bits_most_significant_first() {
        let m = two_by_two();
        let options = RenderOptions::default().with_margin(1).with_scale(3);
        let canvas = Canvas::new(&m, &options, "png", 1).unwrap();
        let mut bits = Vec::new();
        canvas.fill_bits(3, &mut bits);
        // 12 px: 3 light, 6 dark, 3 light.
        assert_eq!(bits, [0b0001_1111, 0b1000_0000]);
        canvas.fill_bits(0, &mut bits);
        assert_eq!(bits, [0, 0]);
        let options = options.with_scale(2); // 8 px, exactly one byte
        let canvas = Canvas::new(&m, &options, "png", 1).unwrap();
        canvas.fill_bits(4, &mut bits);
        assert_eq!(bits, [0b0000_1100]);
    }

    #[test]
    fn rows_are_built_once_per_module_row() {
        let m = two_by_two();
        let options = RenderOptions::default().with_margin(1).with_scale(4);
        let canvas = Canvas::new(&m, &options, "png", 1).unwrap();
        let (mut built, mut emitted) = (0, 0);
        let result: Result<(), ()> = canvas.for_each_row(
            |y, row| {
                built += 1;
                canvas.fill_row(y, 0_u8, 1, row);
            },
            |row| {
                emitted += 1;
                assert_eq!(row.len(), 16);
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        // Quiet, row 0, row 1, quiet.
        assert_eq!((built, emitted), (4, 16));
        let failed: Result<(), &str> =
            canvas.for_each_row(|_, _: &mut Vec<u8>| {}, |_| Err("stop"));
        assert_eq!(failed, Err("stop"));
    }

    #[test]
    fn limits_are_enforced() {
        let m = two_by_two();
        let check =
            |options: RenderOptions, min| Canvas::new(&m, &options, "jpeg", min).map(|c| c.side());
        let base = RenderOptions::default().with_margin(0);
        assert_eq!(check(base.clone().with_scale(4), 4), Ok(8));
        assert_eq!(
            check(base.clone().with_scale(3), 4),
            Err(RenderError::ScaleTooSmall {
                format: "jpeg",
                scale: 3,
                min: 4
            })
        );
        assert_eq!(
            check(base.clone().with_size(Some(7)), 4),
            Err(RenderError::ScaleTooSmall {
                format: "jpeg",
                scale: 3,
                min: 4
            })
        );
        assert_eq!(
            check(base.clone().with_size(Some(MAX_CANVAS_SIDE + 1)), 1),
            Err(RenderError::TooLarge {
                format: "jpeg",
                side: u64::from(MAX_CANVAS_SIDE + 1),
                max: MAX_CANVAS_SIDE
            })
        );
        // 16,384 px per side is allowed by the side limit but is 268 MP.
        assert_eq!(
            check(base.clone().with_scale(MAX_CANVAS_SIDE / 2), 1),
            Err(RenderError::TooManyPixels {
                format: "jpeg",
                pixels: 268_435_456,
                max: MAX_PIXELS
            })
        );
        assert_eq!(
            check(base.clone().with_size(Some(16_000)), 1),
            Ok(16_000),
            "16,000 x 16,000 is exactly 256 MP"
        );
        for dpi in [0, MAX_DPI + 1] {
            assert_eq!(
                check(base.clone().with_dpi(Some(dpi)), 1),
                Err(RenderError::DpiOutOfRange { dpi, max: MAX_DPI })
            );
        }
        assert!(check(base.with_dpi(Some(MAX_DPI)), 1).is_ok());
    }
}
