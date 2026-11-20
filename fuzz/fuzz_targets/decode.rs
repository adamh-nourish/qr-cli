//! `qr decode` with arbitrary bytes: the bounded loader, then every code
//! `qr_core::decode` finds. Neither may panic (a panic aborts a release
//! build), and anything decoded must be self-consistent. An image refused
//! for holding too many finder-like patterns is fine; a slow one is
//! reported by `-timeout` (fuzz.yml). Small inputs are
//! also read as raw greyscale bitmaps, which reaches `rqrr`'s detector with
//! degenerate shapes (thin modules, tiny or lopsided images) far more often
//! than valid image files do.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qr_core::decode::{DecodedCode, decode_bitmap};
use qr_core::loader::{Bitmap, decode_image};

fn check(codes: &[DecodedCode]) {
    for code in codes {
        assert!(code.payload.len() <= rqrr::MAX_PAYLOAD_SIZE);
        assert!((1..=40).contains(&code.version.number()));
        assert!(code.mask.number() < 8);
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(image) = decode_image(data) {
        if let Ok(scan) = decode_bitmap(image) {
            check(&scan.codes);
        }
    }
    // The first byte picks a width; the rest are luma pixels.
    if let Some((&width, pixels)) = data.split_first() {
        let width = usize::from(width.max(1));
        let height = pixels.len() / width;
        if height == 0 {
            return;
        }
        let rgba: Vec<u8> = pixels[..width * height]
            .iter()
            .flat_map(|&v| [v, v, v, 255])
            .collect();
        let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
            return;
        };
        if let Some(bitmap) = Bitmap::from_rgba(w, h, rgba) {
            if let Ok(scan) = decode_bitmap(bitmap) {
                check(&scan.codes);
            }
        }
    }
});
