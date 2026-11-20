//! Resampling a logo to the size it is drawn at.
//!
//! Downscaling averages the source pixels each target pixel covers (an
//! area, or box, filter); upscaling interpolates bilinearly. Both work on
//! premultiplied alpha, so transparent pixels never bleed their (invisible)
//! colour into the edge of the logo.
//!
//! Memory stays proportional to the output: rows are produced one at a
//! time, each from the few source rows it covers, into a single row of
//! accumulators the width of the source.

use crate::color::Color;
use crate::loader::Bitmap;

/// The weighted source indices that make up one target index on one axis.
type Taps = Vec<(usize, f64)>;

/// `bitmap` resampled to `width` x `height` straight-alpha pixels,
/// row-major, or `None` if a side is zero or the buffer cannot be
/// allocated.
pub(crate) fn resample(bitmap: &Bitmap, width: u32, height: u32) -> Option<Vec<Color>> {
    if width == 0 || height == 0 {
        return None;
    }
    let (src_w, src_h) = (bitmap.width(), bitmap.height());
    let columns = axis(src_w, width);
    let rows = axis(src_h, height);
    let len = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    let mut out = Vec::new();
    out.try_reserve_exact(len).ok()?;

    let pixels = bitmap.pixels();
    let stride = src_w as usize * 4;
    let mut acc = vec![[0.0_f64; 4]; src_w as usize];
    for taps in &rows {
        acc.fill([0.0; 4]);
        for &(sy, wy) in taps {
            let row = pixels.get(sy * stride..(sy + 1) * stride)?;
            for (sum, px) in acc.iter_mut().zip(row.chunks_exact(4)) {
                let alpha = f64::from(px[3]) / 255.0;
                let weight = wy * alpha;
                sum[0] += weight * f64::from(px[0]);
                sum[1] += weight * f64::from(px[1]);
                sum[2] += weight * f64::from(px[2]);
                sum[3] += wy * alpha;
            }
        }
        for taps in &columns {
            let mut px = [0.0_f64; 4];
            for &(sx, wx) in taps {
                let sum = acc.get(sx)?;
                for (p, s) in px.iter_mut().zip(sum) {
                    *p += wx * s;
                }
            }
            out.push(unpremultiply(px));
        }
    }
    Some(out)
}

/// The straight-alpha colour of an accumulated premultiplied pixel, whose
/// colour channels are in 0..=255 and alpha in 0..=1.
fn unpremultiply([r, g, b, a]: [f64; 4]) -> Color {
    if a <= 1e-9 {
        return Color::TRANSPARENT;
    }
    Color::rgba(to_u8(r / a), to_u8(g / a), to_u8(b / a), to_u8(a * 255.0))
}

/// `value` rounded and clamped to a byte.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u8(value: f64) -> u8 {
    // Clamped to 0..=255 first, so the cast is exact.
    value.round().clamp(0.0, 255.0) as u8
}

/// The source taps for each of `dst` target indices, from `src` source
/// indices: an area average when shrinking, bilinear when growing (or
/// keeping) the size. The weights of each target index add up to 1.
fn axis(src: u32, dst: u32) -> Vec<Taps> {
    let (s, d) = (f64::from(src), f64::from(dst));
    let last = src.saturating_sub(1) as usize;
    (0..dst)
        .map(|i| {
            let i = f64::from(i);
            if dst < src {
                let ratio = s / d;
                let (lo, hi) = (i * ratio, (i + 1.0) * ratio);
                let (first, end) = (floor(lo), ceil(hi).min(src as usize));
                (first..end)
                    .filter_map(|j| {
                        let j_f = index_f64(j);
                        let cover = hi.min(j_f + 1.0) - lo.max(j_f);
                        (cover > 0.0).then_some((j, cover / ratio))
                    })
                    .collect()
            } else {
                let x = ((i + 0.5) * s / d - 0.5).clamp(0.0, f64::from(src) - 1.0);
                let j0 = floor(x).min(last);
                let j1 = (j0 + 1).min(last);
                let frac = x - index_f64(j0);
                if j1 == j0 || frac <= 0.0 {
                    vec![(j0, 1.0)]
                } else {
                    vec![(j0, 1.0 - frac), (j1, frac)]
                }
            }
        })
        .collect()
}

