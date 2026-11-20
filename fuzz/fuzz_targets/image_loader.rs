//! The bounded image loader (DECISIONS section 4.3) with arbitrary bytes:
//! it must never panic or allocate past its limits, and any image it
//! accepts must be within them and usable as a logo.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qr_core::loader::{MAX_IMAGE_SIDE, decode_image};
use qr_core::logo::Logo;
use qr_core::{EncodeOptions, RenderOptions, encode, render_png, render_svg};

fuzz_target!(|data: &[u8]| {
    let Ok(image) = decode_image(data) else {
        return;
    };
    assert!(image.width() <= MAX_IMAGE_SIDE && image.height() <= MAX_IMAGE_SIDE);
    assert_eq!(
        image.pixels().len(),
        image.width() as usize * image.height() as usize * 4
    );
    let Ok(code) = encode(b"fuzz", &EncodeOptions::default()) else {
        return;
    };
    let options = RenderOptions::default()
        .with_scale(2)
        .with_logo(Some(Logo::new(image)));
    render_png(code.matrix(), &options).expect("a loaded logo renders as png");
    let svg = render_svg(code.matrix(), &options).expect("a loaded logo renders as svg");
    assert!(svg.contains("href=\"data:image/png;base64,"));
});
