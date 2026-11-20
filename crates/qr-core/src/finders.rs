//! A bound on the work `rqrr` does to group finder patterns (DECISIONS
//! section 4.3).
//!
//! `rqrr` 0.9 finds every finder pattern (a "capstone") in an image, then
//! tries candidate triples of them. For each candidate pair it clones the
//! whole prepared image, and the number of pairs grows faster than the
//! square of the number of capstones. A few kilobytes of PNG holding a
//! grid of 7 x 7 finder patterns can therefore keep it busy for minutes.
//!
//! [`count`] repeats `rqrr`'s capstone test on the prepared (thresholded)
//! image before `rqrr` does, in one pass over the pixels, and
//! [`too_many`] refuses an image with more than [`MAX_FINDER_CANDIDATES`].
//! `rqrr` makes a capstone where its row scan finds dark and light runs
//! in a 1:1:3:1:1 ratio and:
//!
//! - the first and last dark runs (the ring) are one connected region;
//! - the middle dark run (the stone) is another;
//! - the stone has more than 10% and less than 70% of the ring's pixels;
//! - the ring is not already part of a capstone.
//!
//! Painting the regions it inspects never changes where dark and light
//! runs start and end (a region is a whole connected component), so the
//! scan here hits exactly where `rqrr`'s does. Regions are found by
//! labelling the dark runs of each row and joining those that touch the
//! row above (4-connected, as `rqrr`'s flood fill is). Every hit that
//! passes the first three tests is a candidate, and candidates are counted
//! once per ring, so the count is never below the number of capstones
//! `rqrr` will group (it skips only `rqrr`'s check that the ring's corners
//! give a usable perspective).
//!
//! # Memory
//!
//! One `u32` per dark run (its parent region, or its area if it is a
//! region's root) plus three per hit: at most about 8 MiB for 4,096 x
//! 4,096 pixels in practice, and 2 bytes per pixel for pathological
//! noise, all freed before `rqrr` runs.

/// The most finder-pattern candidates an image may hold before `rqrr`
/// groups them.
///
/// Every QR code has three finder patterns, and the data area of a real
/// code seldom adds another, so this allows a sheet of about twenty codes.
/// A grid of 20 x 20 finder patterns (400) took `rqrr` 2.7 s to reject and
/// a grid of 40 x 40 more than three minutes; grids at this limit take
/// well under a second.
pub const MAX_FINDER_CANDIDATES: usize = 64;

/// Whether a `width` x `height` image whose pixel at `(x, y)` is dark when
/// `dark(x, y)` is true holds more than [`MAX_FINDER_CANDIDATES`]
/// finder-pattern candidates.
pub(crate) fn too_many(width: usize, height: usize, dark: impl Fn(usize, usize) -> bool) -> bool {
    count(width, height, dark) > MAX_FINDER_CANDIDATES
}

/// Marks a region's root in [`Regions`]: the rest of the value is its
/// area. Images have fewer than 2^31 pixels (the loader's limits).
const ROOT: u32 = 1 << 31;

/// Dark runs joined into connected regions (union-find). Each entry is a
/// run's parent run, or [`ROOT`] plus the region's area.
struct Regions(Vec<u32>);

impl Regions {
    /// Adds a run of `len` pixels as a region of its own; returns its id.
    fn push(&mut self, len: usize) -> u32 {
        let id = u32::try_from(self.0.len()).unwrap_or(u32::MAX);
        let len = u32::try_from(len).map_or(ROOT - 1, |len| len.min(ROOT - 1));
        self.0.push(ROOT | len);
        id
    }

    /// The root run of `id`'s region.
    fn find(&mut self, mut id: u32) -> u32 {
        while self.0[id as usize] & ROOT == 0 {
            let parent = self.0[id as usize];
            // Path halving.
            let grand = self.0[parent as usize];
            if grand & ROOT == 0 {
                self.0[id as usize] = grand;
            }
            id = parent;
        }
        id
    }

    /// The area of the region whose root is `root`.
    fn area(&self, root: u32) -> u32 {
        self.0[root as usize] & !ROOT
    }

    /// Joins the regions of `a` and `b`.
    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return;
        }
        // The larger region stays the root, which keeps paths short.
        let (root, child) = if self.area(a) >= self.area(b) {
            (a, b)
        } else {
            (b, a)
        };
        let area = (self.area(root) + self.area(child)).min(ROOT - 1);
        self.0[root as usize] = ROOT | area;
        self.0[child as usize] = root;
    }
}

