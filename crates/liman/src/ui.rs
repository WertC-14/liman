//! Turns [`App`] state into a frame. Immediate mode: the whole screen is described on every draw,
//! ratatui then sends only the cells that changed since the previous frame.

use liman_core::format;
use liman_core::i18n::{tr, trf};
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
    if let Some(dialog) = &app.dialog {
        render_dialog(frame, dialog);
    } else if app.branch_picker.is_some() {
        render_branch_picker(frame, app);
    } else if app.git_panel.is_some() {
        render_git_panel(frame, app);
    } else if app.menu.is_some() {
        render_menu(frame, app);
    } else if app.theme_picker.is_some() {
        render_theme_picker(frame, app);
    } else if app.help_open {
        render_help(frame);
    }
    if !liman_widgets::colors::truecolor() {
        liman_widgets::colors::downsample(frame.buffer_mut());
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
        tr(" Theme · ↑↓ preview · Enter keep · Esc cancel "),
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

fn render_dialog(frame: &mut Frame, dialog: &crate::app::Dialog) {
    use crate::app::Dialog;
    let (title, lines): (&str, Vec<Line>) = match dialog {
        Dialog::Conflict { existing } => {
            let mut lines = vec![
                Line::from(trf("{} already here:", &[&format::items(existing.len())]))
                    .fg(theme::fg()),
            ];
            for path in existing.iter().take(5) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                lines.push(Line::from(format!("  • {name}")).fg(theme::dim()));
            }
            if existing.len() > 5 {
                lines.push(
                    Line::from(trf("  … and {} more", &[&(existing.len() - 5)])).fg(theme::dim()),
                );
            }
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::raw(" b ").bold().fg(theme::accent()),
                Span::raw(tr("keep both (Enter)   ")).fg(theme::fg()),
                Span::raw(" r ").bold().fg(theme::accent()),
                Span::raw(tr("replace (old to trash)   ")).fg(theme::fg()),
                Span::raw(" s ").bold().fg(theme::accent()),
                Span::raw(tr("skip   ")).fg(theme::fg()),
                Span::raw(" Esc ").bold().fg(theme::dim()),
            ]));
            (tr(" Paste "), lines)
        }
        Dialog::ConfirmDiscard { paths } => {
            let lines = vec![
                Line::from(trf(
                    "Throw away the changes in {}?",
                    &[&format::items(paths.len())],
                ))
                .fg(theme::fg())
                .bold(),
                Line::from(tr(
                    "Tracked files go back to the last commit; new files go to the trash.",
                ))
                .fg(theme::dim()),
                Line::default(),
                Line::from(vec![
                    Span::raw(" y ")
                        .bold()
                        .fg(theme::type_color(liman_core::FileType::Pdf)),
                    Span::raw(tr("discard   ")).fg(theme::fg()),
                    Span::raw(" n / Esc ").bold().fg(theme::accent()),
                    Span::raw("keep").fg(theme::fg()),
                ]),
            ];
            (tr(" Git: discard "), lines)
        }
        Dialog::ConfirmDelete { paths } => {
            let what = match paths.as_slice() {
                [one] => format!(
                    "“{}”",
                    one.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                many => format::items(many.len()),
            };
            let lines = vec![
                Line::from(trf("Delete {} for good?", &[&what]))
                    .fg(theme::fg())
                    .bold(),
                Line::from(tr(
                    "This cannot be undone (Del moves to the trash instead).",
                ))
                .fg(theme::dim()),
                Line::default(),
                Line::from(vec![
                    Span::raw(" y ")
                        .bold()
                        .fg(theme::type_color(liman_core::FileType::Pdf)),
                    Span::raw(tr("delete   ")).fg(theme::fg()),
                    Span::raw(" n / Esc ").bold().fg(theme::accent()),
                    Span::raw("keep").fg(theme::fg()),
                ]),
            ];
            (tr(" Delete for good "), lines)
        }
    };
    let height = lines.len() as u16 + 4;
    let inner = popup(frame, title, 72, height);
    frame.render_widget(Paragraph::new(lines), inner.inner(Margin::new(1, 1)));
}

