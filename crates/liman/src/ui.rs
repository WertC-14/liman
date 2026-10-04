//! Turns [`App`] state into a frame. Immediate mode: the whole screen is described on every draw,
//! ratatui then sends only the cells that changed since the previous frame.

use liman_core::format;
use liman_widgets::theme;
use liman_widgets::{FileList, GridView, ListMode, Sidebar, breadcrumb, file_list, grid, sidebar};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::{App, ClipMode, Focus, Listing, TermMode, View};

/// Below this width the sidebar is hidden so the list keeps enough room.
const SIDEBAR_MIN_WIDTH: u16 = 70;

pub fn render(frame: &mut Frame, app: &mut App) {
    render_screen(frame, app);
    if app.theme_picker.is_some() {
        render_theme_picker(frame, app);
    } else if app.help_open {
        render_help(frame);
    }
}

/// A centered box of `width` × `height` cells with a rounded accent frame; returns the inside.
fn popup(frame: &mut Frame, title: &str, width: u16, height: u16) -> Rect {
    let area = frame.area();
    let (w, h) = (width.min(area.width), height.min(area.height));
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    frame.render_widget(Clear, rect);
    let block = panel(title, true);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    inner
}

fn render_theme_picker(frame: &mut Frame, app: &mut App) {
    let themes = &liman_widgets::theme::THEMES;
    let inner = popup(
        frame,
        " Theme · ↑↓ preview · Enter keep · Esc cancel ",
        56,
        themes.len() as u16 + 4,
    );
    let list = inner.inner(Margin::new(1, 1));
    app.picker_area = list;
    let selected = app.theme_picker.map_or(0, |(s, _)| s);
    for (i, t) in themes.iter().enumerate() {
        let mut spans = vec![
            Span::raw(if i == selected { " ▶ " } else { "   " }).fg(theme::accent()),
            Span::raw(format!("{:<12}", t.name)).fg(theme::fg()).bold(),
        ];
        // swatches on the theme's own background
        spans.push(Span::raw(" ").bg(t.bg));
        for c in t.types.iter().take(9) {
            spans.push(Span::raw("■ ").fg(*c).bg(t.bg));
        }
        let mut line = Line::from(spans);
        if i == selected {
            line = line.style(Style::new().bg(theme::selected_bg()));
        }
        let row = Rect::new(list.x, list.y + i as u16, list.width, 1);
        frame.render_widget(Paragraph::new(line), row.intersection(list));
    }
}

