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

/// Four lines, `WIDTH` cells each.
pub fn render(entry: &Entry) -> Text<'static> {
    let width = usize::from(WIDTH);
    let inner = width - 2;
    let label = if entry.is_dir && entry.special.is_none() {
        String::new() // plain folders: the shape says enough
    } else {
        badge::label(entry)
    };
    let middle = format!("│{label:^inner$}│");
    let bottom = format!("╰{}╯", "─".repeat(inner));
    let lines = if entry.is_dir {
        let tab = (width / 3).max(3);
        [
            format!("╭{}╮{}", "─".repeat(tab - 2), " ".repeat(width - tab)),
            format!("│{}╰{}╮", " ".repeat(tab - 2), "─".repeat(width - tab - 1)),
            middle,
            bottom,
        ]
    } else {
        [
            format!("╭{}╮", "─".repeat(inner)),
            format!("│{}│", " ".repeat(inner)),
            middle,
            bottom,
        ]
    };
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
    fn every_line_has_the_same_width() {
        for e in [entry("x", true, None), entry("a.markdown", false, None)] {
            for line in render(&e).lines {
                assert_eq!(line.width(), usize::from(WIDTH));
            }
        }
    }
}
