//! EPS renderer: hand-written Encapsulated PostScript (EPSF-3.0, language
//! level 2) with one `rectfill` per dark run. Rounded runs and dots are
//! outlines of lines and cubic Béziers, filled together at the end.
//!
//! PostScript has no transparency, so translucent colours are blended over
//! what lies beneath them (the background, or white paper where the
//! background is not painted) and fully transparent layers are skipped. The
//! only string written is the escaped `%%Title`, which is kept to one DSC
//! line of at most 255 bytes.

use std::fmt::Write as _;

use super::escape;
use super::options::{MAX_CANVAS_SIDE, RenderError, RenderOptions};
use super::outline::{Num, Primitive, Segment, primitives};
use crate::color::Color;
use crate::matrix::Matrix;

/// The longest line the Document Structuring Conventions allow.
const DSC_MAX_LINE: usize = 255;

/// Renders `matrix` as an EPS document, `scale` points per module.
///
/// # Errors
///
/// Returns [`RenderError`] if the scale is zero, the bounding box would
/// exceed [`MAX_CANVAS_SIDE`] points, or the title is too long, and
/// [`RenderError::LogoUnsupported`] if the options carry a logo.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_eps};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let eps = render_eps(code.matrix(), &RenderOptions::default())?;
/// assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0\n"));
/// assert!(eps.contains("%%BoundingBox: 0 0 290 290\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_eps(matrix: &Matrix, options: &RenderOptions) -> Result<String, RenderError> {
    if options.logo.is_some() {
        return Err(RenderError::LogoUnsupported { format: "eps" });
    }
    let layout = options.layout(matrix, "eps", MAX_CANVAS_SIDE)?;
    let side = layout.side;

    let mut out = String::from("%!PS-Adobe-3.0 EPSF-3.0\n");
    let _ = writeln!(out, "%%BoundingBox: 0 0 {side} {side}");
    let _ = writeln!(out, "%%HiResBoundingBox: 0 0 {side} {side}");
    let creator = format!("qr-core {}", crate::VERSION);
    dsc_string(&mut out, "%%Creator: ", &creator);
    if let Some(title) = &options.title {
        dsc_string(&mut out, "%%Title: ", title);
    }
    out.push_str("%%LanguageLevel: 2\n%%Pages: 1\n%%EndComments\n%%Page: 1 1\ngsave\n");

    let background = options.background();
    let paper = if background.is_invisible() {
        Color::WHITE
    } else {
        let painted = background.over(Color::WHITE);
        set_rgb(&mut out, painted);
        let _ = writeln!(out, "0 0 {side} {side} rectfill");
        painted
    };

    let mut shapes = primitives(matrix, options.shape, layout.margin).peekable();
    if !options.fg.is_invisible() && shapes.peek().is_some() {
        set_rgb(&mut out, options.fg.over(paper));
        let s = layout.scale;
        let _ = writeln!(out, "[{s} 0 0 -{s} 0 {side}] concat");
        let mut outlines = false;
        for primitive in shapes {
            outlines |= draw(&mut out, primitive);
        }
        if outlines {
            out.push_str("fill\n");
        }
    }
    out.push_str("grestore\nshowpage\n%%Trailer\n%%EOF\n");
    Ok(out)
}

/// Fills a square run at once, or appends an outline to the current path
/// and returns true.
fn draw(out: &mut String, primitive: Primitive) -> bool {
    match primitive {
        Primitive::Rect(run) => {
            // `rectfill` leaves the current path alone.
            let _ = writeln!(out, "{} {} {} 1 rectfill", run.x, run.y, run.len);
            false
        }
        Primitive::Shape(kind) => {
            kind.outline(|segment| {
                let _ = match segment {
                    Segment::Move(p) => writeln!(out, "{} {} moveto", Num(p.x), Num(p.y)),
                    Segment::Line(p) => writeln!(out, "{} {} lineto", Num(p.x), Num(p.y)),
                    Segment::Cubic(a, b, p) => writeln!(
                        out,
                        "{} {} {} {} {} {} curveto",
                        Num(a.x),
                        Num(a.y),
                        Num(b.x),
                        Num(b.y),
                        Num(p.x),
                        Num(p.y)
                    ),
                    Segment::Close => writeln!(out, "closepath"),
                };
            });
            true
        }
    }
}