fn render_help(frame: &mut Frame) {
    const KEYS: &[(&str, &str)] = &[
        ("Enter / double click", "open"),
        ("Bksp / Alt+↑", "parent folder"),
        ("Alt+← / Alt+→", "back / forward"),
        ("Tab / Shift+Tab", "Places · Files · Terminal"),
        ("v", "small list ↔ large view"),
        ("+ / -  (Ctrl+wheel)", "zoom"),
        ("/", "filter"),
        ("Ctrl+H / .", "hidden files"),
        ("s / S  (header click)", "sort by / reverse"),
        ("Space / Ctrl+A", "mark / mark all"),
        ("Ctrl+C  Ctrl+X  Ctrl+V", "copy  cut  paste"),
        ("Del / F2 / Ctrl+Z", "trash / rename / undo"),
        ("F4 / Ctrl+O", "terminal panel / full screen"),
        ("Ctrl+↑ / Ctrl+↓", "terminal size"),
        ("t", "theme"),
        ("~", "home"),
        ("q", "quit"),
    ];
    let inner = popup(frame, " Keys · any key closes ", 62, KEYS.len() as u16 + 4);
    let lines: Vec<Line> = KEYS
        .iter()
        .map(|(k, what)| {
            Line::from(vec![
                Span::raw(format!(" {k:<24}")).fg(theme::accent()).bold(),
                Span::raw(*what).fg(theme::fg()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner.inner(Margin::new(1, 1)));
}

fn render_screen(frame: &mut Frame, app: &mut App) {
    let [top, body, status] = Layout::vertical([
        Constraint::Length(3), // framed title bar with the path chips
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(
        Block::new().style(Style::new().bg(theme::bg())),
        frame.area(),
    );

    if app.term_mode == TermMode::Fullscreen {
        render_terminal(frame, app, top.union(body));
        render_status_bar(frame, app, status);
        return;
    }
    render_path_bar(frame, app, top);

    // F4 panel: full width under the files, like Dolphin.
    let body = if app.term_mode == TermMode::Panel {
        let height = app
            .term_height
            .unwrap_or(body.height * 2 / 5)
            .max(4)
            .min(body.height.saturating_sub(4));
        let [files, term] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(height)]).areas(body);
        render_terminal(frame, app, term);
        files
    } else {
        app.term_area = Rect::default();
        body
    };

    let main = if body.width >= SIDEBAR_MIN_WIDTH {
        let [side, main] =
            Layout::horizontal([Constraint::Length(sidebar::WIDTH + 2), Constraint::Fill(1)])
                .areas(body);
        let places_focused = app.focus == Focus::Places;
        let block = panel(" Places ", places_focused);
        let places = block.inner(side).inner(Margin::new(0, 1));
        frame.render_widget(block, side);
        app.sidebar_area = places;
        let mut sidebar = Sidebar::new(&app.sidebar, &app.cwd);
        if places_focused {
            sidebar = sidebar.focused(app.sidebar_selected);
        }
        frame.render_widget(sidebar, places);
        main
    } else {
        app.sidebar_area = Rect::default();
        body
    };
    let files_focused = app.focus == Focus::Files;
    let title = match (&app.results, app.entry_count()) {
        (Some(_), Some(n)) => format!(" Results · {} ", format::items(n)),
        (None, Some(n)) => format!(" {} · {} ", folder_name(app), format::items(n)),
        _ => format!(" {} ", folder_name(app)),
    };
    let block = panel(&title, files_focused);
    let inner = block.inner(main);
    frame.render_widget(block, main);
    let main = inner;
    render_list(frame, app, main.inner(Margin::new(2, 1)));
    render_status_bar(frame, app, status);
}

/// A rounded panel with a title; the focused one gets the accent color (style A, fm-research LOG).
fn panel(title: &str, focused: bool) -> Block<'static> {
    let accent = liman_widgets::theme::type_color(liman_core::FileType::Folder);
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { accent } else { theme::border() }))
        .title(
            Span::raw(title.to_string())
                .fg(if focused { theme::fg() } else { theme::dim() })
                .bold(),
        )
        .style(Style::new().bg(theme::bg()))
}

fn folder_name(app: &App) -> String {
    app.cwd
        .file_name()
        .map_or_else(|| "/".to_string(), |n| n.to_string_lossy().into_owned())
}

fn render_path_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = panel(" liman ", false);
    let area = block.inner(area);
    frame.render_widget(
        block,
        Rect::new(area.x - 1, area.y - 1, area.width + 2, area.height + 2),
    );
    app.path_bar_area = area;
    if let Some(results) = &app.results {
        let accent = liman_widgets::theme::type_color(liman_core::FileType::Folder);
        let line = Line::from(vec![
            Span::raw(" ⌕ ").fg(accent).bold(),
            Span::raw(results.command.clone()).fg(theme::fg()).bold(),
            Span::raw(format!(
                "  ·  {} found in {}",
                format::items(results.count),
                app.cwd.display()
            ))
            .fg(theme::dim()),
            Span::raw("   Bksp/Esc back to the folder").fg(theme::dim()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    let line = breadcrumb::line(&app.path_segments());
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(theme::bar_bg())),
        area,
    );
}

fn render_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let message = match &app.listing {
        Listing::Loading => "Loading…".to_string(),
        Listing::Failed(err) => format!("Cannot open this folder: {err}"),
        Listing::Ready(entries) if entries.is_empty() => "Folder is empty".to_string(),
        Listing::Ready(_) if app.visible.is_empty() => format!("Nothing matches “{}”", app.filter),
        Listing::Ready(entries) => {
            app.list_area = area;
            app.drawn_view = fitting_view(app.view, area);
            // Borrow the fields directly (not through a method on `app`) so that `app.table`
            // can be borrowed mutably while `entries` is borrowed immutably.
            let rows: Vec<_> = app.visible.iter().map(|&i| &entries[i]).collect();
            match app.drawn_view.list_mode() {
                Some(mode) => frame.render_stateful_widget(
                    FileList::new(&rows, format::now(), mode)
                        .marked(&app.marked)
                        .sort(app.sort),
                    area,
                    &mut app.table,
                ),
                None => {
                    let wanted = app.grid_level.unwrap_or(grid::DEFAULT_LEVEL);
                    app.drawn_grid_level = grid::fitting_level(wanted, area);
                    frame.render_stateful_widget(
                        GridView::new(&rows, app.drawn_grid_level).marked(&app.marked),
                        area,
                        &mut app.table,
                    );
                }
            }
            return;
        }
    };
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(message).centered().fg(theme::dim()), middle);
}

