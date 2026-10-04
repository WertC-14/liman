//! Grid view (ADR 0003, 0006): one hollow, type-colored box per entry, name below.
//! Zooming changes the box size; medium boxes are the default.
//!
//! Uses a `TableState` as state (selected index; offset = first visible tile row) so the app keeps
//! one selection across all three views.

use std::collections::HashSet;
use std::path::PathBuf;

use liman_core::Entry;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, TableState, Widget};

use crate::boxes;
use crate::theme;

/// Box sizes (width, height in cells) for the zoom levels, smallest first.
pub const BOX_SIZES: [(u16, u16); 4] = [(12, 5), (16, 7), (20, 9), (26, 11)];

/// Tile size in cells for a zoom level: the box plus a gap on the sides, the name line and a gap.
pub const fn tile_size(level: usize) -> (u16, u16) {
    let (w, h) = BOX_SIZES[level];
    (w + 4, h + 2)
}

/// Smallest tile, for "does the grid fit at all".
pub const MIN_TILE: (u16, u16) = tile_size(0);

/// Level used when the user has not zoomed: medium boxes (16×7), about 7 per row on a wide terminal.
pub const DEFAULT_LEVEL: usize = 1;

/// The largest level not above `wanted` for which one tile fits in `area`.
pub fn fitting_level(wanted: usize, area: Rect) -> usize {
    (0..=wanted.min(BOX_SIZES.len() - 1))
        .rev()
        .find(|&level| {
            let (w, h) = tile_size(level);
            area.width >= w && area.height >= h
        })
        .unwrap_or(0)
}

pub struct GridView<'a> {
    entries: &'a [&'a Entry],
    marked: Option<&'a HashSet<PathBuf>>,
    level: usize,
}

impl<'a> GridView<'a> {
    pub fn new(entries: &'a [&'a Entry], level: usize) -> Self {
        Self {
            entries,
            marked: None,
            level: level.min(BOX_SIZES.len() - 1),
        }
    }

    pub fn marked(mut self, marked: &'a HashSet<PathBuf>) -> Self {
        self.marked = Some(marked);
        self
    }

    /// Tiles per row for an area of this width (at least one).
    pub fn columns(width: u16, level: usize) -> usize {
        usize::from((width / tile_size(level).0).max(1))
    }

    /// Which entry index is at terminal cell (`column`, `row`); `offset` is the first visible tile row.
    pub fn index_at(
        area: Rect,
        offset: usize,
        level: usize,
        column: u16,
        row: u16,
    ) -> Option<usize> {
        let inside =
            column >= area.x && column < area.right() && row >= area.y && row < area.bottom();
        if !inside {
            return None;
        }
        let (tile_w, tile_h) = tile_size(level);
        let tile_col = usize::from((column - area.x) / tile_w);
        let cols = Self::columns(area.width, level);
        if tile_col >= cols {
            return None; // the unused strip at the right edge
        }
        let tile_row = usize::from((row - area.y) / tile_h) + offset;
        Some(tile_row * cols + tile_col)
    }
}

impl StatefulWidget for GridView<'_> {
    type State = TableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
        let (tile_w, tile_h) = tile_size(self.level);
        let cols = Self::columns(area.width, self.level);
        let visible_rows = usize::from((area.height / tile_h).max(1));
        // Keep the selected tile on screen.
        if let Some(selected) = state.selected() {
            let row = selected / cols;
            let offset = state.offset_mut();
            if row < *offset {
                *offset = row;
            } else if row >= *offset + visible_rows {
                *offset = row + 1 - visible_rows;
            }
        }
        let first = state.offset() * cols;
        for (i, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(first)
            .take(visible_rows * cols)
        {
            let slot = i - first;
            let x = area.x + (slot % cols) as u16 * tile_w;
            let y = area.y + (slot / cols) as u16 * tile_h;
            let tile = Rect::new(x, y, tile_w, tile_h - 1).intersection(area);
            let selected = state.selected() == Some(i);
            let marked = self.marked.is_some_and(|m| m.contains(&entry.path));
            self.render_tile(entry, tile, selected, marked, buf);
        }
    }
}

