//! Application state. Rendering reads it, events change it.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use liman_core::{Entry, ListOptions};
use liman_widgets::DetailedView;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::event::AppEvent;
use crate::worker;

/// Two clicks on the same row within this time count as a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Rows moved per mouse wheel step.
const WHEEL_STEP: isize = 3;

/// What the body of the window shows.
pub enum Listing {
    Loading,
    Ready(Vec<Entry>),
    Failed(String),
}

pub struct App {
    pub running: bool,
    /// Set when the screen must be redrawn.
    pub dirty: bool,
    pub cwd: PathBuf,
    pub listing: Listing,
    /// Indices into the `Ready` entries that pass the filter, in display order.
    pub visible: Vec<usize>,
    /// Selected row (index into `visible`) and scroll offset; kept between frames.
    pub table: TableState,
    /// Case-insensitive substring filter typed after `/`.
    pub filter: String,
    /// True while the user is typing the filter.
    pub filter_editing: bool,
    /// One-line message for the status bar (cleared on the next key press).
    pub message: Option<String>,
    /// Where the list was drawn last frame, for mouse hit-testing. Written by the UI.
    pub list_area: Rect,
    /// Incremented on every listing request; older results are ignored.
    generation: u64,
    /// After going up a level, the folder we came from gets selected.
    select_after_load: Option<String>,
    last_click: Option<(Instant, usize)>,
    options: ListOptions,
    tx: Sender<AppEvent>,
}

impl App {
    /// Creates the app and starts listing `cwd` in the background.
    pub fn new(cwd: PathBuf, tx: Sender<AppEvent>) -> Self {
        let mut app = Self {
            running: true,
            dirty: true,
            cwd: cwd.clone(),
            listing: Listing::Loading,
            visible: Vec::new(),
            table: TableState::default(),
            filter: String::new(),
            filter_editing: false,
            message: None,
            list_area: Rect::default(),
            generation: 0,
            select_after_load: None,
            last_click: None,
            options: ListOptions::default(),
            tx,
        };
        app.load(cwd);
        app
    }

    /// Requests a listing of `path`. The view switches when the result arrives.
    pub fn load(&mut self, path: PathBuf) {
        self.generation += 1;
        self.listing = Listing::Loading;
        self.visible.clear();
        self.filter.clear();
        self.filter_editing = false;
        self.dirty = true;
        worker::spawn_listing(self.tx.clone(), self.generation, path, self.options);
    }

