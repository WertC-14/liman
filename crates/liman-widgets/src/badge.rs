//! One-line type badge for the detailed view (ADR 0003): ` PDF `, ` RS `, ` ▸ ` for folders.

use liman_core::Entry;
use ratatui::style::Style;
use ratatui::text::Span;

use crate::theme::{text_on, type_color};

/// Badge width in cells, including padding.
pub const WIDTH: u16 = 6;

/// Short label: the extension in upper case (max 4 chars), `▸` for folders, `·` without extension.
pub fn label(entry: &Entry) -> String {
    if entry.is_dir {
        return "▸".into();
    }
    match entry.extension() {
        Some(ext) => ext.chars().take(4).collect::<String>().to_uppercase(),
        None => "·".into(),
    }
}

pub fn badge(entry: &Entry) -> Span<'static> {
    let bg = type_color(entry.file_type);
    let text = format!("{:^width$}", label(entry), width = usize::from(WIDTH));
    Span::styled(text, Style::new().bg(bg).fg(text_on(bg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::FileType;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            special: None,
            size: 0,
            item_count: None,
            modified: None,
            file_type: FileType::from_path(&PathBuf::from(name), is_dir),
        }
    }

    #[test]
    fn labels() {
        assert_eq!(label(&entry("Notes.pdf", false)), "PDF");
        assert_eq!(label(&entry("main.rs", false)), "RS");
        assert_eq!(label(&entry("a.markdown", false)), "MARK");
        assert_eq!(label(&entry("Makefile", false)), "·");
        assert_eq!(label(&entry("Downloads", true)), "▸");
    }

    #[test]
    fn badge_is_fixed_width() {
        assert_eq!(badge(&entry("x.pdf", false)).width(), usize::from(WIDTH));
        assert_eq!(badge(&entry("Music", true)).width(), usize::from(WIDTH));
    }
}
