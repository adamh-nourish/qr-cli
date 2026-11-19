//! Renderers that turn a [`Matrix`](crate::Matrix) into output.
//!
//! Every renderer draws from the same module grid and adds the quiet zone
//! itself. This module currently holds the text renderers (terminal,
//! Unicode and ASCII); vector and raster renderers are added alongside them.

mod text;

pub use self::text::{DEFAULT_MARGIN, MAX_MARGIN, TextFormat, TextOptions, render_text};
