//! Renderers that turn a [`Matrix`](crate::Matrix) into output.
//!
//! Every renderer draws from the same module grid and adds the quiet zone
//! itself. The text renderers (terminal, Unicode and ASCII) take
//! [`TextOptions`]; the vector renderers (SVG, PDF and EPS) and the raster
//! renderers (PNG, JPEG and WebP) share [`RenderOptions`].
//!
//! The vector writers never pass user strings through: colours are written
//! from the typed [`Color`](crate::Color), SVG is built by a typed XML writer
//! that escapes every value, and PDF and PostScript strings are escaped.

mod eps;
mod escape;
mod options;
mod outline;
mod pdf;
pub(crate) mod raster;
mod runs;
mod shape;
mod svg;
mod text;
mod vector;
mod xml;

pub use self::eps::render_eps;
pub use self::options::{
    DEFAULT_SCALE, MAX_CANVAS_SIDE, MAX_DPI, MAX_TITLE_CHARS, RenderError, RenderOptions,
};
pub use self::pdf::{PDF_MAX_PAGE_SIDE, render_pdf};
pub use self::raster::{
    JPEG_MIN_SCALE, JPEG_QUALITY, MAX_PIXELS, RasterFormat, UnknownRasterFormat, render_jpeg,
    render_png, render_raster, render_webp,
};
pub use self::shape::{DOT_DIAMETER, ModuleShape, UnknownModuleShape};
pub use self::svg::render_svg;
pub use self::text::{DEFAULT_MARGIN, MAX_MARGIN, TextFormat, TextOptions, render_text};
pub use self::vector::{UnknownVectorFormat, VectorFormat, render_vector};
