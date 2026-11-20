//! SVG renderer, built with the typed [`XmlWriter`].
//!
//! The document is a background `<rect>` and one `<path>` holding every dark
//! run (or, for rounded and dotted modules, every outline). Coordinates are
//! in modules (`viewBox`), and `width`/`height` scale the drawing to
//! `scale` pixels per module. The output never contains scripts, event
//! attributes, `<foreignObject>`, links or external references; colours are
//! written from the typed [`Color`](crate::Color).
//!
//! A logo is one `<image>` whose `href` is always a `data:image/png`
//! URI: the decoded logo is resampled and re-encoded as PNG here, never
//! copied from the input file, and the URI is built by a typed value
//! ([`PngDataUri`]) rather than from a string, so no external reference
//! can be written.

use std::fmt::{self, Write as _};

use super::data_uri::base64;
use super::escape;
use super::options::{MAX_CANVAS_SIDE, RenderError, RenderOptions};
use super::outline::{Num, Primitive, Segment, primitives};
use super::runs::dark_runs;
use super::shape::ModuleShape;
use super::xml::XmlWriter;
use crate::color::Color;
use crate::logo::{Planned, resample};
use crate::matrix::Matrix;

/// The SVG namespace. It is an identifier, not a resource that is fetched.
const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// Renders `matrix` as an SVG document.
///
/// # Errors
///
/// Returns [`RenderError`] if the scale is zero, the drawing would exceed
/// [`MAX_CANVAS_SIDE`] pixels, or the title is too long.
///
/// ```
/// use qr_core::{EncodeOptions, RenderOptions, encode, render_svg};
/// let code = encode(b"hello", &EncodeOptions::default())?;
/// let svg = render_svg(code.matrix(), &RenderOptions::default())?;
/// assert!(svg.contains(r#"viewBox="0 0 29 29""#));
/// assert!(svg.contains(r#"width="290""#));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn render_svg(matrix: &Matrix, options: &RenderOptions) -> Result<String, RenderError> {
    let layout = options.layout(matrix, "svg", MAX_CANVAS_SIDE)?;
    let (modules, margin) = (layout.modules, layout.margin);
    let plan = Planned::new(matrix, options)?;
    // With a logo, the modules under it are cleared.
    let matrix = plan.as_ref().map_or(matrix, |p| &p.matrix);

    let mut w = XmlWriter::new();
    w.start("svg")
        .attr("xmlns", SVG_NS)
        .attr("version", "1.1")
        .attr("width", layout.side)
        .attr("height", layout.side)
        .attr("viewBox", format_args!("0 0 {modules} {modules}"));
    // Square modules sit on whole units, so crisp edges avoid seams; curves
    // need anti-aliasing.
    if options.shape == ModuleShape::Square {
        w.attr("shape-rendering", "crispEdges");
    }
    w.newline();

    if let Some(title) = &options.title {
        w.start("title").text(title).end().newline();
    }

    let background = options.background();
    if !background.is_invisible() {
        w.start("rect")
            .attr("width", modules)
            .attr("height", modules);
        fill(&mut w, background);
        w.end().newline();
    }

    let d = match options.shape {
        ModuleShape::Square => path_data(matrix, margin),
        shape => shaped_path_data(matrix, shape, margin),
    };
    if !d.is_empty() && !options.fg.is_invisible() {
        w.start("path");
        fill(&mut w, options.fg);
        w.attr("d", d).end().newline();
    }
    if let Some(plan) = &plan {
        logo(&mut w, plan, margin, layout.scale)?;
    }
    Ok(w.finish())
}

/// How many image pixels the embedded logo has per pixel it is shown at,
/// so it stays sharp on high-density screens and when the SVG is enlarged.
const LOGO_DENSITY: f64 = 2.0;

