//! The file list in two of the three views (ADR 0003):
//! - **Detailed**: one line per entry — badge, name, size, modified.
//! - **Normal**: a type-colored box per entry (4 lines), name and details on the box's label line.
//!
//! Built on ratatui's `Table`, which handles selection highlight and scrolling for rows of any height.
//! It is a `StatefulWidget`: the caller keeps a `TableState` (selected row, scroll offset)
//! between frames, the widget itself is rebuilt every frame.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use liman_core::Entry;
use liman_core::format::{self, Timestamp};
use liman_core::git::GitMark;
use liman_core::sort::{SortKey, SortOrder};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, Row, StatefulWidget, Table, TableState};

use crate::boxes;
use crate::gitmark;
use crate::theme;

const COLUMN_SPACING: u16 = 2;
const DETAILED_WIDTHS: [Constraint; 4] = [
    Constraint::Fill(1),
    Constraint::Length(12),
    Constraint::Length(12),
    Constraint::Length(16),
];

/// Rows taken by the header line and the blank line under it.
pub const HEADER_HEIGHT: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    Detailed,
    Normal,
}

impl ListMode {
    /// Terminal rows one entry takes, including the gap below it.
    pub const fn row_height(self) -> u16 {
        match self {
            Self::Detailed => 1,
            Self::Normal => boxes::HEIGHT + 1,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Detailed => "Detailed",
            Self::Normal => "Normal",
        }
    }
}

pub struct FileList<'a> {
    entries: &'a [&'a Entry],
    now: Timestamp,
    mode: ListMode,
    marked: Option<&'a HashSet<PathBuf>>,
    sort: Option<SortOrder>,
    git: Option<&'a HashMap<PathBuf, GitMark>>,
}

impl<'a> FileList<'a> {
    /// `entries` are the rows to show, already filtered and sorted by the caller.
    pub fn new(entries: &'a [&'a Entry], now: Timestamp, mode: ListMode) -> Self {
        Self {
            entries,
            now,
            mode,
            marked: None,
            sort: None,
            git: None,
        }
    }

    /// Inside a git repository: status letters before each name.
    pub fn git(mut self, marks: &'a HashMap<PathBuf, GitMark>) -> Self {
        self.git = Some(marks);
        self
    }

    /// Shows ▲/▼ next to the sorted column in the header.
    pub fn sort(mut self, order: SortOrder) -> Self {
        self.sort = Some(order);
        self
    }

    /// Detailed view: the sort key of the header column under `column` (header row only).
    pub fn sort_key_at(area: Rect, mode: ListMode, column: u16, row: u16) -> Option<SortKey> {
        if mode != ListMode::Detailed || row != area.y || column < area.x || column >= area.right()
        {
            return None;
        }
        let rects = Layout::horizontal(DETAILED_WIDTHS)
            .flex(Flex::Start)
            .spacing(COLUMN_SPACING)
            .split(Rect::new(area.x, 0, area.width, 1));
        let i = rects
            .iter()
            .position(|r| column >= r.x && column < r.right())?;
        [
            SortKey::Name,
            SortKey::Type,
            SortKey::Size,
            SortKey::Modified,
        ]
        .get(i)
        .copied()
    }

    /// Entries whose path is in `marked` get a check mark and a tinted background.
    pub fn marked(mut self, marked: &'a HashSet<PathBuf>) -> Self {
        self.marked = Some(marked);
        self
    }

    /// Hit-testing for mouse clicks: which row index sits at terminal cell (`column`, `row`)
    /// when the list was rendered into `area` with scroll `offset` in `mode`. The caller checks
    /// the result against the number of rows.
    pub fn row_at(
        area: Rect,
        offset: usize,
        mode: ListMode,
        column: u16,
        row: u16,
    ) -> Option<usize> {
        let inside = column >= area.x
            && column < area.right()
            && row >= area.y + HEADER_HEIGHT
            && row < area.bottom();
        let line = row.checked_sub(area.y + HEADER_HEIGHT)?;
        let height = mode.row_height();
        // The gap line under a box belongs to no entry.
        let on_gap = mode == ListMode::Normal && line % height == height - 1;
        (inside && !on_gap).then(|| offset + usize::from(line / height))
    }

    fn row(&self, entry: &Entry) -> Row<'static> {
        let is_marked = self.marked.is_some_and(|m| m.contains(&entry.path));
        let mut name = Vec::new();
        if let Some(marks) = self.git {
            name.extend(gitmark::spans(marks.get(&entry.path).copied()));
        }
        if is_marked {
            name.push(Span::raw("✓ ").bold());
        }
        name.push(Span::raw(entry.name.clone()));
        if entry.is_symlink {
            name.push(Span::raw(" ↗").fg(theme::dim()));
        }
        // Folders stand out by their name (bold, folder color), not by an icon column.
        let name = if entry.is_dir {
            Line::from(name).fg(theme::accent()).bold()
        } else {
            Line::from(name).fg(theme::fg())
        };
        let size = Line::from(size_text(entry))
            .right_aligned()
            .fg(theme::dim());
        let modified = Line::from(self.modified_text(entry))
            .right_aligned()
            .fg(theme::dim());
        let row = match self.mode {
            ListMode::Detailed => Row::new([
                Cell::from(name),
                Cell::from(Line::from(type_text(entry)).fg(theme::dim())),
                Cell::from(size),
                Cell::from(modified),
            ]),
            ListMode::Normal => Row::new([
                Cell::from(boxes::render(entry)),
                Cell::from(on_label_line(name)),
                Cell::from(on_label_line(size)),
                Cell::from(on_label_line(modified)),
            ])
            .height(boxes::HEIGHT)
            .bottom_margin(1),
        };
        if is_marked {
            row.style(Style::new().bg(theme::marked_bg()))
        } else {
            row
        }
    }

    fn modified_text(&self, entry: &Entry) -> String {
        entry
            .modified
            .map_or_else(String::new, |t| format::modified(t, self.now))
    }
}

