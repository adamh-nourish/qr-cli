//! Image styling: the renderer options the flags select, and the colour
//! safety rails (UX proposal: warn when the contrast is below 3:1 or the
//! foreground is lighter than the background).

use qr_core::render::DEFAULT_SCALE;
use qr_core::{Color, MIN_CONTRAST, ModuleShape, RenderOptions};

use crate::cli::StyleArgs;
use crate::diag::Reporter;
use crate::output::Renderer;

/// The renderer options selected by the flags. `--invert` swaps the
/// colours, so it composes with `--fg` and `--bg`.
pub(crate) fn render_options(args: &StyleArgs) -> RenderOptions {
    let (mut fg, mut bg) = (
        args.fg.unwrap_or(Color::BLACK),
        args.bg.unwrap_or(Color::WHITE),
    );
    if args.invert {
        std::mem::swap(&mut fg, &mut bg);
    }
    RenderOptions::default()
        .with_margin(usize::from(args.margin))
        .with_scale(args.scale.unwrap_or(DEFAULT_SCALE))
        .with_size(args.size)
        .with_dpi(args.dpi)
        .with_fg(fg)
        .with_bg(bg)
        .with_transparent(args.transparent)
        .with_shape(args.shape.into())
}

/// Whether the image departs from plain black squares on white: a logo,
/// a module shape, a colour, transparency or `--invert`. Such images are
/// verified automatically (DECISIONS section 3).
pub(crate) fn is_styled(options: &RenderOptions) -> bool {
    options.logo.is_some()
        || options.shape != ModuleShape::Square
        || options.fg != Color::BLACK
        || options.bg != Color::WHITE
        || options.transparent
}

/// Warns when the colours, as `renderer` will draw them, are hard to scan:
/// a contrast ratio below [`MIN_CONTRAST`], or a foreground lighter than
/// the background.
pub(crate) fn warn_contrast(
    args: &StyleArgs,
    options: &RenderOptions,
    renderer: Renderer,
    reporter: &mut Reporter<'_>,
) {
    let contrast = as_drawn(options, renderer).contrast();
    let pair = format!("{} on {}", contrast.dark, contrast.light);
    if contrast.is_low() {
        reporter.warn(
            "low_contrast",
            &format!(
                "the colours have a contrast ratio of {:.1}:1 ({pair}), below the \
                 {MIN_CONTRAST:.0}:1 scanners need",
                // Shown rounded down, so 2.96 never reads as 3.0:1.
                (contrast.ratio * 10.0).floor() / 10.0
            ),
            Some("use a darker --fg or a lighter --bg"),
        );
    }
    if contrast.inverted {
        reporter.warn(
            "inverted_colors",
            &format!(
                "the modules are lighter than the background ({pair}); \
                 many scanners cannot read light-on-dark codes"
            ),
            Some(if args.invert {
                "drop --invert to draw dark modules on a light background"
            } else {
                "swap the --fg and --bg colours"
            }),
        );
    }
}

