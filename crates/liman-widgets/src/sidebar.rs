//! Sidebar with places (Home, Downloads, ..., Trash, Computer). One row per place.

use std::path::Path;

use liman_core::Place;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::symbols;
use crate::theme::{BAR_BG, DIM, FG, SELECTED_BG, type_color};

pub const WIDTH: u16 = 22;

pub struct Sidebar<'a> {
    places: &'a [Place],
    current: &'a Path,
}

impl<'a> Sidebar<'a> {
    pub fn new(places: &'a [Place], current: &'a Path) -> Self {
        Self { places, current }
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
            .map(|place| {
                let active = place.path == self.current;
                let line = Line::from(vec![
                    Span::raw(format!(" {} ", symbols::special_dir(place.kind))).fg(folder),
                    Span::raw(place.name.as_str()).fg(if active { FG } else { DIM }),
                ]);
                if active {
                    line.style(Style::new().bg(SELECTED_BG)).bold()
                } else {
                    line
                }
            })
            .collect();
        Paragraph::new(lines)
            .style(Style::new().bg(BAR_BG))
            .render(area, buf);
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
        assert_eq!(buf[(10, 1)].bg, SELECTED_BG);
        assert_eq!(buf[(10, 0)].bg, BAR_BG);
    }
}