/// A dark run on one row: its first and last x and its region id.
#[derive(Clone, Copy)]
struct Run {
    first: usize,
    last: usize,
    id: u32,
}

/// Counts the finder-pattern candidates in a `width` x `height` image
/// whose pixel at `(x, y)` is dark when `dark(x, y)` is true: the distinct
/// rings of the hits that pass `rqrr`'s capstone tests (module docs).
pub(crate) fn count(width: usize, height: usize, dark: impl Fn(usize, usize) -> bool) -> usize {
    let mut regions = Regions(Vec::new());
    // The ring, stone and ring runs of every 1:1:3:1:1 hit.
    let mut hits: Vec<[u32; 3]> = Vec::new();
    let mut above: Vec<Run> = Vec::new();
    let mut here: Vec<Run> = Vec::new();
    for y in 0..height {
        if width == 0 {
            break;
        }
        here.clear();
        let first = dark(0, y);
        let mut scanner = Scanner::new(first);
        let mut start = first.then_some(0);
        // Each hit ends at a light pixel, just after the ring's last run,
        // so the hit's dark runs are the last three closed on this row.
        let mut close = |start: &mut Option<usize>, end: usize, here: &mut Vec<Run>| {
            if let Some(first) = start.take() {
                let id = regions.push(end - first);
                here.push(Run {
                    first,
                    last: end - 1,
                    id,
                });
            }
        };
        let mut hit = |here: &[Run]| {
            if let [.., left, stone, right] = here {
                hits.push([left.id, stone.id, right.id]);
            }
        };
        for x in 1..width {
            let is_dark = dark(x, y);
            if is_dark {
                start.get_or_insert(x);
            } else {
                close(&mut start, x, &mut here);
            }
            if scanner.advance(is_dark) {
                hit(&here);
            }
        }
        // `rqrr` ends every row with a virtual light pixel.
        close(&mut start, width, &mut here);
        if scanner.advance(false) {
            hit(&here);
        }
        // Join runs that touch a run on the row above.
        let mut i = 0;
        for run in &here {
            while i < above.len() && above[i].last < run.first {
                i += 1;
            }
            let mut j = i;
            while j < above.len() && above[j].first <= run.last {
                regions.union(run.id, above[j].id);
                j += 1;
            }
        }
        std::mem::swap(&mut above, &mut here);
    }
    let mut rings: Vec<u32> = hits
        .into_iter()
        .filter_map(|[left, stone, right]| {
            let (left, stone, ring) =
                (regions.find(left), regions.find(stone), regions.find(right));
            let ratio = u64::from(regions.area(stone)) * 100 / u64::from(regions.area(ring).max(1));
            (left == ring && stone != ring && 10 < ratio && ratio < 70).then_some(ring)
        })
        .collect();
    rings.sort_unstable();
    rings.dedup();
    rings.len()
}

/// `rqrr` 0.9's `LineScanner`: the run lengths of the last five colour
/// changes along a row, tested for a 1:1:3:1:1 ratio with the same integer
/// tolerance.
struct Scanner {
    runs: [usize; 5],
    dark: bool,
    run: usize,
    changes: usize,
}

impl Scanner {
    const fn new(dark: bool) -> Self {
        Self {
            runs: [0; 5],
            dark,
            run: 1,
            changes: 0,
        }
    }

    /// Advances one pixel; returns whether this (light) pixel completes a
    /// dark 1:1:3:1:1 pattern.
    fn advance(&mut self, dark: bool) -> bool {
        if self.dark == dark {
            self.run += 1;
            return false;
        }
        self.dark = dark;
        self.runs.rotate_left(1);
        self.runs[4] = self.run;
        self.run = 1;
        self.changes += 1;
        if dark || self.changes < 5 {
            return false;
        }
        let avg = (self.runs[0] + self.runs[1] + self.runs[3] + self.runs[4]) / 4;
        let err = avg * 3 / 4;
        self.runs
            .iter()
            .zip([1, 1, 3, 1, 1])
            .all(|(&run, ratio)| run + err >= ratio * avg && run <= ratio * avg + err)
    }
}