/// Writes a DSC comment whose value is an escaped PostScript string, cut so
/// the whole line fits in [`DSC_MAX_LINE`] bytes.
fn dsc_string(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(&escape::ps_string(
        value,
        DSC_MAX_LINE.saturating_sub(key.len()),
    ));
    out.push('\n');
}

/// Sets an opaque fill colour.
fn set_rgb(out: &mut String, color: Color) {
    let _ = writeln!(
        out,
        "{} {} {} setrgbcolor",
        escape::unit(color.r),
        escape::unit(color.g),
        escape::unit(color.b)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::options::MAX_TITLE_CHARS;

    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    #[test]
    fn exact_body_for_a_tiny_matrix() {
        let options = RenderOptions::default().with_margin(1).with_scale(5);
        let eps = render_eps(&two_by_two(), &options).unwrap();
        let expected_body = "%%Page: 1 1\ngsave\n1 1 1 setrgbcolor\n0 0 20 20 rectfill\n\
                             0 0 0 setrgbcolor\n[5 0 0 -5 0 20] concat\n1 1 2 1 rectfill\n\
                             2 2 1 1 rectfill\ngrestore\nshowpage\n%%Trailer\n%%EOF\n";
        assert!(eps.ends_with(expected_body), "{eps}");
        assert!(eps.contains("%%BoundingBox: 0 0 20 20\n"));
        assert!(eps.contains("%%Creator: (qr-core "));
        assert!(!eps.contains("%%Title"));
    }

    #[test]
    fn shapes_are_filled_outlines() {
        use crate::render::shape::ModuleShape;
        let options = RenderOptions::default()
            .with_margin(1)
            .with_scale(5)
            .with_shape(ModuleShape::Dots);
        let eps = render_eps(&two_by_two(), &options).unwrap();
        assert!(
            eps.contains("[5 0 0 -5 0 20] concat\n1.95 1.5 moveto\n"),
            "{eps}"
        );
        assert_eq!(eps.matches("closepath\n").count(), 3, "{eps}");
        assert!(eps.contains("closepath\nfill\ngrestore\n"), "{eps}");
        let rounded = render_eps(&two_by_two(), &options.with_shape(ModuleShape::Rounded)).unwrap();
        assert!(rounded.contains(" curveto\n"), "{rounded}");
        assert!(
            rounded
                .lines()
                .all(|l| l.len() <= DSC_MAX_LINE && l.is_ascii())
        );
    }

    #[test]
    fn translucent_colours_are_blended() {
        let options = RenderOptions::default()
            .with_fg(Color::rgba(0, 0, 0, 51))
            .with_bg(Color::rgba(255, 0, 0, 255));
        let eps = render_eps(&two_by_two(), &options).unwrap();
        // 20% black over red is (204, 0, 0).
        assert!(eps.contains("0.8 0 0 setrgbcolor\n"), "{eps}");

        let transparent = RenderOptions::default()
            .with_transparent(true)
            .with_fg(Color::rgba(0, 0, 0, 51));
        let eps = render_eps(&two_by_two(), &transparent).unwrap();
        assert!(!eps.contains("0 0 100 100 rectfill"), "{eps}");
        assert!(eps.contains("0.8 0.8 0.8 setrgbcolor\n"), "{eps}");

        let invisible_fg = RenderOptions::default().with_fg(Color::TRANSPARENT);
        let eps = render_eps(&two_by_two(), &invisible_fg).unwrap();
        assert!(!eps.contains("concat"));
    }

    #[test]
    fn title_stays_on_one_escaped_line() {
        let options = RenderOptions::default().with_title("x)\nshowpage\n%%EOF (");
        let eps = render_eps(&two_by_two(), &options).unwrap();
        assert!(
            eps.contains("%%Title: (x\\)\\012showpage\\012%%EOF \\()\n"),
            "{eps}"
        );
        assert_eq!(eps.matches("showpage\n").count(), 1);

        let options = RenderOptions::default().with_title("é".repeat(MAX_TITLE_CHARS));
        let eps = render_eps(&two_by_two(), &options).unwrap();
        let line = eps.lines().find(|l| l.starts_with("%%Title: ")).unwrap();
        assert!(line.len() <= DSC_MAX_LINE, "{}", line.len());
        assert!(line.ends_with(')'));
        assert!(eps.lines().all(|l| l.len() <= DSC_MAX_LINE && l.is_ascii()));
    }
}
