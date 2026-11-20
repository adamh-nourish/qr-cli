//! PDF renderer: a hand-written single-page PDF 1.4 file with vector paths.
//!
//! The page is `side × side` points. The content stream paints the
//! background, then sets a transform to module units with the origin at the
//! top-left (matching the [`Matrix`] coordinates) and fills one rectangle per
//! dark run, or the outline of each rounded run or dot (cubic Béziers). Translucent colours use an `ExtGState` with a fill alpha (`/ca`).
//! The stream is uncompressed, so the file has no dependencies and is easy to
//! inspect. The only string written is the escaped `/Title`.

use std::fmt::Write as _;

use super::escape;
use super::options::{MAX_CANVAS_SIDE, RenderError, RenderOptions};
use super::outline::{Num, Primitive, Segment, primitives};
use crate::color::Color;
use crate::matrix::Matrix;

/// The largest page side, in points, that PDF viewers are required to
/// support (the PDF 1.4 implementation limit is 14,400 units, 200 inches).
pub const PDF_MAX_PAGE_SIDE: u32 = 14_400;

/// Renders `matrix` as a one-page PDF document, `scale` points per module.
///
/// # Errors
///
/// Returns [`RenderError`] if the scale is zero, the page would exceed
/// [`PDF_MAX_PAGE_SIDE`] points, or the title is too long, and
/// [`RenderError::LogoUnsupported`] if the options carry a logo.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_pdf};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let pdf = render_pdf(code.matrix(), &RenderOptions::default())?;
/// assert!(pdf.starts_with(b"%PDF-1.4\n"));
/// assert!(pdf.ends_with(b"%%EOF\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_pdf(matrix: &Matrix, options: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    if options.logo.is_some() {
        return Err(RenderError::LogoUnsupported { format: "pdf" });
    }
    let layout = options.layout(matrix, "pdf", PDF_MAX_PAGE_SIDE.min(MAX_CANVAS_SIDE))?;
    let side = layout.side;
    let background = options.background();
    let foreground = options.fg;

    let mut ext_g_state = String::new();
    let mut content = String::from("q\n");
    if !background.is_invisible() {
        paint(&mut content, &mut ext_g_state, "Gb", background);
        let _ = writeln!(content, "0 0 {side} {side} re f");
    }
    let mut shapes = primitives(matrix, options.shape, layout.margin).peekable();
    if !foreground.is_invisible() && shapes.peek().is_some() {
        paint(&mut content, &mut ext_g_state, "Gf", foreground);
        let s = layout.scale;
        let _ = writeln!(content, "{s} 0 0 -{s} 0 {side} cm");
        for primitive in shapes {
            draw(&mut content, primitive);
        }
        content.push_str("f\n");
    }
    content.push_str("Q\n");

    let resources = if ext_g_state.is_empty() {
        String::from("<< >>")
    } else {
        format!("<< /ExtGState <<{ext_g_state} >> >>")
    };
    let mut info = format!(
        "<< /Producer {}",
        escape::pdf_text(&format!("qr-core {}", crate::VERSION))
    );
    if let Some(title) = &options.title {
        let _ = write!(info, " /Title {}", escape::pdf_text(title));
    }
    info.push_str(" >>");

    let objects = [
        String::from("<< /Type /Catalog /Pages 2 0 R >>"),
        String::from("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {side} {side}] \
             /Resources {resources} /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        info,
    ];
    Ok(assemble(&objects))
}

/// Appends `primitive` to the current path, in module units.
fn draw(content: &mut String, primitive: Primitive) {
    match primitive {
        Primitive::Rect(run) => {
            let _ = writeln!(content, "{} {} {} 1 re", run.x, run.y, run.len);
        }
        Primitive::Shape(kind) => kind.outline(|segment| {
            let _ = match segment {
                Segment::Move(p) => writeln!(content, "{} {} m", Num(p.x), Num(p.y)),
                Segment::Line(p) => writeln!(content, "{} {} l", Num(p.x), Num(p.y)),
                Segment::Cubic(a, b, p) => writeln!(
                    content,
                    "{} {} {} {} {} {} c",
                    Num(a.x),
                    Num(a.y),
                    Num(b.x),
                    Num(b.y),
                    Num(p.x),
                    Num(p.y)
                ),
                Segment::Close => writeln!(content, "h"),
            };
        }),
    }
}

/// Sets the fill colour (and, for a translucent colour, a graphics state
/// named `name` with that fill alpha).
fn paint(content: &mut String, ext_g_state: &mut String, name: &str, color: Color) {
    if !color.is_opaque() {
        let _ = write!(ext_g_state, " /{name} << /ca {} >>", escape::unit(color.a));
        let _ = writeln!(content, "/{name} gs");
    }
    let _ = writeln!(
        content,
        "{} {} {} rg",
        escape::unit(color.r),
        escape::unit(color.g),
        escape::unit(color.b)
    );
}

