//! Turns [`App`] state into a frame. Immediate mode: the whole screen is described on every draw,
//! ratatui then sends only the cells that changed since the previous frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::App;

const BG: Color = Color::Rgb(21, 22, 28);
const BAR_BG: Color = Color::Rgb(32, 33, 41);
const FG: Color = Color::Rgb(230, 230, 235);
const DIM: Color = Color::Rgb(150, 150, 160);

pub fn render(frame: &mut Frame, app: &App) {
    let [top, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Block::new().style(Style::new().bg(BG)), frame.area());

    let path = Line::from(vec![
        Span::raw(" liman ").bold(),
        Span::raw("› ").fg(DIM),
        Span::raw(app.cwd.display().to_string()),
    ]);
    frame.render_widget(
        Paragraph::new(path).style(Style::new().fg(FG).bg(BAR_BG)),
        top,
    );

    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(body);
    frame.render_widget(
        Paragraph::new("skeleton is running").centered().fg(DIM),
        middle,
    );

    let status_line = Line::from(vec![
        Span::raw(" q ").bold(),
        Span::raw("quit  ").fg(DIM),
        Span::raw(app.last_input.as_str()).fg(DIM),
    ]);
    frame.render_widget(
        Paragraph::new(status_line).style(Style::new().fg(FG).bg(BAR_BG)),
        status,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    #[test]
    fn shows_path_and_quit_hint() {
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        let app = App::new(PathBuf::from("/home/test"));
        terminal.draw(|f| render(f, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        let row = |y: u16| -> String { (0..40).map(|x| buffer[(x, y)].symbol()).collect() };
        assert!(row(0).contains("liman"));
        assert!(row(0).contains("/home/test"));
        assert!(row(4).contains("q quit"));
    }
}