/// Writes the logo `<image>`: the logo resampled to [`LOGO_DENSITY`]
/// times its displayed size (never above its own size) and embedded as a
/// PNG.
fn logo(w: &mut XmlWriter, plan: &Planned<'_>, margin: u32, scale: u32) -> Result<(), RenderError> {
    let (x, y, width, height) = plan.geometry.modules(margin);
    let image = plan.logo.image();
    let pixels =
        |modules: f64, max: u32| whole_pixels(modules * f64::from(scale) * LOGO_DENSITY, max);
    let (px_w, px_h) = (pixels(width, image.width()), pixels(height, image.height()));
    let colors = resample(image, px_w, px_h).ok_or_else(|| RenderError::Encoder {
        format: "svg",
        message: format!("cannot allocate the {px_w}x{px_h} logo"),
    })?;
    let png = encode_png(&colors, px_w, px_h)?;
    w.start("image")
        .attr("x", Num(x))
        .attr("y", Num(y))
        .attr("width", Num(width))
        .attr("height", Num(height))
        .attr("preserveAspectRatio", "none")
        .attr("href", PngDataUri(&png))
        .end()
        .newline();
    Ok(())
}

/// `value` rounded to whole pixels, from 1 to `max`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole_pixels(value: f64, max: u32) -> u32 {
    // Clamped to 1..=max first, so the cast is exact.
    value.round().clamp(1.0, f64::from(max.max(1))) as u32
}

/// A `data:image/png;base64,` URI. The scheme and media type are fixed,
/// so an `href` written from this value can only ever be embedded data.
struct PngDataUri<'a>(&'a [u8]);

impl fmt::Display for PngDataUri<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "data:image/png;base64,{}", base64(self.0))
    }
}