/// Lays out numbered objects (1, 2, …) with a cross-reference table and a
/// trailer whose `/Root` is object 1 and `/Info` the last object.
fn assemble(objects: &[String]) -> Vec<u8> {
    // The second line holds bytes above 127 so tools treat the file as binary.
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let size = objects.len() + 1;
    let mut tail = format!("xref\n0 {size}\n0000000000 65535 f \n");
    for offset in offsets {
        // Each entry is exactly 20 bytes, including the two-byte end of line.
        let _ = writeln!(tail, "{offset:010} 00000 n ");
    }
    let _ = write!(
        tail,
        "trailer\n<< /Size {size} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len()
    );
    out.extend_from_slice(tail.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).into_owned()
    }

    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    #[test]
    fn content_stream_for_a_tiny_matrix() {
        let options = RenderOptions::default().with_margin(1).with_scale(5);
        let pdf = text(&render_pdf(&two_by_two(), &options).unwrap());
        let expected = "stream\nq\n1 1 1 rg\n0 0 20 20 re f\n0 0 0 rg\n5 0 0 -5 0 20 cm\n\
                        1 1 2 1 re\n2 2 1 1 re\nf\nQ\nendstream";
        assert!(pdf.contains(expected), "{pdf}");
        assert!(pdf.contains("/MediaBox [0 0 20 20]"));
        assert!(pdf.contains("/Resources << >>"));
        assert!(pdf.contains("/Producer (qr-core "));
        assert!(!pdf.contains("/Title"));
    }

    #[test]
    fn shapes_are_drawn_as_curves() {
        use crate::render::shape::ModuleShape;
        let lone = Matrix::from_modules(1, vec![true]).unwrap();
        let options = RenderOptions::default()
            .with_margin(1)
            .with_scale(5)
            .with_shape(ModuleShape::Dots);
        let pdf = text(&render_pdf(&lone, &options).unwrap());
        assert!(
            pdf.contains("5 0 0 -5 0 15 cm\n1.95 1.5 m\n1.95 1.7485 1.7485 1.95 1.5 1.95 c\n"),
            "{pdf}"
        );
        assert!(pdf.contains("1.95 1.5 c\nh\nf\nQ\n"), "{pdf}");
        let rounded = text(&render_pdf(&lone, &options.with_shape(ModuleShape::Rounded)).unwrap());
        assert!(rounded.contains("1.5 1 m\n1.5 1 l\n"), "{rounded}");
        assert_eq!(rounded.matches(" c\n").count(), 4, "{rounded}");
    }

    #[test]
    fn translucent_colours_use_ext_g_state() {
        let options = RenderOptions::default()
            .with_fg(Color::rgba(255, 0, 0, 128))
            .with_bg(Color::rgba(0, 0, 255, 51));
        let pdf = text(&render_pdf(&two_by_two(), &options).unwrap());
        assert!(pdf.contains("/ExtGState << /Gb << /ca 0.2 >> /Gf << /ca 0.502 >> >>"));
        assert!(pdf.contains("/Gb gs\n0 0 1 rg\n"));
        assert!(pdf.contains("/Gf gs\n1 0 0 rg\n"));
    }

    #[test]
    fn invisible_layers_are_skipped() {
        let transparent = RenderOptions::default().with_transparent(true);
        let pdf = text(&render_pdf(&two_by_two(), &transparent).unwrap());
        assert!(!pdf.contains("1 1 1 rg") && pdf.contains("0 0 0 rg"));

        let blank = Matrix::from_modules(1, vec![false]).unwrap();
        let pdf = text(&render_pdf(&blank, &RenderOptions::default()).unwrap());
        assert!(!pdf.contains(" cm\n"));
    }

    #[test]
    fn title_is_escaped() {
        let options = RenderOptions::default().with_title(") /JS (app.alert(1)");
        let pdf = text(&render_pdf(&two_by_two(), &options).unwrap());
        assert!(
            pdf.contains(r"/Title (\) /JS \(app.alert\(1\)) >>"),
            "{pdf}"
        );
        let options = RenderOptions::default().with_title("café\n");
        let pdf = text(&render_pdf(&two_by_two(), &options).unwrap());
        assert!(pdf.contains("/Title <FEFF00630061006600E9000A>"), "{pdf}");
    }

    #[test]
    fn page_size_is_bounded() {
        let m = two_by_two();
        let options = RenderOptions::default().with_margin(0).with_scale(7_200);
        assert!(render_pdf(&m, &options).is_ok());
        let options = options.with_scale(7_201);
        assert_eq!(
            render_pdf(&m, &options),
            Err(RenderError::TooLarge {
                format: "pdf",
                side: 14_402,
                max: PDF_MAX_PAGE_SIDE
            })
        );
    }
}