impl GridView<'_> {
    fn render_tile(
        &self,
        entry: &Entry,
        tile: Rect,
        selected: bool,
        marked: bool,
        buf: &mut Buffer,
    ) {
        let (box_w, box_h) = BOX_SIZES[self.level];
        let bg = match (selected, marked) {
            (true, _) => theme::selected_bg(),
            (false, true) => theme::marked_bg(),
            _ => theme::bg(),
        };
        buf.set_style(tile, Style::new().bg(bg));

        let mut text = boxes::render_sized(entry, box_w, box_h);
        if selected {
            text = text.patch_style(Style::new().add_modifier(Modifier::BOLD));
        }
        let box_area = Rect::new(tile.x + 2, tile.y, box_w, box_h).intersection(tile);
        text.render(box_area, buf);

        let name_area = Rect::new(tile.x, tile.y + box_h, tile.width, 1).intersection(tile);
        let mut name = truncate(&entry.name, usize::from(tile.width.saturating_sub(2)));
        if marked {
            name = format!("✓ {name}");
        }
        let color = if bg == theme::bg() {
            theme::fg()
        } else {
            Color::White
        };
        let mut spans = vec![Span::styled(name, Style::new().fg(color))];
        if entry.is_symlink {
            spans.push(Span::styled(" ↗", Style::new().fg(theme::dim())));
        }
        Line::from(spans).centered().render(name_area, buf);
    }
}

/// Cuts `name` to `max` characters with a trailing `…`.
fn truncate(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_string()
    } else {
        let mut s: String = name.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::{FileType, SpecialDir};

    fn entry(name: &str, is_dir: bool) -> Entry {
        let path = PathBuf::from(name);
        Entry {
            name: name.into(),
            file_type: FileType::from_path(&path, is_dir),
            path,
            is_dir,
            is_symlink: false,
            special: None,
            size: 0,
            item_count: None,
            modified: None,
        }
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn draws_hollow_boxes_and_names() {
        let mut music = entry("Music", true);
        music.special = Some(SpecialDir::Music);
        let owned = [music, entry("Notes.pdf", false), entry("c.rs", false)];
        let entries: Vec<&Entry> = owned.iter().collect();
        let area = Rect::new(0, 0, 32, 14); // level 0: tiles 16×7 → 2 columns, 2 rows
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(1));
        GridView::new(&entries, 0).render(area, &mut buf, &mut state);

        assert!(row(&buf, 0).starts_with("  ╭──╮"));
        assert!(row(&buf, 2).contains("♪"));
        assert!(row(&buf, 2).contains("PDF"));
        assert!(row(&buf, 5).contains("Music") && row(&buf, 5).contains("Notes.pdf"));
        assert!(row(&buf, 9).contains("RS")); // third entry on the second tile row
        // Inside of a box is empty (hollow), the selected tile is tinted.
        assert_eq!(buf[(22, 1)].symbol(), " ");
        assert_eq!(buf[(22, 1)].bg, theme::selected_bg());
        assert_eq!(buf[(6, 1)].bg, theme::bg());
    }

    #[test]
    fn levels_follow_the_space() {
        assert_eq!(tile_size(0), (16, 7));
        assert_eq!(fitting_level(3, Rect::new(0, 0, 22, 12)), 1);
    }

    #[test]
    fn index_at_maps_cells_to_entries() {
        let area = Rect::new(10, 2, 36, 30); // level 0: 2 columns (32 cells) + 4 unused
        assert_eq!(GridView::index_at(area, 0, 0, 10, 2), Some(0));
        assert_eq!(GridView::index_at(area, 0, 0, 27, 2), Some(1));
        assert_eq!(GridView::index_at(area, 0, 0, 44, 2), None); // unused right strip
        assert_eq!(GridView::index_at(area, 0, 0, 12, 9), Some(2)); // second tile row
        assert_eq!(GridView::index_at(area, 3, 0, 12, 9), Some(8)); // scrolled by 3 rows
        assert_eq!(GridView::index_at(area, 0, 0, 5, 5), None); // left of the area
    }

    #[test]
    fn selection_scrolls_into_view() {
        let owned: Vec<Entry> = (0..20)
            .map(|i| entry(&format!("f{i}.txt"), false))
            .collect();
        let entries: Vec<&Entry> = owned.iter().collect();
        let area = Rect::new(0, 0, 32, 14); // 2 columns, 2 visible rows
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(9)); // row 4
        GridView::new(&entries, 0).render(area, &mut buf, &mut state);
        assert_eq!(state.offset(), 3);
    }

    #[test]
    fn long_names_are_truncated() {
        assert_eq!(truncate("short.txt", 18), "short.txt");
        assert_eq!(truncate("a-very-long-file-name.txt", 10), "a-very-lo…");
    }
}