#[cfg(test)]
/// A light `side` x `side` image holding an `n` x `n` grid of finder
/// patterns `module` pixels per module, `gap` modules apart: the shape of
/// the hostile image from the PR 11 security review.
pub(crate) fn finder_grid(n: u32, module: u32, gap: u32, side: u32) -> crate::loader::Bitmap {
    let mut pixels = vec![255u8; (side * side * 4) as usize];
    let period = (7 + gap) * module;
    for gy in 0..n {
        for gx in 0..n {
            for y in 0..7 * module {
                for x in 0..7 * module {
                    let ring = (x / module).abs_diff(3).max((y / module).abs_diff(3));
                    let (px, py) = (4 * module + gx * period + x, 4 * module + gy * period + y);
                    if (ring == 3 || ring <= 1) && px < side && py < side {
                        let at = ((py * side + px) * 4) as usize;
                        pixels[at..at + 3].fill(0);
                    }
                }
            }
        }
    }
    crate::loader::Bitmap::from_rgba(side, side, pixels).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A light image `side` pixels square with an `n` x `n` grid of finder
    /// patterns `module` pixels per module, `gap` modules apart.
    fn grid(n: usize, module: usize, gap: usize, side: usize) -> Vec<bool> {
        let mut image = vec![false; side * side];
        let period = (7 + gap) * module;
        for gy in 0..n {
            for gx in 0..n {
                for y in 0..7 * module {
                    for x in 0..7 * module {
                        let ring = (x / module).abs_diff(3).max((y / module).abs_diff(3));
                        let (px, py) = (4 * module + gx * period + x, 4 * module + gy * period + y);
                        if px < side && py < side {
                            image[py * side + px] = ring == 3 || ring <= 1;
                        }
                    }
                }
            }
        }
        image
    }

    #[test]
    fn counts_each_finder_pattern_once() {
        for (n, module) in [(1, 1), (1, 3), (2, 5), (4, 2), (10, 3)] {
            let side = 8 * module + n * 9 * module;
            let image = grid(n, module, 2, side);
            let found = count(side, side, |x, y| image[y * side + x]);
            assert_eq!(found, n * n, "{n} x {n} at {module} px");
        }
    }

    #[test]
    fn the_limit_is_on_the_number_of_patterns() {
        let side = 8 * 3 + 9 * 9 * 3;
        let image = grid(8, 3, 2, side);
        assert!(!too_many(side, side, |x, y| image[y * side + x]));
        let side = 8 * 3 + 9 * 9 * 3;
        let image = grid(9, 3, 2, side);
        assert!(too_many(side, side, |x, y| image[y * side + x]));
    }

    /// One pattern at 2 px per module, changed by `edit` (given the
    /// pattern's module coordinates and whether it is dark).
    fn edited(edit: impl Fn(usize, usize, bool) -> bool) -> usize {
        let side = 18;
        count(side, side, |x, y| {
            let (Some(mx), Some(my)) = ((x / 2).checked_sub(1), (y / 2).checked_sub(1)) else {
                return false;
            };
            if mx > 6 || my > 6 {
                return false;
            }
            let ring = mx.abs_diff(3).max(my.abs_diff(3));
            edit(mx, my, ring == 3 || ring <= 1)
        })
    }

    #[test]
    fn rqrr_tests_are_repeated() {
        assert_eq!(edited(|_, _, dark| dark), 1);
        // A ring cut in two on the left and right is two regions.
        assert_eq!(edited(|_, my, dark| dark && my != 0 && my != 6), 0);
        // A stone joined to the ring is not a separate region.
        assert_eq!(edited(|mx, my, dark| dark || (mx == 3 && my == 1)), 0);
        // A stone too small for the ring: one module of nine.
        assert_eq!(
            edited(|mx, my, dark| if mx.abs_diff(3).max(my.abs_diff(3)) <= 1 {
                mx == 3 && my == 3
            } else {
                dark
            }),
            0
        );
    }

    #[test]
    fn a_pattern_on_the_right_edge_is_found() {
        // The virtual light pixel at the end of each row completes it.
        let side = 14;
        let at = |x: usize, y: usize| {
            let ring = (x / 2).abs_diff(3).max((y / 2).abs_diff(3));
            ring == 3 || ring <= 1
        };
        assert_eq!(count(side, side, at), 1);
    }

    #[test]
    fn empty_and_plain_images_hold_none() {
        assert_eq!(count(0, 5, |_, _| true), 0);
        assert_eq!(count(5, 0, |_, _| true), 0);
        assert_eq!(count(9, 9, |_, _| false), 0);
        assert_eq!(count(9, 9, |x, _| x % 2 == 0), 0);
    }
}
