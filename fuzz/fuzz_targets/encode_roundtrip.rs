//! Encode arbitrary bytes and read the symbol back with the independent
//! `rqrr` decoder: every code the encoder produces must decode to exactly
//! its input.
//!
//! Byte 0 picks the error correction level, mode and mask; the rest is the
//! data.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qr_core::{EcLevel, EncodeOptions, Mask, Mode, ModeChoice, encode};

fuzz_target!(|data: &[u8]| {
    let Some((&head, payload)) = data.split_first() else {
        return;
    };
    let ec_level = [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H][usize::from(head & 3)];
    let mode = match (head >> 2) & 3 {
        0 => ModeChoice::Auto,
        1 => ModeChoice::Fixed(Mode::Numeric),
        2 => ModeChoice::Fixed(Mode::Alphanumeric),
        _ => ModeChoice::Fixed(Mode::Byte),
    };
    let mask = if head & 0x80 == 0 {
        None
    } else {
        Mask::new((head >> 4) & 7)
    };
    let options = EncodeOptions::default()
        .with_ec_level(ec_level)
        .with_mode(mode)
        .with_mask(mask);
    // Data the mode cannot hold, or too much of it, is a typed error.
    let Ok(code) = encode(payload, &options) else {
        return;
    };
    let matrix = code.matrix();
    let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(matrix.width(), |x, y| {
        matrix.is_dark(x, y)
    }));
    let mut decoded = Vec::new();
    let meta = grid
        .decode_to(&mut decoded)
        .expect("rqrr could not decode the symbol");
    assert_eq!(decoded, payload);
    // The reported mask (recovered from the format information when the
    // encoder chose it) is the one a scanner reads.
    assert_eq!(meta.mask, u16::from(code.mask().number()));
    if let Some(mask) = mask {
        assert_eq!(code.mask(), mask);
    }
});
