//! Output tests for the vector renderers.
//!
//! Every SVG is parsed with `roxmltree` and checked against the SVG safety
//! rules in DECISIONS §4.4. Every SVG, PDF and EPS file is then read back
//! into a module grid from its own drawing operators, rasterised trivially
//! and decoded with `rqrr`, so the structure is validated end to end without
//! a real rasteriser.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use qr_core::{
    Color, EncodeOptions, Matrix, RenderOptions, VectorFormat, encode, render_eps, render_pdf,
    render_svg, render_vector,
};

/// A module grid read back from a rendering, quiet zone included.
#[derive(Debug, PartialEq, Eq)]
struct Grid {
    side: usize,
    dark: Vec<bool>,
}

impl Grid {
    fn new(side: usize) -> Self {
        Self {
            side,
            dark: vec![false; side * side],
        }
    }

    fn fill_run(&mut self, x: usize, y: usize, len: usize) {
        assert!(
            y < self.side && x + len <= self.side,
            "run outside the frame"
        );
        for x in x..x + len {
            let cell = &mut self.dark[y * self.side + x];
            assert!(!*cell, "module ({x}, {y}) drawn twice");
            *cell = true;
        }
    }

    /// The grid a correct renderer must produce.
    fn expected(matrix: &Matrix, margin: usize) -> Self {
        let side = matrix.width() + 2 * margin;
        let mut grid = Self::new(side);
        for y in 0..side {
            for x in 0..side {
                grid.dark[y * side + x] = matrix.is_dark_with_margin(x, y, margin);
            }
        }
        grid
    }

    /// Decodes the grid as an image with full symbol detection.
    fn decode(&self) -> Vec<u8> {
        let scale = 3;
        let px = self.side * scale;
        let mut img = rqrr::PreparedImage::prepare_from_greyscale(px, px, |x, y| {
            if self.dark[(y / scale) * self.side + x / scale] {
                0
            } else {
                255
            }
        });
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1, "expected exactly one symbol");
        let mut data = Vec::new();
        grids[0]
            .decode_to(&mut data)
            .expect("rqrr failed to decode");
        data
    }
}

// ---------------------------------------------------------------- SVG ----

/// Asserts the DECISIONS §4.4 rules on a parsed SVG and returns it.
fn assert_safe_svg(svg: &str) -> roxmltree::Document<'_> {
    let doc = roxmltree::Document::parse(svg).expect("SVG is not well-formed XML");
    let allowed = ["svg", "title", "rect", "path"];
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        let name = node.tag_name().name();
        assert!(allowed.contains(&name), "unexpected element <{name}>");
        assert_eq!(
            node.tag_name().namespace(),
            Some("http://www.w3.org/2000/svg")
        );
        for attr in node.attributes() {
            let attr_name = attr.name().to_ascii_lowercase();
            assert!(!attr_name.starts_with("on"), "event attribute {attr_name}");
            assert!(!attr_name.contains("href"), "href attribute {attr_name}");
            assert!(
                attr.namespace().is_none(),
                "namespaced attribute {attr_name}"
            );
            assert!(!attr.value().contains("url("), "url() in {attr_name}");
            assert!(!attr.value().to_ascii_lowercase().contains("javascript:"));
        }
        if name != "title" {
            assert!(
                node.children()
                    .all(|c| !c.is_text() || c.text().unwrap().trim().is_empty()),
                "text inside <{name}>"
            );
        }
    }
    for forbidden in ["script", "foreignObject", "image", "use", "style", "a"] {
        assert!(
            doc.descendants().all(|n| n.tag_name().name() != forbidden),
            "<{forbidden}> present"
        );
    }
    assert!(!svg.contains("<!DOCTYPE") && !svg.contains("<!ENTITY"));
    doc
}