/// Draws the shell's screen (vt100 cells) inside a rounded frame; blue frame = keys go to the shell.
fn render_terminal(frame: &mut Frame, app: &mut App, area: Rect) {
    app.term_area = area;
    let focused = app.focus == Focus::Terminal || app.term_mode == TermMode::Fullscreen;
    let accent = if focused {
        liman_widgets::theme::type_color(liman_core::FileType::Folder)
    } else {
        theme::dim()
    };
    let hint = match app.term_mode {
        TermMode::Fullscreen => " Terminal · Ctrl+O back to files ",
        _ if focused => {
            " Terminal · Tab on empty line: next panel · Ctrl+↑↓ size · F4 close · Ctrl+O full screen "
        }
        _ => " Terminal · Tab or click to type ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .title(Span::raw(hint).fg(if focused { theme::fg() } else { theme::dim() }));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(term) = &mut app.terminal else {
        return;
    };
    term.resize(inner.height, inner.width);
    let screen = term.screen();
    let buf = frame.buffer_mut();
    for row in 0..inner.height {
        for col in 0..inner.width {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue; // covered by the wide character to its left
            }
            let mut style = Style::new()
                .fg(term_color(cell.fgcolor(), theme::fg()))
                .bg(term_color(cell.bgcolor(), theme::bg()));
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let symbol = if cell.has_contents() {
                cell.contents()
            } else {
                " "
            };
            buf[(inner.x + col, inner.y + row)]
                .set_symbol(symbol)
                .set_style(style);
        }
    }
    if focused && !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        frame.set_cursor_position((inner.x + col, inner.y + row));
    }
}

