//! Type-colored box for the normal view (ADR 0003): a rounded frame with the label in the middle,
//! folders get a small tab on top. Ported from the fm-research spike `kutu.py`.
//!
//! ```text
//!  ╭──╮              ╭────────╮
//!  │  ╰─────╮        │        │
//!  │   ↓    │        │  PDF   │
//!  ╰────────╯        ╰────────╯
//! ```

use liman_core::Entry;
use ratatui::style::Style;
use ratatui::text::{Line, Text};

use crate::badge;
use crate::theme::type_color;

pub const WIDTH: u16 = 10;
pub const HEIGHT: u16 = 4;

/// The box at the list size (`WIDTH` × `HEIGHT`).
pub fn render(entry: &Entry) -> Text<'static> {
    render_sized(entry, WIDTH, HEIGHT)
}

/// A hollow box `width` × `height` cells (at least 6 × 4): folders get a tab on top,
/// the label (symbol or extension) sits on the middle line, everything in the type color.
pub fn render_sized(entry: &Entry, width: u16, height: u16) -> Text<'static> {
    let (width, height) = (usize::from(width.max(6)), usize::from(height.max(4)));
    let inner = width - 2;
    let label = if entry.is_dir && entry.special.is_none() {
        String::new() // plain folders: the shape says enough
    } else {
        badge::label(entry)
    };
    let empty = format!("│{}│", " ".repeat(inner));
    let mut lines = Vec::with_capacity(height);
    let body = if entry.is_dir {
        let tab = (width / 3).max(3);
        lines.push(format!(
            "╭{}╮{}",
            "─".repeat(tab - 2),
            " ".repeat(width - tab)
        ));
        lines.push(format!(
            "│{}╰{}╮",
            " ".repeat(tab - 2),
            "─".repeat(width - tab - 1)
        ));
        height - 3
    } else {
        lines.push(format!("╭{}╮", "─".repeat(inner)));
        height - 2
    };
    // Same line for folders and files, so labels line up across a row of tiles.
    let label_at = (height / 2).max(lines.len());
    for _ in 0..body {
        lines.push(empty.clone());
    }
    lines[label_at] = format!("│{label:^inner$}│");
    lines.push(format!("╰{}╯", "─".repeat(inner)));
    let style = Style::new().fg(type_color(entry.file_type));
    Text::from(lines.into_iter().map(Line::from).collect::<Vec<_>>()).style(style)
}

/// Line of the box that carries the label; the name and other columns are drawn on the same line.
pub const LABEL_LINE: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::{FileType, SpecialDir};
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

    fn lines(text: &Text) -> Vec<String> {
        text.lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn folder_has_a_tab_and_special_symbol() {
        let text = render(&entry("Downloads", true, Some(SpecialDir::Downloads)));
        assert_eq!(
            lines(&text),
            ["╭─╮       ", "│ ╰──────╮", "│   ↓    │", "╰────────╯"]
        );
    }

    #[test]
    fn file_box_shows_the_extension() {
        let text = render(&entry("Notes.pdf", false, None));
        assert_eq!(
            lines(&text),
            ["╭────────╮", "│        │", "│  PDF   │", "╰────────╯"]
        );
    }

    #[test]
    fn larger_boxes_keep_the_shape() {
        let text = render_sized(&entry("Music", true, Some(SpecialDir::Music)), 12, 6);
        assert_eq!(
            lines(&text),
            [
                "╭──╮        ",
                "│  ╰───────╮",
                "│          │",
                "│    ♪     │",
                "│          │",
                "╰──────────╯",
            ]
        );
        let text = render_sized(&entry("a.pdf", false, None), 10, 5);
        assert_eq!(lines(&text)[2], "│  PDF   │");
    }

    #[test]
    fn every_line_has_the_same_width() {
        for e in [entry("x", true, None), entry("a.markdown", false, None)] {
            for line in render(&e).lines {
                assert_eq!(line.width(), usize::from(WIDTH));
            }
        }
    }
}
