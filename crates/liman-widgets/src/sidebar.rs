//! Sidebar with places (Home, Downloads, ..., Trash, Computer). One row per place.

use std::path::Path;

use liman_core::Place;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::symbols;
use crate::theme::{self, type_color};

pub const WIDTH: u16 = 22;

pub struct Sidebar<'a> {
    places: &'a [Place],
    current: &'a Path,
    focused: Option<usize>,
}

impl<'a> Sidebar<'a> {
    pub fn new(places: &'a [Place], current: &'a Path) -> Self {
        Self {
            places,
            current,
            focused: None,
        }
    }

    /// Shows the keyboard cursor on place `index` (when the Places panel has focus).
    pub fn focused(mut self, index: usize) -> Self {
        self.focused = Some(index);
        self
    }

    /// Which place is at terminal row `row` (column is checked against `area`).
    pub fn row_at(area: Rect, count: usize, column: u16, row: u16) -> Option<usize> {
        let inside =
            column >= area.x && column < area.right() && row >= area.y && row < area.bottom();
        let index = usize::from(row.checked_sub(area.y)?);
        (inside && index < count).then_some(index)
    }
}

impl Widget for Sidebar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let folder = type_color(liman_core::FileType::Folder);
        let lines: Vec<Line> = self
            .places
            .iter()
            .enumerate()
            .map(|(i, place)| {
                let active = place.path == self.current;
                let cursor = self.focused == Some(i);
                let marker = if cursor { "▌" } else { " " };
                let line = Line::from(vec![
                    Span::raw(marker).fg(folder),
                    Span::raw(format!("{} ", symbols::special_dir(place.kind))).fg(folder),
                    Span::raw(place.name.as_str()).fg(if active || cursor {
                        theme::fg()
                    } else {
                        theme::dim()
                    }),
                ]);
                match (cursor, active) {
                    (true, _) => line.style(Style::new().bg(theme::selected_bg())).bold(),
                    (false, true) => line.bold(),
                    _ => line,
                }
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
    fn row_at_maps_rows_to_places() {
        let area = Rect::new(0, 2, WIDTH, 10);
        assert_eq!(Sidebar::row_at(area, 3, 5, 2), Some(0));
        assert_eq!(Sidebar::row_at(area, 3, 5, 4), Some(2));
        assert_eq!(Sidebar::row_at(area, 3, 5, 5), None); // below the last place
        assert_eq!(Sidebar::row_at(area, 3, 30, 2), None); // right of the sidebar
        assert_eq!(Sidebar::row_at(area, 3, 5, 1), None); // above
    }

    #[test]
    fn current_place_is_highlighted() {
        let places = [
            Place {
                name: "Home".into(),
                path: PathBuf::from("/home/u"),
                kind: SpecialDir::Home,
            },
            Place {
                name: "Downloads".into(),
                path: PathBuf::from("/home/u/Downloads"),
                kind: SpecialDir::Downloads,
            },
        ];
        let area = Rect::new(0, 0, WIDTH, 3);
        let mut buf = Buffer::empty(area);
        Sidebar::new(&places, Path::new("/home/u/Downloads")).render(area, &mut buf);
        let row = |y: u16| -> String { (0..WIDTH).map(|x| buf[(x, y)].symbol()).collect() };
        assert!(row(0).contains("⌂ Home"));
        assert!(row(1).contains("↓ Downloads"));
        assert!(
            buf[(10, 1)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        ); // current place

        let mut buf = Buffer::empty(area);
        Sidebar::new(&places, Path::new("/home/u"))
            .focused(1)
            .render(area, &mut buf);
        assert_eq!(buf[(10, 1)].bg, theme::selected_bg()); // keyboard cursor
        assert_eq!(buf[(0, 1)].symbol(), "▌");
    }
}