fn term_color(color: vt100::Color, default: Color) -> Color {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Falls back to a smaller view when the wanted one does not fit: the grid needs one whole tile,
/// boxes need room for two entries.
fn fitting_view(wanted: View, area: Rect) -> View {
    let grid_fits = area.width >= grid::MIN_TILE.0 && area.height >= grid::MIN_TILE.1;
    let boxes_fit = area.height >= file_list::HEADER_HEIGHT + 2 * ListMode::Normal.row_height()
        && area.width >= 50;
    match wanted {
        View::Grid if grid_fits => View::Grid,
        View::Grid | View::Normal if boxes_fit => View::Normal,
        _ => View::Detailed,
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    if app.term_mode != TermMode::Hidden && app.focus == Focus::Terminal
        || app.term_mode == TermMode::Fullscreen
    {
        spans.push(Span::raw(" ⌂ ").fg(theme::dim()));
        spans.push(Span::raw(app.cwd.display().to_string()).fg(theme::fg()));
        for (key, what) in [
            ("Ctrl+O", "files / full screen"),
            ("F6", "focus"),
            ("F4", "panel"),
        ] {
            spans.push(Span::raw(format!("  {key} ")).bold());
            spans.push(Span::raw(what).fg(theme::dim()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(Style::new().fg(theme::fg()).bg(theme::bar_bg())),
            area,
        );
        return;
    }
    if let Some(input) = &app.rename {
        spans.push(Span::raw(" Rename: ").fg(theme::dim()));
        spans.push(Span::raw(input.text.as_str()).fg(theme::fg()).bold());
        spans.push(Span::raw("▏  ").fg(theme::fg()));
        spans.push(Span::raw(" Enter ").bold());
        spans.push(Span::raw("rename ").fg(theme::dim()));
        spans.push(Span::raw(" Esc ").bold());
        spans.push(Span::raw("cancel").fg(theme::dim()));
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(Style::new().fg(theme::fg()).bg(theme::bar_bg())),
            area,
        );
        return;
    }
    if let Some(job) = &app.job {
        let percent = (job.done * 100)
            .checked_div(job.total)
            .unwrap_or(0)
            .min(100);
        spans.push(
            Span::raw(format!(" {}… {percent}%  ", job.label))
                .fg(theme::fg())
                .bold(),
        );
    }
    if app.filter_editing || !app.filter.is_empty() {
        spans.push(
            Span::raw(format!(" /{}", app.filter))
                .fg(theme::fg())
                .bold(),
        );
        if app.filter_editing {
            spans.push(Span::raw("▏").fg(theme::fg()));
        }
        spans.push(Span::raw("  "));
    }
    if let Some(count) = app.entry_count() {
        spans.push(Span::raw(format!(" {}", format::items(count))).fg(theme::fg()));
    }
    if let Some(entry) = app.selected_entry() {
        let size = if entry.is_dir {
            entry.item_count.map(format::items)
        } else {
            Some(format::size(entry.size))
        };
        let size = size.map(|s| format!(" ({s})")).unwrap_or_default();
        spans.push(Span::raw(format!(" | “{}” selected{size}", entry.name)).fg(theme::dim()));
    }
    if !app.marked.is_empty() {
        spans.push(Span::raw(format!(" | {} marked", app.marked.len())).fg(theme::fg()));
    }
    if let Some(clip) = &app.clipboard {
        let verb = match clip.mode {
            ClipMode::Copy => "copied",
            ClipMode::Cut => "cut",
        };
        spans.push(
            Span::raw(format!(" | {} {verb}", format::items(clip.paths.len()))).fg(theme::dim()),
        );
    }
    let view = match app.drawn_view {
        View::Grid => format!("Grid {}", app.drawn_grid_level + 1),
        other => other.name().to_string(),
    };
    spans.push(Span::raw(format!(" | {view} view")).fg(theme::dim()));
    spans.push(Span::raw("  "));
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!("{message}  ")).fg(theme::fg()));
    }
    let hints: &[(&str, &str)] = if app.filter_editing {
        &[("Enter", "keep filter"), ("Esc", "clear")]
    } else {
        &[
            ("Enter", "open"),
            ("Bksp", "up"),
            ("Tab", "panels"),
            ("v", "small/large"),
            ("t", "theme"),
            ("?", "all keys"),
            ("q", "quit"),
        ]
    };
    for (key, what) in hints {
        spans.push(Span::raw(format!(" {key} ")).bold());
        spans.push(Span::raw(format!("{what} ")).fg(theme::dim()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().fg(theme::fg()).bg(theme::bar_bg())),
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
        let mut terminal = Terminal::new(TestBackend::new(140, 14)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();

        let rows = rows(&terminal);
        assert!(rows[0].contains("liman")); // framed title bar
        assert!(rows[1].contains(" ⌂ Home  ›  Projects ")); // path chips
        assert!(rows.iter().any(|r| r.contains("Loading")));
        assert!(rows[3].contains("Places") && rows[3].contains("Projects")); // panel titles
        assert!(rows[5].contains("⌂ Home")); // sidebar
        assert!(rows[13].contains("? all keys"));
    }

    #[test]
    fn normal_view_falls_back_to_detailed_when_too_small() {
        assert_eq!(
            fitting_view(View::Normal, Rect::new(0, 0, 80, 30)),
            View::Normal
        );
        assert_eq!(
            fitting_view(View::Normal, Rect::new(0, 0, 80, 10)),
            View::Detailed
        );
        assert_eq!(
            fitting_view(View::Normal, Rect::new(0, 0, 40, 30)),
            View::Detailed
        );
        assert_eq!(
            fitting_view(View::Grid, Rect::new(0, 0, 80, 30)),
            View::Grid
        );
        assert_eq!(
            fitting_view(View::Grid, Rect::new(0, 0, 80, 6)),
            View::Detailed
        );
        assert_eq!(
            fitting_view(View::Grid, Rect::new(0, 0, 15, 30)),
            View::Detailed
        );
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
        let mut terminal = Terminal::new(TestBackend::new(140, 14)).unwrap();
        terminal.draw(|f| render(f, &mut app)).unwrap();

        let rows = rows(&terminal);
        assert!(
            rows.iter()
                .any(|r| r.contains("RS") && r.contains("main.rs") && r.contains("13.8 kB"))
        );
        assert!(rows[13].contains("1 item") && rows[13].contains("“main.rs” selected (13.8 kB)"));
    }
}
