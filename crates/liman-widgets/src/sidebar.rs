//! The Places panel (like cardea's "Places & Tree"): QUICK ACCESS (home, recent files, XDG
//! folders, bookmarks, trash, computer), a rule, then FOLDERS, a tree that opens and closes.
//!
//! Rows, top to bottom: header, one per place, rule, header, one per tree row. [`Sidebar::item`]
//! maps a row to what is there, so the app and the widget agree on the layout.

use std::path::Path;

use liman_core::Place;
use liman_core::i18n::tr;
use liman_core::tree::TreeRow;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::symbols;
use crate::theme::{self, type_color};

pub const WIDTH: u16 = 22;

/// What a row of the panel holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// A section title or the rule between the sections: not selectable.
    Label,
    /// Index into the places.
    Place(usize),
    /// Index into the tree rows.
    Dir(usize),
}

pub struct Sidebar<'a> {
    places: &'a [Place],
    tree: &'a [TreeRow],
    current: &'a Path,
    focused: Option<usize>,
    offset: usize,
}

impl<'a> Sidebar<'a> {
    pub fn new(places: &'a [Place], tree: &'a [TreeRow], current: &'a Path) -> Self {
        Self {
            places,
            tree,
            current,
            focused: None,
            offset: 0,
        }
    }

    /// Shows the keyboard cursor on row `index` (when the Places panel has focus).
    pub fn focused(mut self, index: usize) -> Self {
        self.focused = Some(index);
        self
    }

    /// First row shown (the panel scrolls when the tree is long).
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Number of rows for `places` places and `dirs` tree rows.
    pub fn len(places: usize, dirs: usize) -> usize {
        places + dirs + 3
    }

    /// What row `row` holds.
    pub fn item(places: usize, dirs: usize, row: usize) -> Option<Item> {
        match row {
            0 => Some(Item::Label),
            r if r <= places => Some(Item::Place(r - 1)),
            r if r <= places + 2 => Some(Item::Label),
            r if r < Self::len(places, dirs) => Some(Item::Dir(r - places - 3)),
            _ => None,
        }
    }

    /// The row of tree row `dir`.
    pub fn dir_row(places: usize, dir: usize) -> usize {
        places + 3 + dir
    }

    /// Which row is at terminal cell (`column`, `row`) with the panel scrolled by `offset`.
    pub fn row_at(area: Rect, offset: usize, len: usize, column: u16, row: u16) -> Option<usize> {
        let inside =
            column >= area.x && column < area.right() && row >= area.y && row < area.bottom();
        let index = offset + usize::from(row.checked_sub(area.y)?);
        (inside && index < len).then_some(index)
    }

