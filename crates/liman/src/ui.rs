//! Turns [`App`] state into a frame. Immediate mode: the whole screen is described on every draw,
//! ratatui then sends only the cells that changed since the previous frame.

use liman_core::format;
use liman_widgets::theme::{BAR_BG, BG, DIM, FG};
use liman_widgets::{DetailedView, Sidebar, breadcrumb, sidebar};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Listing};

/// Below this width the sidebar is hidden so the list keeps enough room.
const SIDEBAR_MIN_WIDTH: u16 = 70;

pub fn render(frame: &mut Frame, app: &mut App) {
    let [top, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Block::new().style(Style::new().bg(BG)), frame.area());
    render_path_bar(frame, app, top);

    let main = if body.width >= SIDEBAR_MIN_WIDTH {
        let [side, main] =
            Layout::horizontal([Constraint::Length(sidebar::WIDTH), Constraint::Fill(1)])
                .areas(body);
        let places = side.inner(Margin::new(0, 1));
        app.sidebar_area = places;
        frame.render_widget(Block::new().style(Style::new().bg(BAR_BG)), side);
        frame.render_widget(Sidebar::new(&app.sidebar, &app.cwd), places);
        main
    } else {
        app.sidebar_area = Rect::default();
        body
    };
    render_list(frame, app, main.inner(Margin::new(2, 1)));
    render_status_bar(frame, app, status);
}

fn render_path_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    app.path_bar_area = area;
    let line = breadcrumb::line(&app.path_segments());
    frame.render_widget(Paragraph::new(line).style(Style::new().bg(BAR_BG)), area);
}

fn render_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let message = match &app.listing {
        Listing::Loading => "Loading…".to_string(),
        Listing::Failed(err) => format!("Cannot open this folder: {err}"),
        Listing::Ready(entries) if entries.is_empty() => "Folder is empty".to_string(),
        Listing::Ready(_) if app.visible.is_empty() => format!("Nothing matches “{}”", app.filter),
        Listing::Ready(entries) => {
            app.list_area = area;
            // Borrow the fields directly (not through a method on `app`) so that `app.table`
            // can be borrowed mutably while `entries` is borrowed immutably.
            let rows: Vec<_> = app.visible.iter().map(|&i| &entries[i]).collect();
            frame.render_stateful_widget(
                DetailedView::new(&rows, format::now()),
                area,
                &mut app.table,
            );
            return;
        }
    };
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(message).centered().fg(DIM), middle);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    if app.filter_editing || !app.filter.is_empty() {
        spans.push(Span::raw(format!(" /{}", app.filter)).fg(FG).bold());
        if app.filter_editing {
            spans.push(Span::raw("▏").fg(FG));
        }
        spans.push(Span::raw("  "));
    }
    if let Some(count) = app.entry_count() {
        spans.push(Span::raw(format!(" {}", format::items(count))).fg(FG));
    }
    if let Some(entry) = app.selected_entry() {
        let size = if entry.is_dir {
            entry.item_count.map(format::items)
        } else {
            Some(format::size(entry.size))
        };
        let size = size.map(|s| format!(" ({s})")).unwrap_or_default();
        spans.push(Span::raw(format!(" | “{}” selected{size}", entry.name)).fg(DIM));
    }
    spans.push(Span::raw("  "));
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!("{message}  ")).fg(FG));
    }
    let hints: &[(&str, &str)] = if app.filter_editing {
        &[("Enter", "keep filter"), ("Esc", "clear")]
    } else {
        &[
            ("Enter", "open"),
            ("⌫", "up"),
            ("~", "home"),
            ("/", "filter"),
            ("q", "quit"),
        ]
    };
    for (key, what) in hints {
        spans.push(Span::raw(format!(" {key} ")).bold());
        spans.push(Span::raw(format!("{what} ")).fg(DIM));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().fg(FG).bg(BAR_BG)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::{Entry, FileType, Places};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    fn app(cwd: &str) -> App {
        let (tx, _rx) = mpsc::channel();
        App::new(
            PathBuf::from(cwd),
            Places::from_user_dirs("", Path::new("/home/test")),
            tx,
        )
    }

    #[test]
    fn shows_path_loading_and_quit_hint() {
        let mut app = app("/home/test/Projects");
        let mut terminal = Terminal::new(TestBackend::new(100, 8)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();

        let rows = rows(&terminal);
        assert!(rows[0].contains("⌂ Home › Projects"));
        assert!(rows.iter().any(|r| r.contains("Loading")));
        assert!(rows[2].contains("⌂ Home")); // sidebar
        assert!(rows[7].contains("q quit"));
    }

    #[test]
    fn narrow_terminal_hides_the_sidebar() {
        let mut app = app("/home/test");
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();
        assert_eq!(app.sidebar_area, Rect::default());
    }

    #[test]
    fn shows_entries_count_and_selection_when_ready() {
        let mut app = app("/home/test");
        app.listing = Listing::Ready(vec![Entry {
            name: "main.rs".into(),
            path: PathBuf::from("main.rs"),
            is_dir: false,
            is_symlink: false,
            special: None,
            size: 13_800,
            item_count: None,
            modified: None,
            file_type: FileType::Code,
        }]);
        app.visible = vec![0];
        app.table.select(Some(0));
        let mut terminal = Terminal::new(TestBackend::new(100, 8)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();

        let rows = rows(&terminal);
        assert!(
            rows.iter()
                .any(|r| r.contains("RS") && r.contains("main.rs") && r.contains("13.8 kB"))
        );
        assert!(rows[7].contains("1 item") && rows[7].contains("“main.rs” selected (13.8 kB)"));
    }
}
