//! Grid view (ADR 0003): a 16×8-cell icon per entry, drawn with half blocks (ADR 0002), name below.
//!
//! One terminal cell shows two icon pixels: `▀` with the top pixel as foreground color and the
//! bottom pixel as background color. Transparent pixels are blended with the cell background,
//! so selection and marking tint the whole tile.
//!
//! Uses a `TableState` as state (selected index; offset = first visible tile row) so the app keeps
//! one selection across all three views.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use liman_core::Entry;
use liman_core::icons::{ICON_SIZE, IconPixels, icon_key};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, TableState, Widget};

use crate::badge::badge;
use crate::theme::{BG, DIM, FG, MARKED_BG, SELECTED_BG};

/// Tile size in terminal cells: icon (16×8) plus a name line and gaps.
pub const TILE_WIDTH: u16 = 20;
pub const TILE_HEIGHT: u16 = 10;
const ICON_COLS: u16 = ICON_SIZE as u16;
const ICON_ROWS: u16 = ICON_COLS / 2;

pub struct GridView<'a> {
    entries: &'a [&'a Entry],
    icons: &'a HashMap<String, IconPixels>,
    marked: Option<&'a HashSet<PathBuf>>,
}

impl<'a> GridView<'a> {
    /// `icons` maps [`icon_key`] to rendered icons; entries without one show their badge until it arrives.
    pub fn new(entries: &'a [&'a Entry], icons: &'a HashMap<String, IconPixels>) -> Self {
        Self {
            entries,
            icons,
            marked: None,
        }
    }

    pub fn marked(mut self, marked: &'a HashSet<PathBuf>) -> Self {
        self.marked = Some(marked);
        self
    }

    /// Tiles per row for an area of this width (at least one).
    pub fn columns(width: u16) -> usize {
        usize::from((width / TILE_WIDTH).max(1))
    }

    /// Which entry index is at terminal cell (`column`, `row`); `offset` is the first visible tile row.
    pub fn index_at(area: Rect, offset: usize, column: u16, row: u16) -> Option<usize> {
        let inside =
            column >= area.x && column < area.right() && row >= area.y && row < area.bottom();
        if !inside {
            return None;
        }
        let tile_col = usize::from((column - area.x) / TILE_WIDTH);
        let cols = Self::columns(area.width);
        if tile_col >= cols {
            return None; // the unused strip at the right edge
        }
        let tile_row = usize::from((row - area.y) / TILE_HEIGHT) + offset;
        Some(tile_row * cols + tile_col)
    }
}

impl StatefulWidget for GridView<'_> {
    type State = TableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
        let cols = Self::columns(area.width);
        let visible_rows = usize::from((area.height / TILE_HEIGHT).max(1));
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
        let offset = state.offset();
        let first = offset * cols;
        for (i, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(first)
            .take(visible_rows * cols)
        {
            let slot = i - first;
            let x = area.x + (slot % cols) as u16 * TILE_WIDTH;
            let y = area.y + (slot / cols) as u16 * TILE_HEIGHT;
            let tile = Rect::new(x, y, TILE_WIDTH, TILE_HEIGHT - 1).intersection(area);
            let bg = if state.selected() == Some(i) {
                SELECTED_BG
            } else if self.marked.is_some_and(|m| m.contains(&entry.path)) {
                MARKED_BG
            } else {
                BG
            };
            self.render_tile(entry, tile, bg, buf);
        }
    }
}

impl GridView<'_> {
    fn render_tile(&self, entry: &Entry, tile: Rect, bg: Color, buf: &mut Buffer) {
        buf.set_style(tile, Style::new().bg(bg));
        let icon_x = tile.x + (TILE_WIDTH - ICON_COLS) / 2;
        match self.icons.get(&icon_key(entry)) {
            Some(icon) => draw_icon(icon, icon_x, tile.y, bg, tile, buf),
            None => {
                // Icon still loading: show the badge in the middle of where the icon will be.
                let line = Line::from(badge(entry));
                let w = line.width() as u16;
                let area = Rect::new(tile.x + (TILE_WIDTH - w) / 2, tile.y + ICON_ROWS / 2, w, 1);
                line.render(area.intersection(tile), buf);
            }
        }
        let name_area = Rect::new(tile.x, tile.y + ICON_ROWS, TILE_WIDTH, 1).intersection(tile);
        let name = truncate(&entry.name, usize::from(TILE_WIDTH) - 2);
        let color = if bg == BG { FG } else { Color::White };
        Line::from(Span::styled(name, Style::new().fg(color)))
            .centered()
            .render(name_area, buf);
        if entry.is_symlink {
            Line::from(Span::styled("↗", Style::new().fg(DIM))).render(
                Rect::new(icon_x + ICON_COLS - 1, tile.y, 1, 1).intersection(tile),
                buf,
            );
        }
    }
}

