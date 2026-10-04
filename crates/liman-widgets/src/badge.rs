//! One-line type badge for the detailed view (ADR 0003): ` PDF `, ` RS `, ` ▸ ` for folders.

use liman_core::Entry;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::symbols;
use crate::theme::type_color;

/// Badge width in cells, including padding.
pub const WIDTH: u16 = 6;

/// Short label: the extension in upper case (max 4 chars), a symbol for well-known folders
/// (`↓` Downloads, `♪` Music), `▸` for other folders, `·` without extension.
pub fn label(entry: &Entry) -> String {
    if let Some(kind) = entry.special {
        return symbols::special_dir(kind).into();
    }
    if entry.is_dir {
        return "▸".into();
    }
    match entry.extension() {
        Some(ext) => ext.chars().take(4).collect::<String>().to_uppercase(),
        None => "·".into(),
    }
}

/// The label in the type color on the normal background. (A filled color block per row made a
/// bright stripe down the list that tired the eyes.)
pub fn badge(entry: &Entry) -> Span<'static> {
    let text = format!("{:^width$}", label(entry), width = usize::from(WIDTH));
    Span::styled(
        text,
        Style::new()
            .fg(type_color(entry.file_type))
            .add_modifier(Modifier::BOLD),
    )
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
        assert_eq!(label(&entry("Projects", true)), "▸");
        let mut downloads = entry("Downloads", true);
        downloads.special = Some(liman_core::SpecialDir::Downloads);
        assert_eq!(label(&downloads), "↓");
    }

    #[test]
    fn badge_is_fixed_width() {
        assert_eq!(badge(&entry("x.pdf", false)).width(), usize::from(WIDTH));
        assert_eq!(badge(&entry("Music", true)).width(), usize::from(WIDTH));
    }
}