/// Ctrl+G: changed files on the left (status letters, path in the repository), the selected file's
/// diff on the right. Keys are listed in the frame title.
fn render_git_panel(frame: &mut Frame, app: &mut App) {
    app.git_panel_diff();
    let (Some(git), Some(panel)) = (&app.git, &mut app.git_panel) else {
        return;
    };
    let screen = frame.area();
    let (w, h) = (
        screen.width.saturating_sub(6).max(40),
        screen.height.saturating_sub(4).max(10),
    );
    // "⎇ main ↑2 → origin/main · github.com/user/repo": what p pushes and where.
    let target = match (&git.upstream, &panel.remote) {
        (Some(up), Some((_, url))) => {
            format!(" → {up} · {}", liman_core::git::short_url(url))
        }
        (None, Some((name, url))) => format!(
            " → {name} ({}) · {}",
            tr("new branch"),
            liman_core::git::short_url(url)
        ),
        (_, None) => format!(" · {}", tr("no remote")),
    };
    let title = format!(" Git · ⎇ {}{target} ", git.summary());
    let inner = popup(frame, &title, w, h);
    let [body, keys] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(
            tr(" Space stage/unstage  a stage all  c commit  d discard  p push  P pull  b branch  Enter show  Esc close"),
        )
        .fg(theme::dim()),
        keys,
    );
    let [list_area, diff_area] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Fill(1)])
            .spacing(1)
            .areas(body.inner(Margin::new(1, 1)));

    if git.files.is_empty() {
        frame.render_widget(
            Paragraph::new(tr("Working tree clean")).fg(theme::dim()),
            list_area,
        );
        return;
    }
    panel.selected = panel.selected.min(git.files.len() - 1);
    let visible = usize::from(list_area.height);
    let start = panel.selected.saturating_sub(visible.saturating_sub(1));
    let lines: Vec<Line> = git
        .files
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(i, (path, mark))| {
            let rel = path
                .strip_prefix(&git.root)
                .unwrap_or(path)
                .display()
                .to_string();
            let mut spans = liman_widgets::gitmark::spans(Some(*mark));
            spans.push(Span::raw(rel).fg(theme::fg()));
            let line = Line::from(spans);
            if i == panel.selected {
                line.style(Style::new().bg(theme::selected_bg()))
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);

    let added = theme::type_color(liman_core::FileType::Spreadsheet);
    let removed = theme::type_color(liman_core::FileType::Pdf);
    let hunk = theme::type_color(liman_core::FileType::Document);
    panel.scroll = panel.scroll.min(panel.diff.len().saturating_sub(1));
    let diff: Vec<Line> = panel
        .diff
        .iter()
        .skip(panel.scroll)
        .take(usize::from(diff_area.height))
        .map(|l| {
            let color = match l.chars().next() {
                Some('+') if !l.starts_with("+++") => added,
                Some('-') if !l.starts_with("---") => removed,
                Some('@') => hunk,
                _ => theme::dim(),
            };
            Line::from(Span::raw(l.clone()).fg(color))
        })
        .collect();
    frame.render_widget(Clear, diff_area);
    frame.render_widget(Paragraph::new(diff).bg(theme::bg()), diff_area);
}

