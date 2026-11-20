//! The automatic mask against an independent ISO/IEC 18004 penalty scorer.
//!
//! `tests/differential.rs` only shows that the automatic mask is the one
//! `qrcode` picks. This test scores all eight masks itself, with the four
//! penalty rules of ISO/IEC 18004:2015 section 7.8.3.1 written from the
//! standard rather than from `qrcode`, and checks how often the automatic
//! mask has the lowest score.
//!
//! `qrcode`'s scoring differs from this one in details (for example how
//! N3 treats the symbol edge), so a few choices are not the minimum here.
//! The count is pinned so any change to mask selection is deliberate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qr_core::{EncodeOptions, Mask, Mode, ModeChoice, encode};

/// N1: each run of five or more same-colour modules in a row or column
/// scores 3, plus 1 for each module beyond five.
fn n1(line: &[bool]) -> u32 {
    let mut score = 0;
    let mut run = 0;
    for (i, &m) in line.iter().enumerate() {
        if i > 0 && m == line[i - 1] {
            run += 1;
        } else {
            run = 1;
        }
        if run == 5 {
            score += 3;
        } else if run > 5 {
            score += 1;
        }
    }
    score
}

/// N3: each 1:1:3:1:1 finder-like pattern (dark, light, dark x3, light,
/// dark) with four light modules on either side scores 40. The quiet zone
/// counts as light, so the pattern can sit against the symbol edge.
fn n3(line: &[bool]) -> u32 {
    const CORE: [bool; 7] = [true, false, true, true, true, false, true];
    let mut padded = vec![false; 4];
    padded.extend_from_slice(line);
    padded.extend([false; 4]);
    let mut score = 0;
    for start in 4..=padded.len() - 11 {
        if padded[start..start + 7] != CORE {
            continue;
        }
        let before = padded[start - 4..start].iter().all(|&m| !m);
        let after = padded[start + 7..start + 11].iter().all(|&m| !m);
        if before || after {
            score += 40;
        }
    }
    score
}

/// The total penalty of a symbol (N1 to N4).
fn penalty(width: usize, modules: &[bool]) -> u32 {
    let at = |x: usize, y: usize| modules[y * width + x];
    let rows: Vec<Vec<bool>> = (0..width)
        .map(|y| (0..width).map(|x| at(x, y)).collect())
        .collect();
    let cols: Vec<Vec<bool>> = (0..width)
        .map(|x| (0..width).map(|y| at(x, y)).collect())
        .collect();
    let lines = || rows.iter().chain(cols.iter());

    let n1: u32 = lines().map(|l| n1(l)).sum();
    // N2: each 2x2 block of one colour scores 3.
    let mut n2 = 0;
    for y in 0..width - 1 {
        for x in 0..width - 1 {
            let c = at(x, y);
            if at(x + 1, y) == c && at(x, y + 1) == c && at(x + 1, y + 1) == c {
                n2 += 3;
            }
        }
    }
    let n3: u32 = lines().map(|l| n3(l)).sum();
    // N4: 10 for each full 5% the dark proportion is away from 50%.
    let dark = modules.iter().filter(|&&m| m).count();
    let total = modules.len();
    let deviation = (dark * 100).abs_diff(total * 50) / total;
    let n4 = u32::try_from(deviation / 5).unwrap() * 10;
    n1 + n2 + n3 + n4
}

#[test]
fn scorer_matches_hand_counted_lines() {
    let line = |s: &str| s.bytes().map(|b| b == b'1').collect::<Vec<bool>>();
    assert_eq!(n1(&line("11110")), 0);
    assert_eq!(n1(&line("111110")), 3);
    assert_eq!(n1(&line("1111111000000")), 3 + 2 + 3 + 1);
    // Light modules after the pattern; at the left edge the quiet zone
    // supplies the light modules before it.
    assert_eq!(n3(&line("1011101")), 40);
    assert_eq!(n3(&line("110111010000")), 40);
    assert_eq!(n3(&line("11011101011")), 0);
    // A dark 2x2 block on light: N2 3, N4 for 4/9 dark (5.6% off) 10.
    let m = [true, true, false, true, true, false, false, false, false];
    assert_eq!(penalty(3, &m), 3 + 10);
}

/// The automatic mask has the lowest independent ISO penalty in 297 of 300
/// seeded random byte inputs (0..600 bytes, EC level M, fixed byte mode).
/// In the others `qrcode`'s scoring disagrees with this scorer by a few
/// points.
#[test]
fn automatic_mask_has_the_lowest_iso_penalty() {
    let mut state: u64 = 0x5eed_0000_0000_0002;
    let mut next = || {
        // splitmix64: deterministic and dependency-free.
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let options = EncodeOptions::default().with_mode(ModeChoice::Fixed(Mode::Byte));
    let mut lowest = 0;
    for _ in 0..300 {
        let len = usize::try_from(next() % 600).unwrap();
        let data: Vec<u8> = (0..len).map(|_| next().to_le_bytes()[0]).collect();
        let auto = encode(&data, &options).unwrap();
        let width = auto.matrix().width();
        let scores: Vec<u32> = Mask::all()
            .map(|mask| {
                let forced = encode(&data, &options.with_mask(Some(mask))).unwrap();
                assert_eq!(forced.version(), auto.version());
                penalty(width, forced.matrix().modules())
            })
            .collect();
        let chosen = scores[usize::from(auto.mask().number())];
        assert_eq!(
            chosen,
            penalty(width, auto.matrix().modules()),
            "the automatic symbol is the forced one for its mask"
        );
        if chosen == *scores.iter().min().unwrap() {
            lowest += 1;
        }
    }
    assert_eq!(
        lowest, 297,
        "automatic mask is the ISO minimum in {lowest}/300"
    );
}