/// Pushes a single line down to the box's label line.
fn on_label_line(line: Line<'static>) -> Text<'static> {
    let mut lines = vec![Line::default(); boxes::LABEL_LINE];
    lines.push(line);
    Text::from(lines)
}

impl StatefulWidget for FileList<'_> {
    type State = TableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
        let title = |text: &str, key: SortKey| match self.sort {
            Some(o) if o.key == key => format!("{text} {}", if o.descending { "▼" } else { "▲" }),
            _ => text.to_string(),
        };
        let size = Cell::from(Line::from(title("Size", SortKey::Size)).right_aligned());
        let modified = Cell::from(Line::from(title("Modified", SortKey::Modified)).right_aligned());
        let (header, widths) = match self.mode {
            // Detailed: no icon column, the type is written out (Folder, PDF file, ...).
            ListMode::Detailed => (
                Row::new([
                    Cell::from(title("Name", SortKey::Name)),
                    Cell::from(title("Type", SortKey::Type)),
                    size,
                    modified,
                ]),
                DETAILED_WIDTHS,
            ),
            ListMode::Normal => (
                Row::new([Cell::from(""), Cell::from("Name"), size, modified]),
                [
                    Constraint::Length(boxes::WIDTH),
                    Constraint::Fill(1),
                    Constraint::Length(10),
                    Constraint::Length(16),
                ],
            ),
        };
        let header = header.style(Style::new().fg(theme::dim())).bottom_margin(1);
        // Only the rows on screen are built (a folder with 20 000 files cost 100 ms per frame when
        // every row was built). We keep the scroll offset ourselves and hand `Table` just the window.
        let (start, end) = visible_window(state, self.entries.len(), area, self.mode);
        let rows: Vec<_> = self.entries[start..end]
            .iter()
            .map(|e| self.row(e))
            .collect();
        let mut window = TableState::default().with_selected(state.selected().map(|s| s - start));
        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(COLUMN_SPACING)
            .row_highlight_style(Style::new().bg(theme::selected_bg()));
        StatefulWidget::render(table, area, buf, &mut window);
    }
}

/// The range of entries shown in `area`, scrolled so the selection is visible. Updates the
/// offset in `state` (first shown entry), which hit-testing and the next frame use.
fn visible_window(
    state: &mut TableState,
    len: usize,
    area: Rect,
    mode: ListMode,
) -> (usize, usize) {
    let capacity =
        usize::from((area.height.saturating_sub(HEADER_HEIGHT) / mode.row_height()).max(1));
    let mut offset = state.offset();
    if let Some(selected) = state.selected() {
        if selected < offset {
            offset = selected;
        } else if selected >= offset + capacity {
            offset = selected + 1 - capacity;
        }
    }
    // Do not leave empty space below the last entry when there is enough to fill it.
    offset = offset.min(len.saturating_sub(capacity));
    *state.offset_mut() = offset;
    (offset, (offset + capacity).min(len))
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn header_click_finds_the_sort_column() {
        let area = Rect::new(0, 0, 100, 10);
        // widths: Fill(1)=52, 12, 12, 16 with spacing 2 -> Name 0..52, Type 54..66, Size 68..80, Modified 82..98
        assert_eq!(
            FileList::sort_key_at(area, ListMode::Detailed, 3, 0),
            Some(SortKey::Name)
        );
        assert_eq!(
            FileList::sort_key_at(area, ListMode::Detailed, 60, 0),
            Some(SortKey::Type)
        );
        assert_eq!(
            FileList::sort_key_at(area, ListMode::Detailed, 70, 0),
            Some(SortKey::Size)
        );
        assert_eq!(
            FileList::sort_key_at(area, ListMode::Detailed, 90, 0),
            Some(SortKey::Modified)
        );
        assert_eq!(FileList::sort_key_at(area, ListMode::Detailed, 90, 1), None);
        assert_eq!(FileList::sort_key_at(area, ListMode::Normal, 3, 0), None);
    }

    #[test]
    fn window_follows_the_selection_and_fills_the_screen() {
        let area = Rect::new(0, 0, 80, 12); // 10 rows after the header
        let mut state = TableState::default().with_selected(Some(0));
        assert_eq!(
            visible_window(&mut state, 100, area, ListMode::Detailed),
            (0, 10)
        );
        state.select(Some(25));
        assert_eq!(
            visible_window(&mut state, 100, area, ListMode::Detailed),
            (16, 26)
        );
        state.select(Some(20));
        assert_eq!(
            visible_window(&mut state, 100, area, ListMode::Detailed),
            (16, 26)
        );
        state.select(Some(99));
        assert_eq!(
            visible_window(&mut state, 100, area, ListMode::Detailed),
            (90, 100)
        );
        state.select(Some(3));
        assert_eq!(
            visible_window(&mut state, 5, area, ListMode::Detailed),
            (0, 5)
        );
        assert_eq!(
            visible_window(&mut state, 0, area, ListMode::Detailed),
            (0, 0)
        );
    }
}

