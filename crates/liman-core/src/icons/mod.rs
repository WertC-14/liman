//! Icons for the grid view (ADR 0002, 0005): find the icon file in the user's icon theme,
//! rasterize the SVG to a small RGBA square. Turning pixels into terminal cells is the UI's job.
//!
//! Blocking (file system, SVG parsing): call from a worker thread.

mod embedded;
mod names;
mod raster;
mod theme;

pub use embedded::embedded_svg;
pub use names::icon_names;
pub use raster::{IconPixels, render_svg};
pub use theme::{IconTheme, detect_theme_name};

use crate::Entry;

/// Icon side length in pixels. One halfblock cell holds 1×2 pixels, so 16×16 pixels = 16×8 cells.
pub const ICON_SIZE: u32 = 16;

/// Cache key: entries with the same key get the same icon (same candidate names, same fallback).
pub fn icon_key(entry: &Entry) -> String {
    format!(
        "{}|{:?}|{:?}",
        icon_names(entry).join(","),
        entry.special,
        entry.file_type
    )
}

/// The icon for `entry`: from the theme if it has one, else from liman's own set.
/// Never fails; a broken theme file falls back to the embedded icon.
pub fn load_icon(entry: &Entry, theme: &IconTheme) -> IconPixels {
    theme
        .find(&icon_names(entry))
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|svg| render_svg(&svg, ICON_SIZE))
        .or_else(|| render_svg(embedded_svg(entry), ICON_SIZE))
        .expect("embedded icons are valid SVG")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileType, SpecialDir};
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, special: Option<SpecialDir>) -> Entry {
        let path = PathBuf::from(name);
        Entry {
            name: name.into(),
            file_type: FileType::from_path(&path, is_dir),
            path,
            is_dir,
            is_symlink: false,
            special,
            size: 0,
            item_count: None,
            modified: None,
        }
    }

    #[test]
    fn every_embedded_icon_renders() {
        let mut entries = vec![entry("x", true, None)];
        for special in [
            SpecialDir::Home,
            SpecialDir::Desktop,
            SpecialDir::Documents,
            SpecialDir::Downloads,
            SpecialDir::Music,
            SpecialDir::Pictures,
            SpecialDir::Videos,
            SpecialDir::Trash,
        ] {
            entries.push(entry("x", true, Some(special)));
        }
        for name in [
            "a.rs", "a.toml", "a.md", "a.pdf", "a.docx", "a.xls", "a.pptx", "a.zip", "a.png",
            "a.mp4", "a.mp3", "a",
        ] {
            entries.push(entry(name, false, None));
        }
        for e in &entries {
            let px = render_svg(embedded_svg(e), ICON_SIZE).unwrap_or_else(|| panic!("{}", e.name));
            // Something visible in the middle of every icon.
            assert!(px.get(8, 10)[3] > 0, "{} is empty in the middle", e.name);
        }
    }

    #[test]
    fn falls_back_to_embedded_without_a_theme() {
        let px = load_icon(&entry("Notes.pdf", false, None), &IconTheme::empty());
        assert_eq!(px.size, ICON_SIZE);
        let [r, g, b, _] = px.get(4, 14); // plain page area, away from the white glyph
        assert!(
            r > 150 && g < 120 && b < 120,
            "pdf icon is red, got {r},{g},{b}"
        );
    }

    #[test]
    fn same_kind_same_key() {
        assert_eq!(
            icon_key(&entry("a.pdf", false, None)),
            icon_key(&entry("b.pdf", false, None))
        );
        assert_ne!(
            icon_key(&entry("a.pdf", false, None)),
            icon_key(&entry("a.rs", false, None))
        );
        assert_ne!(
            icon_key(&entry("x", true, None)),
            icon_key(&entry("x", true, Some(SpecialDir::Music)))
        );
    }
}
