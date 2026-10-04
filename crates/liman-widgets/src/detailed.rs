//! Detailed view (ADR 0003): one line per entry — badge, name, size, modified.
//!
//! Built on ratatui's `Table`, which already handles selection highlight and scrolling.
//! It is a `StatefulWidget`: the caller keeps a `TableState` (selected row, scroll offset)
//! between frames, the widget itself is rebuilt every frame.

use liman_core::Entry;
use liman_core::format::{self, Timestamp};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, StatefulWidget, Table, TableState};

use crate::badge::{self, badge};
use crate::theme::{DIM, FG, SELECTED_BG};

pub struct DetailedView<'a> {
    entries: &'a [Entry],
    now: Timestamp,
}

impl<'a> DetailedView<'a> {
    pub fn new(entries: &'a [Entry], now: Timestamp) -> Self {
        Self { entries, now }
    }
}

impl StatefulWidget for DetailedView<'_> {
    type State = TableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
        let header = Row::new([
            Cell::from(""),
            Cell::from("Name"),
            Cell::from(Line::from("Size").right_aligned()),
            Cell::from(Line::from("Modified").right_aligned()),
        ])
        .style(Style::new().fg(DIM))
        .bottom_margin(1);

        let rows = self.entries.iter().map(|entry| {
            let name = if entry.is_symlink {
                Line::from(vec![
                    Span::raw(entry.name.as_str()),
                    Span::raw(" ↗").fg(DIM),
                ])
            } else {
                Line::from(entry.name.as_str())
            };
            Row::new([
                Cell::from(badge(entry)),
                Cell::from(name.fg(FG)),
                Cell::from(Line::from(size_text(entry)).right_aligned().fg(DIM)),
                Cell::from(
                    Line::from(self.modified_text(entry))
                        .right_aligned()
                        .fg(DIM),
                ),
            ])
        });

        let widths = [
            Constraint::Length(badge::WIDTH),
            Constraint::Fill(1),
            Constraint::Length(10),
            Constraint::Length(16),
        ];
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(2)
            .row_highlight_style(Style::new().bg(SELECTED_BG));
        StatefulWidget::render(table, area, buf, state);
    }
}

impl DetailedView<'_> {
    fn modified_text(&self, entry: &Entry) -> String {
        entry
            .modified
            .map_or_else(String::new, |t| format::modified(t, self.now))
    }
}

fn size_text(entry: &Entry) -> String {
    if entry.is_dir {
        entry.item_count.map_or_else(|| "—".into(), format::items)
    } else {
        format::size(entry.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::FileType;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, size: u64, items: Option<usize>) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            size,
            item_count: items,
            modified: None,
            file_type: FileType::from_path(&PathBuf::from(name), is_dir),
        }
    }

    #[test]
    fn renders_header_and_rows() {
        let entries = [
            entry("Downloads", true, 0, Some(72)),
            entry("Notes.pdf", false, 17_100_000, None),
        ];
        let mut terminal = Terminal::new(TestBackend::new(60, 5)).unwrap();
        let mut state = TableState::default().with_selected(Some(0));
        terminal
            .draw(|f| {
                f.render_stateful_widget(
                    DetailedView::new(&entries, format::now()),
                    f.area(),
                    &mut state,
                )
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let row = |y: u16| -> String { (0..60).map(|x| buffer[(x, y)].symbol()).collect() };
        assert!(row(0).contains("Name") && row(0).contains("Size"));
        assert!(
            row(2).contains("▸") && row(2).contains("Downloads") && row(2).contains("72 items")
        );
        assert!(
            row(3).contains("PDF") && row(3).contains("Notes.pdf") && row(3).contains("17.1 MB")
        );
        // Selected row is highlighted.
        assert_eq!(buffer[(20, 2)].bg, SELECTED_BG);
    }
}