fn render_branch_picker(frame: &mut Frame, app: &App) {
    let Some((branches, selected)) = &app.branch_picker else {
        return;
    };
    let inner = popup(
        frame,
        tr(" Switch branch · Enter switch · Esc "),
        48,
        branches.len().min(16) as u16 + 4,
    );
    let lines: Vec<Line> = branches
        .iter()
        .enumerate()
        .take(16)
        .map(|(i, b)| {
            let line = Line::from(format!(" ⎇ {b}")).fg(theme::fg());
            if i == *selected {
                line.style(Style::new().bg(theme::selected_bg())).bold()
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner.inner(Margin::new(1, 1)));
}

/// Command palette (centered, with a search line) or right-click menu (at the mouse).
fn render_menu(frame: &mut Frame, app: &mut App) {
    let availability: Vec<bool> = app
        .menu
        .as_ref()
        .map(|m| m.items.iter().map(|a| app.action_available(*a)).collect())
        .unwrap_or_default();
    let Some(menu) = &mut app.menu else {
        return;
    };
    const ROWS: usize = 14;
    let (list, area) = if let Some(query) = &menu.query {
        let inner = popup(
            frame,
            tr(" Commands · type to search · Enter run · Esc close "),
            60,
            ROWS as u16 + 5,
        );
        let input = Rect::new(inner.x + 1, inner.y + 1, inner.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("> ").fg(theme::accent()).bold(),
                Span::raw(query.clone()).fg(theme::fg()).bold(),
                Span::raw("▏").fg(theme::fg()),
            ])),
            input,
        );
        let list = Rect::new(
            inner.x + 1,
            inner.y + 3,
            inner.width.saturating_sub(2),
            ROWS as u16,
        );
        (list, inner)
    } else {
        let screen = frame.area();
        let (w, h) = (40, menu.items.len() as u16 + 2);
        let x = menu.at.0.min(screen.right().saturating_sub(w));
        let y = menu.at.1.min(screen.bottom().saturating_sub(h));
        let rect = Rect::new(x, y, w, h);
        frame.render_widget(Clear, rect);
        let block = panel("", true);
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        (inner, inner)
    };
    let _ = area;
    // Keep the selection in view.
    let visible = usize::from(list.height);
    if menu.selected < menu.offset {
        menu.offset = menu.selected;
    } else if menu.selected >= menu.offset + visible {
        menu.offset = menu.selected + 1 - visible;
    }
    app.menu_area = list;
    for (row, (i, action)) in menu
        .items
        .iter()
        .enumerate()
        .skip(menu.offset)
        .take(visible)
        .enumerate()
    {
        let available = availability.get(i).copied().unwrap_or(true);
        let color = if available { theme::fg() } else { theme::dim() };
        let keys = action.keys();
        let width = usize::from(list.width);
        let label_width = width.saturating_sub(keys.chars().count() + 2);
        let mut line = Line::from(vec![
            Span::raw(format!(" {:<label_width$}", action.label())).fg(color),
            Span::raw(format!("{keys} ")).fg(theme::dim()),
        ]);
        if i == menu.selected {
            line = line.style(Style::new().bg(theme::selected_bg())).bold();
        }
        let rect = Rect::new(list.x, list.y + row as u16, list.width, 1);
        frame.render_widget(Paragraph::new(line), rect);
    }
    if menu.items.is_empty() {
        frame.render_widget(
            Paragraph::new(tr(" No matching command")).fg(theme::dim()),
            list,
        );
    }
}

/// The `?` window: keys and what they do (English; shown through `tr`).
const HELP_KEYS: &[(&str, &str)] = &[
    ("Enter / double click", "open"),
    ("Bksp / Alt+← / Alt+↑", "parent folder"),
    ("Alt+→ / Alt+↓", "into the folder / open"),
    ("Ctrl+← / Ctrl+→", "back / forward"),
    ("Tab / Shift+Tab", "Places · Files · Terminal"),
    ("v", "small list ↔ large view"),
    ("+ / -  (Ctrl+wheel)", "zoom"),
    ("/", "filter"),
    ("Ctrl+F", "search in subfolders"),
    ("Ctrl+D", "bookmark folder (again: remove)"),
    ("Ctrl+H / .", "hidden files"),
    ("s / S  (header click)", "sort by / reverse"),
    ("Space / Ctrl+A", "mark / mark all"),
    ("Ctrl+click / Shift+click", "mark one / mark a range"),
    ("Shift+arrows / Home / End", "mark a range"),
    ("Ctrl+Space", "mark one, stay in place"),
    ("drag onto a folder", "move (hold Ctrl: copy)"),
    ("Ctrl+C  Ctrl+X  Ctrl+V", "copy  cut  paste"),
    ("Del / F2 / Ctrl+Z", "trash / rename / undo"),
    ("Shift+Del", "delete for good (asks first)"),
    ("Ctrl+T / Ctrl+W", "new tab / close tab"),
    ("Alt+1…9 / wheel on the tabs", "go to tab"),
    ("F3", "preview panel"),
    ("r", "read the file full screen"),
    ("F4 / Ctrl+O", "terminal panel / full screen"),
    ("Alt+Enter", "selected paths into the terminal"),
    ("Ctrl+↑ / Ctrl+↓", "terminal size"),
    ("t", "theme"),
    ("Ctrl+P / right click", "all commands / menu"),
    ("Ctrl+N / Alt+C", "new folder / copy path (works over SSH)"),
    (
        "Ctrl+G",
        "git: changes, diff, stage, commit, push, pull, branch",
    ),
    ("~", "home"),
    ("q", "quit"),
];