/// `colors` (straight alpha, row-major) as an 8-bit RGB PNG, or RGBA when
/// any pixel is translucent.
fn encode_png(colors: &[Color], width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let error = |e: png::EncodingError| RenderError::Encoder {
        format: "svg",
        message: format!("cannot encode the logo: {e}"),
    };
    let opaque = colors.iter().all(|c| c.is_opaque());
    let mut data = Vec::new();
    for c in colors {
        data.extend_from_slice(&[c.r, c.g, c.b, c.a][..if opaque { 3 } else { 4 }]);
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(if opaque {
        png::ColorType::Rgb
    } else {
        png::ColorType::Rgba
    });
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(error)?;
    writer.write_image_data(&data).map_err(error)?;
    writer.finish().map_err(error)?;
    Ok(out)
}

/// The path data for every dark run, offset by `margin` modules.
///
/// Each run is a closed rectangle `h{len}v1h-{len}z`. Closing a subpath
/// returns the current point to its start, so each run after the first is
/// reached with a relative move from the previous run's start, which keeps
/// the numbers short.
fn path_data(matrix: &Matrix, margin: u32) -> String {
    let mut d = String::new();
    let mut previous: Option<(i64, i64)> = None;
    for run in dark_runs(matrix) {
        let (x, y) = (i64::from(run.x + margin), i64::from(run.y + margin));
        let len = run.len;
        let _ = match previous {
            None => write!(d, "M{x} {y}"),
            Some((px, py)) => write!(d, "m{} {}", x - px, y - py),
        };
        let _ = write!(d, "h{len}v1h-{len}z");
        previous = Some((x, y));
    }
    d
}

/// The path data for `shape`, all in absolute coordinates: each square run
/// as `M{x} {y}h{len}v1h-{len}z` and each outline as `M`, `L`, `C` and `Z`
/// commands.
fn shaped_path_data(matrix: &Matrix, shape: ModuleShape, margin: u32) -> String {
    let mut d = String::new();
    for primitive in primitives(matrix, shape, margin) {
        match primitive {
            Primitive::Rect(run) => {
                let _ = write!(d, "M{} {}h{len}v1h-{len}z", run.x, run.y, len = run.len);
            }
            Primitive::Shape(kind) => kind.outline(|segment| {
                let _ = match segment {
                    Segment::Move(p) => write!(d, "M{} {}", Num(p.x), Num(p.y)),
                    Segment::Line(p) => write!(d, "L{} {}", Num(p.x), Num(p.y)),
                    Segment::Cubic(a, b, p) => write!(
                        d,
                        "C{} {} {} {} {} {}",
                        Num(a.x),
                        Num(a.y),
                        Num(b.x),
                        Num(b.y),
                        Num(p.x),
                        Num(p.y)
                    ),
                    Segment::Close => write!(d, "Z"),
                };
            }),
        }
    }
    d
}

/// Writes `fill` (and `fill-opacity` for a translucent colour) for `color`.
fn fill(w: &mut XmlWriter, color: Color) {
    w.attr("fill", color.with_alpha(u8::MAX));
    if !color.is_opaque() {
        w.attr("fill-opacity", escape::unit(color.a));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_by_two() -> Matrix {
        Matrix::from_modules(2, vec![true, true, false, true]).unwrap()
    }

    #[test]
    fn exact_output_for_a_tiny_matrix() {
        let options = RenderOptions::default().with_margin(1).with_scale(5);
        let svg = render_svg(&two_by_two(), &options).unwrap();
        assert_eq!(
            svg,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"20\" \
             height=\"20\" viewBox=\"0 0 4 4\" shape-rendering=\"crispEdges\">\n\
             <rect width=\"4\" height=\"4\" fill=\"#ffffff\"/>\n\
             <path fill=\"#000000\" d=\"M1 1h2v1h-2zm1 1h1v1h-1z\"/>\n\
             </svg>\n"
        );
    }

    #[test]
    fn shapes_draw_outlines_without_crisp_edges() {
        // A lone module in a 3x3 grid with a margin of 1.
        let m = Matrix::from_modules(
            3,
            vec![false, false, false, false, true, false, false, false, false],
        )
        .unwrap();
        let options = RenderOptions::default()
            .with_margin(1)
            .with_scale(2)
            .with_shape(ModuleShape::Dots);
        let svg = render_svg(&m, &options).unwrap();
        assert!(!svg.contains("crispEdges"), "{svg}");
        assert!(
            svg.contains(
                "d=\"M2.95 2.5C2.95 2.7485 2.7485 2.95 2.5 2.95C2.2515 2.95 2.05 2.7485 \
                 2.05 2.5C2.05 2.2515 2.2515 2.05 2.5 2.05C2.7485 2.05 2.95 2.2515 2.95 2.5Z\""
            ),
            "{svg}"
        );
        let rounded = render_svg(&m, &options.clone().with_shape(ModuleShape::Rounded)).unwrap();
        assert!(rounded.contains("d=\"M2.5 2L2.5 2C"), "{rounded}");
        assert!(rounded.ends_with("Z\"/>\n</svg>\n"), "{rounded}");

        // Runs that touch nothing above or below stay plain rectangles.
        let full = Matrix::from_modules(3, vec![true; 9]).unwrap();
        let rounded = render_svg(&full, &options.with_shape(ModuleShape::Rounded)).unwrap();
        assert!(rounded.contains("M1 2h3v1h-3z"), "{rounded}");
    }

    #[test]
    fn translucent_colours_use_fill_opacity() {
        let options = RenderOptions::default()
            .with_fg(Color::rgba(0x1a, 0x73, 0xe8, 0x80))
            .with_bg(Color::rgba(255, 255, 0, 51));
        let svg = render_svg(&two_by_two(), &options).unwrap();
        assert!(
            svg.contains(r##"fill="#ffff00" fill-opacity="0.2""##),
            "{svg}"
        );
        assert!(
            svg.contains(r##"fill="#1a73e8" fill-opacity="0.502""##),
            "{svg}"
        );
    }

    #[test]
    fn invisible_layers_are_omitted() {
        let transparent = RenderOptions::default().with_transparent(true);
        let svg = render_svg(&two_by_two(), &transparent).unwrap();
        assert!(!svg.contains("<rect"));
        assert!(svg.contains("<path"));

        let no_fg = RenderOptions::default().with_fg(Color::TRANSPARENT);
        let svg = render_svg(&two_by_two(), &no_fg).unwrap();
        assert!(svg.contains("<rect") && !svg.contains("<path"));

        let blank = Matrix::from_modules(1, vec![false]).unwrap();
        let svg = render_svg(&blank, &RenderOptions::default()).unwrap();
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn title_is_escaped() {
        let options = RenderOptions::default().with_title("\"><script>alert(1)</script>&\u{0}");
        let svg = render_svg(&two_by_two(), &options).unwrap();
        assert!(
            svg.contains("<title>&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;&amp;</title>")
        );
    }

    #[test]
    fn errors_come_from_the_layout() {
        let m = two_by_two();
        let zero = RenderOptions::default().with_scale(0);
        assert_eq!(render_svg(&m, &zero), Err(RenderError::ZeroScale));
        let huge = RenderOptions::default().with_scale(MAX_CANVAS_SIDE);
        assert!(matches!(
            render_svg(&m, &huge),
            Err(RenderError::TooLarge { format: "svg", .. })
        ));
    }
}
