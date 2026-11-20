//! The typed SVG writer with arbitrary data and options: it must not panic,
//! and every document must be well-formed XML holding only the elements and
//! attributes DECISIONS section 4.4 allows.
//!
//! Byte 0 picks the shape, margin and inversion, bytes 1 to 8 the colours,
//! and the rest is the data to encode.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qr_core::{Color, EncodeOptions, ModuleShape, RenderOptions, encode, render_svg};

fuzz_target!(|data: &[u8]| {
    if data.len() < 9 {
        return;
    }
    let (head, payload) = data.split_at(9);
    let Ok(code) = encode(payload, &EncodeOptions::default()) else {
        return;
    };
    let shape = match head[0] % 3 {
        0 => ModuleShape::Square,
        1 => ModuleShape::Rounded,
        _ => ModuleShape::Dots,
    };
    let options = RenderOptions::default()
        .with_shape(shape)
        .with_margin(usize::from(head[0] >> 4))
        .with_scale(u32::from(head[0] >> 6) + 1)
        .with_transparent(head[0] & 0x08 != 0)
        .with_fg(Color::rgba(head[1], head[2], head[3], head[4]))
        .with_bg(Color::rgba(head[5], head[6], head[7], head[8]))
        .with_title(String::from_utf8_lossy(payload).into_owned());
    let Ok(svg) = render_svg(code.matrix(), &options) else {
        return;
    };

    let doc = roxmltree::Document::parse(&svg).expect("SVG is not well-formed XML");
    let allowed = ["svg", "title", "rect", "path"];
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        let name = node.tag_name().name();
        assert!(allowed.contains(&name), "unexpected element <{name}>");
        for attr in node.attributes() {
            let attr_name = attr.name().to_ascii_lowercase();
            assert!(!attr_name.starts_with("on"), "event attribute {attr_name}");
            assert!(!attr_name.contains("href"), "href attribute {attr_name}");
            assert!(!attr.value().contains("url("), "url() in {attr_name}");
        }
    }
});