/// `options` as the renderer draws them: JPEG has no alpha channel, so its
/// colours are flattened onto white, transparent background included.
pub(crate) fn as_drawn(options: &RenderOptions, renderer: Renderer) -> RenderOptions {
    match renderer {
        Renderer::Raster(format) if !format.has_alpha() => options
            .clone()
            .with_bg(options.background().over(Color::WHITE))
            .with_transparent(false),
        Renderer::Raster(_) | Renderer::Vector(_) | Renderer::Text => options.clone(),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use qr_core::{RasterFormat, VectorFormat};

    use super::*;
    use crate::cli::Cli;
    use crate::diag::Verbosity;

    fn args(extra: &[&str]) -> StyleArgs {
        let mut argv = vec!["qr"];
        argv.extend_from_slice(extra);
        argv.push("x");
        Cli::try_parse_from(argv).unwrap().generate.render.style
    }

    fn warnings(extra: &[&str], renderer: Renderer) -> String {
        let args = args(extra);
        let mut buf = Vec::new();
        let mut reporter = Reporter::new(&mut buf, false, Verbosity::Normal);
        warn_contrast(&args, &render_options(&args), renderer, &mut reporter);
        String::from_utf8(buf).unwrap()
    }

    const PNG: Renderer = Renderer::Raster(RasterFormat::Png);

    #[test]
    fn options_follow_the_flags() {
        let o = render_options(&args(&[]));
        assert_eq!(o, RenderOptions::default());
        assert!(!is_styled(&o));

        let o = render_options(&args(&["--shape", "rounded", "--scale", "3"]));
        assert_eq!((o.shape, o.scale), (ModuleShape::Rounded, 3));
        assert!(is_styled(&o));

        let o = render_options(&args(&["--invert", "--fg", "navy"]));
        assert_eq!((o.fg, o.bg), (Color::WHITE, Color::rgb(0, 0, 0x80)));
        assert!(is_styled(&o));

        // Naming the default colours is not styling.
        let o = render_options(&args(&["--fg", "black", "--bg", "#fff"]));
        assert!(!is_styled(&o));
        assert!(is_styled(&render_options(&args(&["--transparent"]))));
        assert!(is_styled(&render_options(&args(&["--bg", "ivory"]))));
        let image = qr_core::loader::Bitmap::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
        let logo = qr_core::logo::Logo::new(image);
        assert!(is_styled(&RenderOptions::default().with_logo(Some(logo))));
    }

    #[test]
    fn default_and_good_colours_do_not_warn() {
        assert_eq!(warnings(&[], PNG), "");
        assert_eq!(warnings(&["--fg", "navy", "--bg", "ivory"], PNG), "");
        assert_eq!(warnings(&["--transparent"], PNG), "");
    }

    #[test]
    fn low_contrast_warns_with_the_ratio() {
        assert_eq!(
            warnings(&["--fg", "#aaa"], PNG),
            "warning: the colours have a contrast ratio of 2.3:1 (#aaaaaa on #ffffff), \
             below the 3:1 scanners need\n  hint: use a darker --fg or a lighter --bg\n"
        );
    }

    #[test]
    fn light_on_dark_warns_with_a_hint_for_the_flag_used() {
        let out = warnings(&["--invert"], Renderer::Vector(VectorFormat::Svg));
        assert_eq!(
            out,
            "warning: the modules are lighter than the background (#ffffff on #000000); \
             many scanners cannot read light-on-dark codes\n  \
             hint: drop --invert to draw dark modules on a light background\n"
        );
        let out = warnings(&["--fg", "white", "--bg", "#333"], PNG);
        assert!(
            out.contains("hint: swap the --fg and --bg colours"),
            "{out}"
        );
        // Both problems at once.
        let out = warnings(&["--fg", "#eee", "--bg", "#ddd"], PNG);
        assert!(
            out.contains("contrast ratio of 1.1:1") && out.contains("lighter"),
            "{out}"
        );
    }

    #[test]
    fn jpeg_is_judged_on_white() {
        // Gold on a transparent navy background is fine in a PNG, but a
        // JPEG flattens the background onto white.
        let flags = ["--fg", "gold", "--bg", "navy", "--transparent"];
        let png = warnings(&flags, PNG);
        assert!(png.contains("lighter than the background"), "{png}");
        assert!(!png.contains("contrast ratio"), "{png}");
        let jpeg = warnings(&flags, Renderer::Raster(RasterFormat::Jpeg));
        assert!(jpeg.contains("(#ffd700 on #ffffff)"), "{jpeg}");
        assert!(jpeg.contains("contrast ratio of 1.4:1"), "{jpeg}");
        assert_eq!(
            as_drawn(&render_options(&args(&[])), Renderer::Text),
            RenderOptions::default()
        );
    }
}