/// "Folder", "PDF file", "File" (no extension).
fn type_text(entry: &Entry) -> String {
    if entry.is_dir {
        return "Folder".into();
    }
    match entry.extension() {
        Some(ext) => format!(
            "{} file",
            ext.chars().take(5).collect::<String>().to_uppercase()
        ),
        None => "File".into(),
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
            special: None,
            size,
            item_count: items,
            modified: None,
            file_type: FileType::from_path(&PathBuf::from(name), is_dir),
        }
    }

    fn draw(mode: ListMode, height: u16) -> Vec<String> {
        let owned = [
            entry("Downloads", true, 0, Some(72)),
            entry("Notes.pdf", false, 17_100_000, None),
        ];
        let entries: Vec<&Entry> = owned.iter().collect();
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        let mut state = TableState::default().with_selected(Some(0));
        terminal
            .draw(|f| {
                f.render_stateful_widget(
                    FileList::new(&entries, format::now(), mode),
                    f.area(),
                    &mut state,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn detailed_renders_header_and_one_line_per_entry() {
        let rows = draw(ListMode::Detailed, 5);
        assert!(rows[0].contains("Name") && rows[0].contains("Size"));
        assert!(rows[0].contains("Type"));
        assert!(
            rows[2].contains("Downloads")
                && rows[2].contains("Folder")
                && rows[2].contains("72 items")
        );
        assert!(
            rows[3].contains("Notes.pdf")
                && rows[3].contains("PDF file")
                && rows[3].contains("17.1 MB")
        );
        assert!(!rows[2].contains('▸')); // no icon column any more
    }

    #[test]
    fn normal_renders_boxes_with_details_on_the_label_line() {
        let rows = draw(ListMode::Normal, 13);
        // header (0), blank (1), folder box (2..=5), gap (6), file box (7..=10)
        assert!(rows[2].starts_with("╭─╮"));
        assert!(rows[4].contains("Downloads") && rows[4].contains("72 items"));
        assert!(
            rows[9].contains("PDF") && rows[9].contains("Notes.pdf") && rows[9].contains("17.1 MB")
        );
        assert!(rows[6].trim().is_empty());
    }

    #[test]
    fn marked_rows_get_a_check_mark_and_background() {
        let owned = [
            entry("a.txt", false, 1, None),
            entry("b.txt", false, 1, None),
        ];
        let entries: Vec<&Entry> = owned.iter().collect();
        let marked: HashSet<PathBuf> = [PathBuf::from("b.txt")].into();
        let mut terminal = Terminal::new(TestBackend::new(60, 4)).unwrap();
        let mut state = TableState::default().with_selected(Some(0));
        terminal
            .draw(|f| {
                f.render_stateful_widget(
                    FileList::new(&entries, format::now(), ListMode::Detailed).marked(&marked),
                    f.area(),
                    &mut state,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..60).map(|x| buffer[(x, 3)].symbol()).collect();
        assert!(row.contains("✓ b.txt"));
        assert_eq!(buffer[(30, 3)].bg, theme::marked_bg());
    }

    #[test]
    fn row_at_detailed() {
        let area = Rect::new(2, 3, 40, 10);
        let at = |offset, row| FileList::row_at(area, offset, ListMode::Detailed, 10, row);
        assert_eq!(at(0, 3), None); // header
        assert_eq!(at(0, 4), None); // blank line under header
        assert_eq!(at(0, 5), Some(0));
        assert_eq!(at(7, 6), Some(8));
        assert_eq!(FileList::row_at(area, 0, ListMode::Detailed, 1, 6), None); // left of the area
        assert_eq!(at(0, 13), None); // below the area
    }

    #[test]
    fn row_at_normal_skips_the_gap_lines() {
        let area = Rect::new(0, 0, 40, 30);
        let at = |offset, row| FileList::row_at(area, offset, ListMode::Normal, 5, row);
        assert_eq!(at(0, 2), Some(0)); // first line of the first box
        assert_eq!(at(0, 5), Some(0)); // last line of the first box
        assert_eq!(at(0, 6), None); // gap
        assert_eq!(at(0, 7), Some(1));
        assert_eq!(at(3, 7), Some(4));
    }
}
