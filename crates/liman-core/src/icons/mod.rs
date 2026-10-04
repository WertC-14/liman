//! Icons for the grid view (ADR 0002, 0005): find the icon file in the user's icon theme,
//! rasterize the SVG to a small RGBA square. Turning pixels into terminal cells is the UI's job.
//!
//! Blocking (file system, SVG parsing): call from a worker thread.

mod names;
mod raster;
mod theme;

pub use names::icon_names;
pub use raster::{IconPixels, render_svg};
pub use theme::{IconTheme, detect_theme_name};

/// Icon side length in pixels. One halfblock cell holds 1×2 pixels, so 16×16 pixels = 16×8 cells.
pub const ICON_SIZE: u32 = 16;