/// `x.floor()` as an index; `x` is non-negative and below 2^32 here.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn floor(x: f64) -> usize {
    x.floor().max(0.0) as usize
}

/// `x.ceil()` as an index; `x` is non-negative and below 2^32 here.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ceil(x: f64) -> usize {
    x.ceil().max(0.0) as usize
}

/// An index, which is below 2^32 here, as a float.
fn index_f64(i: usize) -> f64 {
    u32::try_from(i).map_or(f64::from(u32::MAX), f64::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(width: u32, height: u32, pixels: &[[u8; 4]]) -> Bitmap {
        Bitmap::from_rgba(width, height, pixels.concat()).unwrap()
    }

    #[test]
    fn same_size_is_the_identity() {
        let px = [
            [1, 2, 3, 255],
            [200, 100, 50, 255],
            [9, 9, 9, 128],
            [0, 0, 0, 0],
        ];
        let out = resample(&bitmap(2, 2, &px), 2, 2).unwrap();
        assert_eq!(
            out,
            [
                Color::rgba(1, 2, 3, 255),
                Color::rgba(200, 100, 50, 255),
                Color::rgba(9, 9, 9, 128),
                Color::TRANSPARENT,
            ]
        );
    }

    #[test]
    fn shrinking_averages_the_area() {
        // Black and white columns average to mid grey.
        let px = [[0, 0, 0, 255], [255, 255, 255, 255]].repeat(4);
        let out = resample(&bitmap(2, 4, &px), 1, 1).unwrap();
        assert_eq!(out, [Color::rgb(128, 128, 128)]);
        // Three pixels to two: the middle one is split between both.
        let px = [[0, 0, 0, 255], [90, 90, 90, 255], [180, 180, 180, 255]];
        let out = resample(&bitmap(3, 1, &px), 2, 1).unwrap();
        assert_eq!(out, [Color::rgb(30, 30, 30), Color::rgb(150, 150, 150)]);
    }

    #[test]
    fn transparent_pixels_do_not_bleed_colour() {
        // Opaque red next to fully transparent green: the average is red
        // at half opacity, not a red-green mix.
        let px = [[255, 0, 0, 255], [0, 255, 0, 0]];
        let out = resample(&bitmap(2, 1, &px), 1, 1).unwrap();
        assert_eq!(out, [Color::rgba(255, 0, 0, 128)]);
        let clear = resample(&bitmap(1, 1, &[[9, 9, 9, 0]]), 3, 3).unwrap();
        assert!(clear.iter().all(|c| *c == Color::TRANSPARENT));
    }

    #[test]
    fn growing_interpolates() {
        let px = [[0, 0, 0, 255], [200, 200, 200, 255]];
        let out = resample(&bitmap(2, 1, &px), 4, 1).unwrap();
        let grey: Vec<u8> = out.iter().map(|c| c.r).collect();
        assert_eq!(grey, [0, 50, 150, 200]);
        // A single pixel grows into a flat block.
        let out = resample(&bitmap(1, 1, &[[7, 8, 9, 255]]), 3, 2).unwrap();
        assert_eq!(out, vec![Color::rgb(7, 8, 9); 6]);
    }

    #[test]
    fn weights_add_up_to_one() {
        for (src, dst) in [
            (1, 1),
            (3, 2),
            (7, 3),
            (100, 17),
            (5, 11),
            (2, 9),
            (640, 64),
        ] {
            for taps in axis(src, dst) {
                let sum: f64 = taps.iter().map(|t| t.1).sum();
                assert!((sum - 1.0).abs() < 1e-9, "{src}->{dst}: {taps:?}");
                assert!(taps.iter().all(|t| t.0 < src as usize));
            }
        }
    }

    #[test]
    fn zero_sizes_are_refused() {
        let one = bitmap(1, 1, &[[0, 0, 0, 255]]);
        assert!(resample(&one, 0, 1).is_none());
        assert!(resample(&one, 1, 0).is_none());
        assert_eq!(to_u8(-3.0), 0);
        assert_eq!(to_u8(300.0), 255);
    }
}