    pub fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => self.on_key(key),
            AppEvent::Input(Event::Mouse(mouse)) => self.on_mouse(mouse),
            AppEvent::Input(Event::Resize(..)) => self.dirty = true,
            AppEvent::Input(_) => {}
            AppEvent::Listing {
                generation,
                path,
                result,
            } => self.on_listing(generation, path, result),
        }
    }

    /// Entries currently shown, in display order.
    pub fn visible_entries(&self) -> Vec<&Entry> {
        match &self.listing {
            Listing::Ready(entries) => self.visible.iter().map(|&i| &entries[i]).collect(),
            _ => Vec::new(),
        }
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        let Listing::Ready(entries) = &self.listing else {
            return None;
        };
        let row = self.table.selected()?;
        self.visible.get(row).map(|&i| &entries[i])
    }

    // ---- keyboard ----

    fn on_key(&mut self, key: KeyEvent) {
        self.dirty = true;
        self.message = None;
        if self.filter_editing {
            self.on_filter_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('c') if ctrl => self.running = false,
            KeyCode::Esc if !self.filter.is_empty() => self.set_filter(String::new()),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(self.page()),
            KeyCode::PageUp => self.move_selection(-self.page()),
            KeyCode::Home | KeyCode::Char('g') => self.select_row(0),
            KeyCode::End | KeyCode::Char('G') => self.select_row(usize::MAX),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.activate_selected(),
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => self.go_up(),
            KeyCode::Char('/') => self.filter_editing = true,
            _ => self.dirty = false,
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filter_editing = false;
                self.set_filter(String::new());
            }
            KeyCode::Enter => self.filter_editing = false,
            KeyCode::Backspace if self.filter.is_empty() => self.filter_editing = false,
            KeyCode::Backspace => {
                let mut f = self.filter.clone();
                f.pop();
                self.set_filter(f);
            }
            // Arrows still move the selection while typing.
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Char(c) => {
                let f = format!("{}{c}", self.filter);
                self.set_filter(f);
            }
            _ => {}
        }
    }

    // ---- mouse ----

    fn on_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.move_selection(WHEEL_STEP),
            MouseEventKind::ScrollUp => self.move_selection(-WHEEL_STEP),
            MouseEventKind::Down(MouseButton::Left) => self.on_click(mouse.column, mouse.row),
            _ => return, // movement, drag, release: nothing to do yet
        }
        self.dirty = true;
    }

    fn on_click(&mut self, column: u16, row: u16) {
        let Some(index) = DetailedView::row_at(self.list_area, self.table.offset(), column, row)
        else {
            return;
        };
        if index >= self.visible.len() {
            return;
        }
        let now = Instant::now();
        let double = self
            .last_click
            .is_some_and(|(at, i)| i == index && now.duration_since(at) <= DOUBLE_CLICK);
        self.table.select(Some(index));
        if double {
            self.last_click = None;
            self.activate_selected();
        } else {
            self.last_click = Some((now, index));
        }
    }

    // ---- navigation ----

    fn page(&self) -> isize {
        // Visible rows minus the header; at least one.
        isize::try_from(self.list_area.height.saturating_sub(2))
            .unwrap_or(1)
            .max(1)
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let current = self.table.selected().unwrap_or(0);
        self.select_row(current.saturating_add_signed(delta));
    }

    /// Selects `row`, clamped to the last row.
    fn select_row(&mut self, row: usize) {
        if self.visible.is_empty() {
            self.table.select(None);
        } else {
            self.table.select(Some(row.min(self.visible.len() - 1)));
        }
    }

    /// Enter / double click: open a folder. Opening files comes with file operations (ROADMAP item 7).
    fn activate_selected(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.is_dir {
            let path = entry.path.clone();
            self.load(path);
        } else {
            self.message = Some("Opening files is not implemented yet".into());
        }
    }

    fn go_up(&mut self) {
        let Some(parent) = self.cwd.parent().map(PathBuf::from) else {
            return;
        };
        self.select_after_load = self
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.load(parent);
    }

    fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.refresh_visible();
        self.select_row(0);
    }

    fn refresh_visible(&mut self) {
        let Listing::Ready(entries) = &self.listing else {
            self.visible.clear();
            return;
        };
        let needle = self.filter.to_lowercase();
        self.visible = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
    }

    // ---- worker results ----

    fn on_listing(&mut self, generation: u64, path: PathBuf, result: Result<Vec<Entry>, String>) {
        if generation != self.generation {
            return; // stale: a newer request is on its way
        }
        self.cwd = path;
        self.listing = match result {
            Ok(entries) => Listing::Ready(entries),
            Err(err) => Listing::Failed(err),
        };
        self.refresh_visible();
        self.table = TableState::default();
        let came_from = self.select_after_load.take();
        let row = came_from
            .and_then(|name| self.visible_entries().iter().position(|e| e.name == name))
            .unwrap_or(0);
        self.select_row(row);
        self.dirty = true;
    }

    pub fn entry_count(&self) -> Option<usize> {
        match &self.listing {
            Listing::Ready(_) => Some(self.visible.len()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liman_core::FileType;
    use ratatui::crossterm::event::KeyEventState;
    use std::sync::mpsc::{self, Receiver};

    fn key(code: KeyCode) -> AppEvent {
        AppEvent::Input(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }))
    }

    fn click(column: u16, row: u16) -> AppEvent {
        AppEvent::Input(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
    }

    fn entry(name: &str, is_dir: bool) -> Entry {
        let path = PathBuf::from("/data").join(name);
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

    /// App showing /data with: Music/, Projects/, a.txt, b.pdf, c.rs
    fn app() -> (App, Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data"), tx);
        let generation = app.generation;
        app.handle(AppEvent::Listing {
            generation,
            path: PathBuf::from("/data"),
            result: Ok(vec![
                entry("Music", true),
                entry("Projects", true),
                entry("a.txt", false),
                entry("b.pdf", false),
                entry("c.rs", false),
            ]),
        });
        app.list_area = Rect::new(2, 2, 60, 10); // rows start at y = 4
        app.dirty = false;
        (app, rx)
    }

    fn selected_name(app: &App) -> String {
        app.selected_entry().unwrap().name.clone()
    }

    #[test]
    fn q_and_ctrl_c_quit_esc_does_not() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Esc));
        assert!(app.running);
        app.handle(key(KeyCode::Char('q')));
        assert!(!app.running);

        let (mut app, _rx) = self::app();
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))));
        assert!(!app.running);
    }

    #[test]
    fn moving_the_selection_is_clamped() {
        let (mut app, _rx) = app();
        assert_eq!(selected_name(&app), "Music");
        app.handle(key(KeyCode::Char('j')));
        app.handle(key(KeyCode::Down));
        assert_eq!(selected_name(&app), "a.txt");
        app.handle(key(KeyCode::End));
        assert_eq!(selected_name(&app), "c.rs");
        app.handle(key(KeyCode::Down));
        assert_eq!(selected_name(&app), "c.rs");
        app.handle(key(KeyCode::Char('g')));
        app.handle(key(KeyCode::Up));
        assert_eq!(selected_name(&app), "Music");
    }

    #[test]
    fn enter_on_a_folder_loads_it() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Char('j'))); // Projects
        app.handle(key(KeyCode::Enter));
        assert!(matches!(app.listing, Listing::Loading));
    }

    #[test]
    fn enter_on_a_file_only_shows_a_message() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Char('G')));
        app.handle(key(KeyCode::Enter));
        assert!(matches!(app.listing, Listing::Ready(_)));
        assert!(app.message.is_some());
    }

    #[test]
    fn going_up_selects_the_folder_we_came_from() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data/Projects"), tx);
        app.handle(key(KeyCode::Backspace));
        let generation = app.generation;
        app.handle(AppEvent::Listing {
            generation,
            path: PathBuf::from("/data"),
            result: Ok(vec![
                entry("Music", true),
                entry("Projects", true),
                entry("a.txt", false),
            ]),
        });
        assert_eq!(app.cwd, PathBuf::from("/data"));
        assert_eq!(selected_name(&app), "Projects");
    }

    #[test]
    fn filter_narrows_the_list_and_esc_clears_it() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Char('/')));
        for c in ['P', 'r', 'o'] {
            app.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(app.filter, "Pro");
        assert_eq!(app.entry_count(), Some(1));
        assert_eq!(selected_name(&app), "Projects");

        app.handle(key(KeyCode::Enter)); // stop typing, keep the filter
        assert!(!app.filter_editing);
        app.handle(key(KeyCode::Char('q'))); // 'q' is a command again
        assert!(!app.running);

        let (mut app, _rx) = self::app();
        app.handle(key(KeyCode::Char('/')));
        app.handle(key(KeyCode::Char('x')));
        app.handle(key(KeyCode::Esc));
        assert_eq!(app.filter, "");
        assert_eq!(app.entry_count(), Some(5));
    }

    #[test]
    fn click_selects_and_double_click_opens() {
        let (mut app, _rx) = app();
        app.handle(click(10, 5)); // second row: Projects
        assert_eq!(selected_name(&app), "Projects");
        assert!(matches!(app.listing, Listing::Ready(_)));
        app.handle(click(10, 5));
        assert!(matches!(app.listing, Listing::Loading));
    }

    #[test]
    fn clicks_outside_the_rows_are_ignored() {
        let (mut app, _rx) = app();
        app.handle(click(10, 2)); // header
        app.handle(click(10, 11)); // below the last entry
        assert_eq!(selected_name(&app), "Music");
    }

    #[test]
    fn wheel_moves_the_selection() {
        let (mut app, _rx) = app();
        app.handle(AppEvent::Input(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        })));
        assert_eq!(selected_name(&app), "b.pdf");
    }

    #[test]
    fn stale_results_are_ignored() {
        let (mut app, _rx) = app();
        app.load(PathBuf::from("/"));
        app.handle(AppEvent::Listing {
            generation: 1,
            path: PathBuf::from("/old"),
            result: Ok(Vec::new()),
        });
        assert!(matches!(app.listing, Listing::Loading));
    }
}