/// Reads the module grid back from the `viewBox` and the path data.
fn svg_grid(svg: &str) -> Grid {
    let doc = assert_safe_svg(svg);
    let root = doc.root_element();
    let view_box: Vec<usize> = root
        .attribute("viewBox")
        .unwrap()
        .split(' ')
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(view_box[..2], [0, 0]);
    assert_eq!(view_box[2], view_box[3]);
    let mut grid = Grid::new(view_box[2]);
    let Some(path) = doc.descendants().find(|n| n.has_tag_name("path")) else {
        return grid;
    };
    // M{x} {y} for the first run, then m{dx} {dy} relative to the previous
    // run's start (where `z` leaves the current point), then h{len}v1h-{len}z.
    let (mut x, mut y) = (0i64, 0i64);
    let d = path.attribute("d").unwrap();
    for run in d.split('z').filter(|s| !s.is_empty()) {
        let (absolute, rest) = match run.as_bytes()[0] {
            b'M' => (true, &run[1..]),
            b'm' => (false, &run[1..]),
            other => panic!("unexpected command {}", other as char),
        };
        let (dx, rest) = rest.split_once(' ').unwrap();
        let (dy, rest) = rest.split_once('h').unwrap();
        let (len, rest) = rest.split_once("v1h-").unwrap();
        assert_eq!(rest, len, "run does not close");
        let (dx, dy): (i64, i64) = (dx.parse().unwrap(), dy.parse().unwrap());
        if absolute {
            (x, y) = (dx, dy);
        } else {
            (x, y) = (x + dx, y + dy);
        }
        grid.fill_run(
            usize::try_from(x).unwrap(),
            usize::try_from(y).unwrap(),
            len.parse().unwrap(),
        );
    }
    grid
}

// ---------------------------------------------------------------- PDF ----

