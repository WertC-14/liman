//! Turns [`App`] state into a frame. Immediate mode: the whole screen is described on every draw,
//! ratatui then sends only the cells that changed since the previous frame.

use liman_core::format;
use liman_widgets::DetailedView;
use liman_widgets::theme::{BAR_BG, BG, DIM, FG};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Listing};

pub fn render(frame: &mut Frame, app: &mut App) {
    let [top, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Block::new().style(Style::new().bg(BG)), frame.area());
    render_top_bar(frame, app, top);
    render_body(frame, app, body.inner(Margin::new(2, 1)));
    render_status_bar(frame, app, status);
}

fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let path = Line::from(vec![
        Span::raw(" liman ").bold(),
        Span::raw("› ").fg(DIM),
        Span::raw(app.cwd.display().to_string()),
    ]);
    frame.render_widget(
        Paragraph::new(path).style(Style::new().fg(FG).bg(BAR_BG)),
        area,
    );
}

fn render_body(frame: &mut Frame, app: &mut App, area: Rect) {
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
        spans.push(Span::raw(format!(" {}  ", format::items(count))).fg(FG));
    }
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!("{message}  ")).fg(FG));
    }
    let hints: &[(&str, &str)] = if app.filter_editing {
        &[("Enter", "keep filter"), ("Esc", "clear")]
    } else {
        &[
            ("↑↓", "move"),
            ("Enter", "open"),
            ("⌫", "up"),
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
    use liman_core::{Entry, FileType};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;
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

    #[test]
    fn shows_path_loading_and_quit_hint() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/home/test"), tx);
        let mut terminal = Terminal::new(TestBackend::new(50, 7)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();

        let rows = rows(&terminal);
        assert!(rows[0].contains("liman") && rows[0].contains("/home/test"));
        assert!(rows.iter().any(|r| r.contains("Loading")));
        assert!(rows[6].contains("q quit"));
        assert!(rows[6].contains("/ filter"));
    }

    #[test]
    fn shows_entries_and_count_when_ready() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/home/test"), tx);
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
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();

        let rows = rows(&terminal);
        assert!(
            rows.iter()
                .any(|r| r.contains("RS") && r.contains("main.rs") && r.contains("13.8 kB"))
        );
        assert!(rows[7].contains("1 item"));
    }
}