/// Paints the icon at (`x`, `y`): one `▀` per pair of pixel rows, transparent pixels show `bg`.
fn draw_icon(icon: &IconPixels, x: u16, y: u16, bg: Color, clip: Rect, buf: &mut Buffer) {
    let base = rgb(bg);
    for row in 0..ICON_ROWS {
        for col in 0..ICON_COLS {
            let pos = (x + col, y + row);
            if !clip.contains(pos.into()) {
                continue;
            }
            let top = blend(icon.get(u32::from(col), u32::from(row) * 2), base);
            let bottom = blend(icon.get(u32::from(col), u32::from(row) * 2 + 1), base);
            let cell = &mut buf[pos];
            if top == bottom {
                cell.set_char(' ').set_bg(Color::Rgb(top.0, top.1, top.2));
            } else {
                cell.set_char('▀')
                    .set_fg(Color::Rgb(top.0, top.1, top.2))
                    .set_bg(Color::Rgb(bottom.0, bottom.1, bottom.2));
            }
        }
    }
}

fn rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

/// Straight-alpha pixel over an opaque background.
fn blend([r, g, b, a]: [u8; 4], (br, bgc, bb): (u8, u8, u8)) -> (u8, u8, u8) {
    let mix = |c: u8, base: u8| {
        ((u16::from(c) * u16::from(a) + u16::from(base) * (255 - u16::from(a))) / 255) as u8
    };
    (mix(r, br), mix(g, bgc), mix(b, bb))
}

/// Cuts `name` to `max` characters with a trailing `…`.
fn truncate(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_string()
    } else {
        let mut s: String = name.chars().take(max - 1).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::FileType;

    fn entry(name: &str) -> Entry {
        let path = PathBuf::from(name);
        Entry {
            name: name.into(),
            file_type: FileType::from_path(&path, false),
            path,
            is_dir: false,
            is_symlink: false,
            special: None,
            size: 0,
            item_count: None,
            modified: None,
        }
    }

    /// Red top half, transparent bottom half.
    fn half_red() -> IconPixels {
        let n = ICON_SIZE * ICON_SIZE;
        let rgba = (0..n)
            .map(|i| {
                if i < n / 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 0, 0]
                }
            })
            .collect();
        IconPixels {
            size: ICON_SIZE,
            rgba,
        }
    }

    #[test]
    fn draws_icons_and_names_in_tiles() {
        let owned = [entry("a.pdf"), entry("b.pdf"), entry("c.rs")];
        let entries: Vec<&Entry> = owned.iter().collect();
        let icons: HashMap<String, IconPixels> = [(icon_key(&owned[0]), half_red())].into();
        let area = Rect::new(0, 0, 40, 20); // 2 columns, 2 rows
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(1));
        GridView::new(&entries, &icons).render(area, &mut buf, &mut state);

        // Tile 0: icon starts at x = 2; top rows red, bottom rows background.
        assert_eq!(buf[(5, 0)].bg, Color::Rgb(255, 0, 0));
        assert_eq!(buf[(5, 7)].bg, BG);
        let name_row: String = (0..40).map(|x| buf[(x, 8)].symbol()).collect();
        assert!(name_row.contains("a.pdf") && name_row.contains("b.pdf"));
        // Tile 1 is selected: transparent pixels show the selection color.
        assert_eq!(buf[(25, 7)].bg, SELECTED_BG);
        // Tile 2 (c.rs, no icon yet) is on the second row and shows its badge.
        let badge_row: String = (0..40).map(|x| buf[(x, 14)].symbol()).collect();
        assert!(badge_row.contains("RS"));
    }

    #[test]
    fn index_at_maps_cells_to_entries() {
        let area = Rect::new(10, 2, 45, 30); // 2 columns (40 cells) + 5 unused
        assert_eq!(GridView::index_at(area, 0, 10, 2), Some(0));
        assert_eq!(GridView::index_at(area, 0, 31, 2), Some(1));
        assert_eq!(GridView::index_at(area, 0, 52, 2), None); // unused right strip
        assert_eq!(GridView::index_at(area, 0, 12, 13), Some(2)); // second tile row
        assert_eq!(GridView::index_at(area, 3, 12, 13), Some(8)); // scrolled by 3 rows
        assert_eq!(GridView::index_at(area, 0, 5, 5), None); // left of the area
    }

    #[test]
    fn selection_scrolls_into_view() {
        let owned: Vec<Entry> = (0..20).map(|i| entry(&format!("f{i}.txt"))).collect();
        let entries: Vec<&Entry> = owned.iter().collect();
        let icons = HashMap::new();
        let area = Rect::new(0, 0, 40, 20); // 2 columns, 2 visible rows
        let mut buf = Buffer::empty(area);
        let mut state = TableState::default().with_selected(Some(9)); // row 4
        GridView::new(&entries, &icons).render(area, &mut buf, &mut state);
        assert_eq!(state.offset(), 3);
    }

    #[test]
    fn long_names_are_truncated() {
        assert_eq!(truncate("short.txt", 18), "short.txt");
        assert_eq!(truncate("a-very-long-file-name.txt", 10), "a-very-lo…");
    }
}