fn render_help(frame: &mut Frame) {
    let inner = popup(
        frame,
        tr(" Keys · any key closes "),
        84,
        HELP_KEYS.len() as u16 + 4,
    );
    let lines: Vec<Line> = HELP_KEYS
        .iter()
        .map(|(k, what)| {
            Line::from(vec![
                Span::raw(format!(" {:<26}", tr(k)))
                    .fg(theme::accent())
                    .bold(),
                Span::raw(tr(what)).fg(theme::fg()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner.inner(Margin::new(1, 1)));
}

fn render_screen(frame: &mut Frame, app: &mut App) {
    frame.render_widget(
        Block::new().style(Style::new().bg(theme::bg())),
        frame.area(),
    );
    // Tab row (ADR 0008, sketch B): only with two or more tabs.
    let labels = app.tab_labels();
    let screen = if labels.is_empty() {
        app.tabs.chip_areas.clear();
        app.tabs.plus_area = Rect::default();
        frame.area()
    } else {
        let [row, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(frame.area());
        render_tab_row(frame, app, &labels, row);
        rest
    };
    let [top, body, status] = Layout::vertical([
        Constraint::Length(3), // framed title bar with the path chips
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(screen);

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

    // `r` / Enter in the preview: the file fills the space of the sidebar and the list.
    if app.preview.reader {
        app.sidebar_area = Rect::default();
        app.list_area = Rect::default();
        let title = tr(" Reader · ↑↓ PgUp/PgDn scroll · Enter back to the panel · Esc close ");
        render_preview(frame, app, body, title, true);
        render_status_bar(frame, app, status);
        return;
    }

    let main = if body.width >= SIDEBAR_MIN_WIDTH {
        let [side, main] =
            Layout::horizontal([Constraint::Length(sidebar::WIDTH + 2), Constraint::Fill(1)])
                .areas(body);
        let places_focused = app.focus == Focus::Places;
        let block = panel(tr(" Places "), places_focused);
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
    // F3: preview on the right, if the files keep enough room.
    let (main, preview_area) = if app.preview.shown && main.width >= 60 {
        let [files, preview] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Fill(1)]).areas(main);
        (files, Some(preview))
    } else {
        (main, None)
    };
    let files_focused = app.focus == Focus::Files;
    let title = match (&app.results, app.entry_count()) {
        (Some(_), Some(n)) => trf(" Results · {} ", &[&format::items(n)]),
        (None, Some(n)) => format!(" {} · {} ", folder_name(app), format::items(n)),
        _ => format!(" {} ", folder_name(app)),
    };
    let block = panel(&title, files_focused);
    let inner = block.inner(main);
    frame.render_widget(block, main);
    let main = inner;
    render_list(frame, app, main.inner(Margin::new(2, 1)));
    match preview_area {
        Some(area) => {
            let focused = app.focus == Focus::Preview;
            render_preview(frame, app, area, tr(" Preview "), focused)
        }
        None => app.preview.area = Rect::default(),
    }
    render_status_bar(frame, app, status);
}

fn render_preview(frame: &mut Frame, app: &mut App, area: Rect, title: &str, focused: bool) {
    let block = panel(title, focused);
    let inner = block.inner(area).inner(Margin::new(1, 1));
    frame.render_widget(block, area);
    app.preview.area = inner;
    app.want_preview(
        inner.width,
        inner
            .height
            .saturating_sub(liman_widgets::preview::HEADER_ROWS),
    );
    if let Some(preview) = &app.preview.current {
        frame.render_widget(
            liman_widgets::preview::PreviewView::new(preview).scroll(app.preview.scroll),
            inner,
        );
    }
}

/// `  1 ⌂ Ev    2 liman ●    3 Downloads    +` — the active tab is a filled chip.
fn render_tab_row(frame: &mut Frame, app: &mut App, labels: &[crate::app::TabLabel], area: Rect) {
    let accent = theme::accent();
    let mut x = area.x + 1;
    app.tabs.chip_areas.clear();
    for (i, label) in labels.iter().enumerate() {
        let shell = if label.has_shell { " ●" } else { "" };
        let text = format!(" {} {}{shell} ", i + 1, label.title);
        let width = (text.chars().count() as u16).min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let style = if label.active {
            Style::new().bg(accent).fg(theme::text_on(accent)).bold()
        } else {
            Style::new().fg(theme::dim())
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        app.tabs.chip_areas.push(rect);
        x += width + 2;
    }
    app.tabs.plus_area = Rect::default();
    if x + 3 <= area.right() {
        let rect = Rect::new(x, area.y, 3, 1);
        frame.render_widget(Paragraph::new(" + ").fg(theme::dim()), rect);
        app.tabs.plus_area = rect;
    }
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
            Span::raw(trf(
                "  ·  {} found in {}",
                &[&format::items(results.count), &app.cwd.display()],
            ))
            .fg(theme::dim()),
            Span::raw(tr("   Bksp/Esc back to the folder")).fg(theme::dim()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    frame.render_widget(
        Paragraph::new("").style(Style::new().bg(theme::bar_bg())),
        area,
    );
    // Git branch on the right: "⎇ main ↑1 ↓2" plus a count of changed files.
    // The path gets the rest of the row so the two never overlap.
    let mut path_area = area;
    if let Some(git) = &app.git {
        let changed = git.files.len();
        let text = if changed > 0 {
            trf(" ⎇ {} · {} changed  Ctrl+G ", &[&git.summary(), &changed])
        } else {
            format!(" ⎇ {}  Ctrl+G ", git.summary())
        };
        let width = text.chars().count() as u16;
        if area.width > width + 20 {
            let rect = Rect::new(area.right() - width, area.y, width, 1);
            path_area.width -= width + 1;
            let color = if changed > 0 {
                theme::type_color(liman_core::FileType::Presentation)
            } else {
                theme::type_color(liman_core::FileType::Spreadsheet)
            };
            frame.render_widget(Paragraph::new(Span::raw(text).fg(color).bold()), rect);
        }
    }
    let line = breadcrumb::line(&app.path_segments());
    frame.render_widget(Paragraph::new(line), path_area);
}

fn render_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let message = match &app.listing {
        Listing::Loading => tr("Loading…").to_string(),
        Listing::Failed(err) => trf("Cannot open this folder: {}", &[err]),
        Listing::Ready(entries) if entries.is_empty() => tr("Folder is empty").to_string(),
        Listing::Ready(_) if app.visible.is_empty() => trf("Nothing matches “{}”", &[&app.filter]),
        Listing::Ready(entries) => {
            app.list_area = area;
            app.drawn_view = fitting_view(app.view, area);
            // Borrow the fields directly (not through a method on `app`) so that `app.table`
            // can be borrowed mutably while `entries` is borrowed immutably.
            let rows = liman_widgets::Rows::ordered(entries, &app.visible);
            let git_marks = app.git.as_ref().map(|g| &g.marks);
            match app.drawn_view.list_mode() {
                Some(mode) => {
                    let mut list = FileList::new(rows, format::now(), mode)
                        .marked(&app.marked)
                        .sort(app.sort);
                    if let Some(marks) = git_marks {
                        list = list.git(marks);
                    }
                    frame.render_stateful_widget(list, area, &mut app.table);
                }
                None => {
                    let wanted = app.grid_level.unwrap_or(grid::DEFAULT_LEVEL);
                    app.drawn_grid_level = grid::fitting_level(wanted, area);
                    let mut grid = GridView::new(rows, app.drawn_grid_level).marked(&app.marked);
                    if let Some(marks) = git_marks {
                        grid = grid.git(marks);
                    }
                    frame.render_stateful_widget(grid, area, &mut app.table);
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
        TermMode::Fullscreen => tr(" Terminal · Ctrl+O back to files "),
        _ if focused => tr(
            " Terminal · Tab on empty line: next panel · Ctrl+↑↓ size · F4 close · Ctrl+O full screen ",
        ),
        _ => tr(" Terminal · Tab or click to type "),
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

/// Status bar key hints (English; shown through `tr`).
const HINTS: &[(&str, &str)] = &[
    ("Enter", "open"),
    ("Bksp", "up"),
    ("Tab", "panels"),
    ("v", "small/large"),
    ("t", "theme"),
    ("Ctrl+P", "commands"),
    ("?", "all keys"),
    ("q", "quit"),
];
const FILTER_HINTS: &[(&str, &str)] = &[("Enter", "keep filter"), ("Esc", "clear")];
const PREVIEW_HINTS: &[(&str, &str)] = &[
    ("↑↓ PgUp/PgDn", "scroll"),
    ("g/G", "start / end"),
    ("Enter", "full screen"),
    ("Esc", "files"),
];
const TERMINAL_HINTS: &[(&str, &str)] = &[
    ("Ctrl+O", "files / full screen"),
    ("F6", "focus"),
    ("F4", "panel"),
];

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    if app.term_mode != TermMode::Hidden && app.focus == Focus::Terminal
        || app.term_mode == TermMode::Fullscreen
    {
        spans.push(Span::raw(" ⌂ ").fg(theme::dim()));
        spans.push(Span::raw(app.cwd.display().to_string()).fg(theme::fg()));
        for (key, what) in TERMINAL_HINTS {
            spans.push(Span::raw(format!("  {key} ")).bold());
            spans.push(Span::raw(tr(what)).fg(theme::dim()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(Style::new().fg(theme::fg()).bg(theme::bar_bg())),
            area,
        );
        return;
    }
    if let Some(text) = &app.commit_input {
        let line = Line::from(vec![
            Span::raw(tr(" ⎇ Commit message: ")).fg(theme::dim()),
            Span::raw(text.clone()).fg(theme::fg()).bold(),
            Span::raw(tr("▏   Enter commit · Esc cancel")).fg(theme::dim()),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(theme::bar_bg())),
            area,
        );
        return;
    }
    if let Some(text) = &app.search_input {
        let line = Line::from(vec![
            Span::raw(tr(" ⌕ Search in this folder and below: ")).fg(theme::dim()),
            Span::raw(text.clone()).fg(theme::fg()).bold(),
            Span::raw(tr("▏   Enter search · Esc cancel")).fg(theme::dim()),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(theme::bar_bg())),
            area,
        );
        return;
    }
    if let Some(input) = &app.rename {
        spans.push(Span::raw(tr(" Rename: ")).fg(theme::dim()));
        spans.push(Span::raw(input.text.as_str()).fg(theme::fg()).bold());
        spans.push(Span::raw("▏  ").fg(theme::fg()));
        spans.push(Span::raw(" Enter ").bold());
        spans.push(Span::raw(tr("rename ")).fg(theme::dim()));
        spans.push(Span::raw(" Esc ").bold());
        spans.push(Span::raw(tr("cancel")).fg(theme::dim()));
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
        spans.push(Span::raw(trf(" | “{}” selected{}", &[&entry.name, &size])).fg(theme::dim()));
    }
    if !app.marked.is_empty() {
        spans.push(Span::raw(trf(" | {} marked", &[&app.marked.len()])).fg(theme::fg()));
    }
    if let Some(clip) = &app.clipboard {
        let template = match clip.mode {
            ClipMode::Copy => " | {} copied",
            ClipMode::Cut => " | {} cut",
        };
        spans.push(Span::raw(trf(template, &[&format::items(clip.paths.len())])).fg(theme::dim()));
    }
    let view = match app.drawn_view {
        View::Grid => trf("Grid {}", &[&(app.drawn_grid_level + 1)]),
        other => tr(other.name()).to_string(),
    };
    spans.push(Span::raw(trf(" | {} view", &[&view])).fg(theme::dim()));
    spans.push(Span::raw("  "));
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!("{message}  ")).fg(theme::fg()));
    }
    let hints = if app.focus == Focus::Preview {
        PREVIEW_HINTS
    } else if app.filter_editing {
        FILTER_HINTS
    } else {
        HINTS
    };
    for (key, what) in hints {
        spans.push(Span::raw(format!(" {key} ")).bold());
        spans.push(Span::raw(format!("{} ", tr(what))).fg(theme::dim()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().fg(theme::fg()).bg(theme::bar_bg())),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_and_hints_have_turkish() {
        use liman_core::i18n::turkish;
        let all = HELP_KEYS
            .iter()
            .chain(HINTS)
            .chain(FILTER_HINTS)
            .chain(PREVIEW_HINTS)
            .chain(TERMINAL_HINTS);
        for (key, what) in all {
            assert!(turkish(what).is_some(), "no Turkish for {what:?}");
            // Key names with words in them ("double click", "header click") are translated too.
            let has_word = key
                .split(|c: char| !c.is_alphabetic())
                .any(|w| w.len() > 3 && w.chars().all(|c| c.is_lowercase()));
            assert!(
                !has_word || turkish(key).is_some(),
                "no Turkish for {key:?}"
            );
        }
    }
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
            contents: None,
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