    /// The column of a tree row's ▸/▾ (a click there opens or closes it).
    pub fn arrow_column(area: Rect, depth: usize) -> u16 {
        area.x + 1 + 2 * depth as u16
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let folder = type_color(liman_core::FileType::Folder);
        let section =
            |title: &str| Line::from(Span::raw(format!(" {title}")).fg(theme::accent()).bold());
        let shade = |active: bool| if active { theme::fg() } else { theme::dim() };
        let mut lines: Vec<Line> =
            Vec::with_capacity(Self::len(self.places.len(), self.tree.len()));
        lines.push(section(tr("QUICK ACCESS")));
        for place in self.places {
            lines.push(Line::from(vec![
                Span::raw(format!(" {} ", symbols::special_dir(place.kind))).fg(folder),
                Span::raw(place.name.as_str()).fg(shade(place.path == self.current)),
            ]));
        }
        let rule = "─".repeat(usize::from(area.width.saturating_sub(2)));
        lines.push(Line::from(
            Span::raw(format!(" {rule}")).fg(theme::border()),
        ));
        lines.push(section(tr("FOLDERS")));
        for dir in self.tree {
            let arrow = if dir.expanded { "▾" } else { "▸" };
            let icon = if crate::icons::nerd() {
                format!("{} ", crate::icons::folder(dir.expanded))
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::raw(format!(" {}{arrow} {icon}", "  ".repeat(dir.depth))).fg(folder),
                Span::raw(dir.name.as_str()).fg(shade(dir.path == self.current)),
            ]));
        }
        // Current folder in bold; the keyboard cursor gets a bar and a tinted row.
        let current_row = |i: usize| match Self::item(self.places.len(), self.tree.len(), i) {
            Some(Item::Place(p)) => self.places[p].path == self.current,
            Some(Item::Dir(d)) => self.tree[d].path == self.current,
            _ => false,
        };
        let lines: Vec<Line> = lines
            .into_iter()
            .enumerate()
            .skip(self.offset)
            .take(usize::from(area.height))
            .map(|(i, line)| {
                let mut line = if current_row(i) { line.bold() } else { line };
                if self.focused == Some(i) {
                    line.spans[0] = Span::raw(format!("▌{}", &line.spans[0].content[1..]))
                        .style(line.spans[0].style);
                    line = line.style(Style::new().bg(theme::selected_bg())).bold();
                }
                line
            })
            .collect();
        // No background of its own: it sits inside the Places panel.
        Paragraph::new(lines).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::SpecialDir;
    use std::path::PathBuf;

    #[test]
    fn rows_are_headers_places_rule_header_tree() {
        // 2 places, 3 tree rows.
        let item = |row| Sidebar::item(2, 3, row);
        assert_eq!(item(0), Some(Item::Label));
        assert_eq!(item(1), Some(Item::Place(0)));
        assert_eq!(item(2), Some(Item::Place(1)));
        assert_eq!(item(3), Some(Item::Label)); // rule
        assert_eq!(item(4), Some(Item::Label)); // FOLDERS
        assert_eq!(item(5), Some(Item::Dir(0)));
        assert_eq!(item(7), Some(Item::Dir(2)));
        assert_eq!(item(8), None);
        assert_eq!(Sidebar::dir_row(2, 2), 7);
    }

    #[test]
    fn row_at_counts_the_scroll_offset() {
        let area = Rect::new(0, 2, WIDTH, 10);
        assert_eq!(Sidebar::row_at(area, 0, 8, 3, 2), Some(0));
        assert_eq!(Sidebar::row_at(area, 4, 8, 3, 3), Some(5));
        assert_eq!(Sidebar::row_at(area, 0, 8, 3, 11), None); // below the last row
        assert_eq!(Sidebar::row_at(area, 0, 8, 30, 2), None); // right of the panel
    }

    #[test]
    fn places_tree_and_cursor_are_drawn() {
        let places = [Place {
            name: "Downloads".into(),
            path: PathBuf::from("/home/u/Downloads"),
            kind: SpecialDir::Downloads,
        }];
        let tree = [
            TreeRow {
                path: PathBuf::from("/home/u"),
                name: "u".into(),
                depth: 0,
                expanded: true,
            },
            TreeRow {
                path: PathBuf::from("/home/u/Downloads"),
                name: "Downloads".into(),
                depth: 1,
                expanded: false,
            },
        ];
        let area = Rect::new(0, 0, WIDTH, 6);
        let mut buf = Buffer::empty(area);
        Sidebar::new(&places, &tree, Path::new("/home/u/Downloads"))
            .focused(Sidebar::dir_row(1, 1))
            .render(area, &mut buf);
        let row = |y: u16| -> String { (0..WIDTH).map(|x| buf[(x, y)].symbol()).collect() };
        assert!(row(0).contains("QUICK ACCESS"));
        assert!(row(1).contains("↓ Downloads"));
        assert!(row(2).contains("───"));
        assert!(row(3).contains("FOLDERS"));
        assert!(row(4).contains("▾ u"));
        assert!(row(5).starts_with("▌  ▸ Downloads"), "{:?}", row(5));
        assert_eq!(buf[(8, 5)].bg, theme::selected_bg()); // keyboard cursor
        // The current folder is bold among the places too.
        assert!(
            buf[(5, 1)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }
}