/// Checks the cross-reference table, trailer and stream length, and returns
/// the page side and the content stream.
fn pdf_structure(pdf: &[u8]) -> (usize, String) {
    let text = String::from_utf8_lossy(pdf).into_owned();
    assert!(pdf.starts_with(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n"));
    assert!(text.ends_with("%%EOF\n"));

    let startxref: usize = text
        .rsplit("startxref\n")
        .next()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let xref = &pdf[startxref..];
    assert!(xref.starts_with(b"xref\n0 6\n0000000000 65535 f \n"));
    let entries = &xref[b"xref\n0 6\n".len()..];
    for n in 1..=5 {
        let entry = &entries[n * 20..(n + 1) * 20];
        let entry = std::str::from_utf8(entry).unwrap();
        assert!(entry.ends_with(" 00000 n \n"), "{entry:?}");
        let offset: usize = entry[..10].parse().unwrap();
        assert!(
            pdf[offset..].starts_with(format!("{n} 0 obj\n").as_bytes()),
            "xref entry {n} points at the wrong place"
        );
    }
    assert!(text.contains("trailer\n<< /Size 6 /Root 1 0 R /Info 5 0 R >>\n"));

    let (_, after_length) = text.split_once("<< /Length ").unwrap();
    let (length, after) = after_length.split_once(" >>\nstream\n").unwrap();
    let length: usize = length.parse().unwrap();
    let content = &after[..length];
    assert!(after[length..].starts_with("endstream\nendobj\n"));

    let (_, media_box) = text.split_once("/MediaBox [0 0 ").unwrap();
    let (side, _) = media_box.split_once(' ').unwrap();
    (side.parse().unwrap(), content.to_owned())
}

/// Reads the module grid back from the `cm` transform and the `re` operators.
fn pdf_grid(pdf: &[u8]) -> Grid {
    let (side, content) = pdf_structure(pdf);
    let mut scale = None;
    let mut grid = None;
    for line in content.lines() {
        let ops: Vec<&str> = line.split(' ').collect();
        match ops.last().copied() {
            Some("cm") => {
                let s: usize = ops[0].parse().unwrap();
                assert_eq!(
                    ops[..6],
                    [ops[0], "0", "0", &format!("-{s}"), "0", &side.to_string()]
                );
                scale = Some(s);
                grid = Some(Grid::new(side / s));
            }
            Some("re") if scale.is_some() => {
                assert_eq!(ops[3], "1");
                grid.as_mut().unwrap().fill_run(
                    ops[0].parse().unwrap(),
                    ops[1].parse().unwrap(),
                    ops[2].parse().unwrap(),
                );
            }
            _ => {}
        }
    }
    grid.unwrap_or_else(|| Grid::new(0))
}

// ---------------------------------------------------------------- EPS ----

/// Reads the module grid back from the `concat` matrix and `rectfill`s.
fn eps_grid(eps: &str) -> Grid {
    assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0\n"));
    assert!(eps.ends_with("%%EOF\n"));
    assert!(eps.is_ascii());
    let bbox = eps
        .lines()
        .find_map(|l| l.strip_prefix("%%BoundingBox: 0 0 "))
        .unwrap();
    let (side, height) = bbox.split_once(' ').unwrap();
    assert_eq!(side, height);
    let side: usize = side.parse().unwrap();
    assert_eq!(
        eps.matches("gsave").count(),
        eps.matches("grestore").count()
    );

    let mut grid = None;
    for line in eps.lines().filter(|l| !l.starts_with('%')) {
        if let Some(matrix) = line.strip_suffix("] concat") {
            let parts: Vec<&str> = matrix.trim_start_matches('[').split(' ').collect();
            let s: usize = parts[0].parse().unwrap();
            assert_eq!(parts[3], format!("-{s}"));
            assert_eq!(parts[5], side.to_string());
            grid = Some(Grid::new(side / s));
        } else if let (Some(rect), Some(grid)) = (line.strip_suffix(" 1 rectfill"), grid.as_mut()) {
            let n: Vec<usize> = rect.split(' ').map(|n| n.parse().unwrap()).collect();
            grid.fill_run(n[0], n[1], n[2]);
        }
    }
    grid.unwrap_or_else(|| Grid::new(0))
}

// -------------------------------------------------------------- tests ----

const PAYLOADS: [&[u8]; 4] = [
    b"hello",
    b"https://example.com/a/longer/path?with=query&and=more#fragment",
    b"WIFI:T:WPA;S:home;P:pa\\;ss;;",
    &[0xff; 300],
];

#[test]
fn every_format_draws_exactly_the_module_frame_and_decodes() {
    for payload in PAYLOADS {
        let code = encode(payload, &EncodeOptions::default()).unwrap();
        for (margin, scale) in [(4, 10), (1, 1), (0, 3), (10, 2)] {
            let options = RenderOptions::default()
                .with_margin(margin)
                .with_scale(scale);
            let expected = Grid::expected(code.matrix(), margin);

            let svg = svg_grid(&render_svg(code.matrix(), &options).unwrap());
            let pdf = pdf_grid(&render_pdf(code.matrix(), &options).unwrap());
            let eps = eps_grid(&render_eps(code.matrix(), &options).unwrap());
            assert_eq!(svg, expected, "svg margin {margin} scale {scale}");
            assert_eq!(pdf, expected, "pdf margin {margin} scale {scale}");
            assert_eq!(eps, expected, "eps margin {margin} scale {scale}");
            if margin > 0 {
                assert_eq!(svg.decode(), payload);
            }
        }
    }
}

#[test]
fn largest_symbol_renders_in_every_format() {
    let data = vec![b'7'; 7_000];
    let code = encode(
        &data,
        &EncodeOptions::default().with_ec_level(qr_core::EcLevel::L),
    )
    .unwrap();
    assert_eq!(code.matrix().width(), 177);
    let options = RenderOptions::default();
    let expected = Grid::expected(code.matrix(), 4);
    let svg = render_svg(code.matrix(), &options).unwrap();
    assert_eq!(svg_grid(&svg), expected);
    assert_eq!(svg_grid(&svg).decode(), data);
    let pdf = render_pdf(code.matrix(), &options).unwrap();
    assert_eq!(pdf_grid(&pdf), expected);
    let eps = render_eps(code.matrix(), &options).unwrap();
    assert_eq!(eps_grid(&eps), expected);
    // Regression guards on output size (measured: svg 98,232, pdf 112,879
    // and eps 161,246 bytes): one drawing operation per run, not per module.
    assert!(svg.len() < 112 * 1024, "svg is {} bytes", svg.len());
    assert!(pdf.len() < 128 * 1024, "pdf is {} bytes", pdf.len());
    assert!(eps.len() < 176 * 1024, "eps is {} bytes", eps.len());
}

#[test]
fn render_vector_matches_the_typed_renderers() {
    let code = encode(b"dispatch", &EncodeOptions::default()).unwrap();
    let options = RenderOptions::default().with_title("t");
    for format in VectorFormat::ALL {
        let bytes = render_vector(code.matrix(), format, &options).unwrap();
        match format {
            VectorFormat::Svg => {
                svg_grid(std::str::from_utf8(&bytes).unwrap());
            }
            VectorFormat::Pdf => {
                pdf_grid(&bytes);
            }
            VectorFormat::Eps => {
                eps_grid(std::str::from_utf8(&bytes).unwrap());
            }
        }
    }
}

#[test]
fn hostile_titles_stay_inert_in_every_format() {
    let code = encode(b"title", &EncodeOptions::default()).unwrap();
    let hostile = [
        "\"><script>alert(1)</script>",
        "]]><foreignObject><a xlink:href='javascript:x'/></foreignObject>",
        "<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///etc/passwd'>]>&e;",
        ") /S /JavaScript /JS (app.alert(1)",
        "x)\nshowpage\nquit\n%%EOF\n(",
        "\u{0}\u{1b}[31m\u{7f}\u{202e}\u{FFFE}é😀",
    ];
    for title in hostile {
        let options = RenderOptions::default().with_title(title);
        let svg = render_svg(code.matrix(), &options).unwrap();
        let doc = assert_safe_svg(&svg);
        let title_node = doc.descendants().find(|n| n.has_tag_name("title")).unwrap();
        let expected: String = title
            .chars()
            .filter(|&c| !matches!(c, '\u{0}' | '\u{1b}' | '\u{FFFE}'))
            .collect();
        assert_eq!(title_node.text().unwrap_or(""), expected);
        assert_eq!(svg_grid(&svg).decode(), b"title");

        let pdf = render_pdf(code.matrix(), &options).unwrap();
        pdf_structure(&pdf);
        let text = String::from_utf8_lossy(&pdf);
        // Outside its strings, the info dictionary holds only its two keys.
        assert_eq!(
            pdf_info_skeleton(&text),
            "<< /Producer  /Title  >>",
            "{text}"
        );
        assert_eq!(text.matches(" 0 obj\n").count(), 5);

        let eps = render_eps(code.matrix(), &options).unwrap();
        eps_grid(&eps);
        assert_eq!(eps.lines().filter(|l| *l == "showpage").count(), 1);
        assert!(eps.lines().all(|l| l != "quit"));
        assert_eq!(eps.lines().filter(|l| *l == "%%EOF").count(), 1, "{eps}");
    }
}

/// The info dictionary (object 5) with every literal and hex string
/// removed, honouring backslash escapes in literal strings.
fn pdf_info_skeleton(pdf: &str) -> String {
    let (_, info) = pdf.split_once("5 0 obj\n").unwrap();
    let (info, _) = info.split_once("\nendobj\n").unwrap();
    let mut out = String::new();
    let mut chars = info.chars();
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                let mut depth = 1;
                while depth > 0 {
                    match chars.next().unwrap() {
                        '\\' => {
                            chars.next().unwrap();
                        }
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
            }
            '<' if chars.clone().next() == Some('<') => {
                chars.next();
                out.push_str("<<");
            }
            '<' => {
                // A hex string runs to the next '>'.
                for c in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn color() -> impl Strategy<Value = Color> {
    any::<[u8; 4]>().prop_map(|[r, g, b, a]| Color::rgba(r, g, b, a))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The fuzz-style SVG check from DECISIONS §4.4: arbitrary titles and
    /// colours always give a well-formed, script-free document.
    #[test]
    fn arbitrary_titles_and_colours_give_safe_svg(
        title in ".{0,64}",
        fg in color(),
        bg in color(),
        transparent: bool,
    ) {
        let code = encode(b"prop", &EncodeOptions::default()).unwrap();
        let options = RenderOptions::default()
            .with_title(title)
            .with_fg(fg)
            .with_bg(bg)
            .with_transparent(transparent)
            .with_scale(2);
        let svg = render_svg(code.matrix(), &options).unwrap();
        let doc = assert_safe_svg(&svg);
        for node in doc.descendants().filter(|n| n.has_attribute("fill")) {
            let fill = node.attribute("fill").unwrap();
            prop_assert!(fill.len() == 7 && fill.starts_with('#'), "{}", fill);
            prop_assert!(fill.parse::<Color>().is_ok());
        }
        let pdf = render_pdf(code.matrix(), &options).unwrap();
        pdf_structure(&pdf);
        let eps = render_eps(code.matrix(), &options).unwrap();
        prop_assert!(eps.is_ascii());
        prop_assert!(eps.lines().all(|l| l.len() <= 255));
    }

    #[test]
    fn arbitrary_payloads_round_trip_through_svg(data in prop::collection::vec(any::<u8>(), 1..200)) {
        let code = encode(&data, &EncodeOptions::default()).unwrap();
        let svg = render_svg(code.matrix(), &RenderOptions::default().with_scale(1)).unwrap();
        prop_assert_eq!(svg_grid(&svg).decode(), data);
    }
}
