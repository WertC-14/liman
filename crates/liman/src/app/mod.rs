//! Application state. Rendering reads it, events change it.

mod file_ops;

pub use file_ops::{ClipMode, Clipboard, JobStatus, RenameInput};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use liman_core::icons::{ICON_SIZES, IconPixels, IconTheme, icon_key};
use liman_core::job::Done;
use liman_core::{Entry, ListOptions, Place, Places, trash};
use liman_widgets::breadcrumb::{self, Segment};
use liman_widgets::{FileList, GridView, ListMode, Sidebar};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::event::AppEvent;
use crate::open::{self, OpenPlan};
use crate::worker;

/// Two clicks on the same row within this time count as a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Rows moved per mouse wheel step.
const WHEEL_STEP: isize = 3;

/// The three views of ADR 0003, smallest first. `+` / `-` step through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Detailed,
    Normal,
    Grid,
}

impl View {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Detailed => "Detailed",
            Self::Normal => "Normal",
            Self::Grid => "Grid",
        }
    }

    /// The file list mode for the two list views; `None` for the grid.
    pub const fn list_mode(self) -> Option<ListMode> {
        match self {
            Self::Detailed => Some(ListMode::Detailed),
            Self::Normal => Some(ListMode::Normal),
            Self::Grid => None,
        }
    }
}

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
    /// A terminal program the main loop should run in the foreground (set by Enter on a file over SSH).
    pub external: Option<(String, PathBuf)>,
    /// View chosen by the user (`+` / `-`, Ctrl+wheel).
    pub view: View,
    /// View actually drawn last frame; smaller than `view` when the terminal is too small. Written by the UI.
    pub drawn_view: View,
    /// Where the list was drawn last frame, for mouse hit-testing. Written by the UI.
    pub list_area: Rect,
    /// Where the sidebar was drawn last frame (empty when hidden). Written by the UI.
    pub sidebar_area: Rect,
    /// Where the path bar was drawn last frame. Written by the UI.
    pub path_bar_area: Rect,
    pub places: Places,
    pub sidebar: Vec<Place>,
    /// Entries marked with Space (or Ctrl+A) for a multi-item operation.
    pub marked: HashSet<PathBuf>,
    pub clipboard: Option<Clipboard>,
    /// The file operation running on the worker, if any.
    pub job: Option<JobStatus>,
    /// Finished operations, newest last; undo (Ctrl+Z) walks back through them.
    pub history: Vec<Done>,
    /// F2 rename in progress.
    pub rename: Option<RenameInput>,
    /// Rendered icons for the grid, by `icon_key`.
    pub icons: HashMap<String, IconPixels>,
    /// Keys already sent to the icon worker (loaded or on their way).
    icons_requested: HashSet<String>,
    /// Icon size chosen with `+` / `-` in the grid; `None` = pick the largest that fits.
    pub grid_size: Option<u32>,
    /// Icon size drawn last frame. Written by the UI.
    pub drawn_icon_size: u32,
    /// (listing generation, icon size) the icons were last requested for.
    icons_requested_for: Option<(u64, u32)>,
    /// The icon theme, loaded once by the first icon worker.
    icon_theme: Arc<OnceLock<IconTheme>>,
    trash_dir: PathBuf,
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
    pub fn new(cwd: PathBuf, places: Places, tx: Sender<AppEvent>) -> Self {
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
            external: None,
            // The grid with large icons is what sets liman apart, so it is the first thing you see.
            view: View::Grid,
            drawn_view: View::Detailed,
            icons: HashMap::new(),
            icons_requested: HashSet::new(),
            grid_size: None,
            drawn_icon_size: ICON_SIZES[0],
            icons_requested_for: None,
            icon_theme: Arc::new(OnceLock::new()),
            list_area: Rect::default(),
            sidebar_area: Rect::default(),
            path_bar_area: Rect::default(),
            sidebar: places.sidebar(),
            marked: HashSet::new(),
            clipboard: None,
            job: None,
            history: Vec::new(),
            rename: None,
            trash_dir: trash::home_trash(&places.home),
            places,
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
        self.marked.clear();
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
            AppEvent::JobProgress { done } => self.on_job_progress(done),
            AppEvent::JobFinished(outcome) => self.on_job_finished(outcome),
            AppEvent::Icons(icons) => {
                self.icons.extend(icons);
                self.dirty = true;
            }
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
        if self.rename.is_some() {
            self.on_rename_key(key);
            return;
        }
        if self.filter_editing {
            self.on_filter_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // GUI shortcuts: Ctrl+C copies, so quitting is q or Ctrl+Q.
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('c') if ctrl => self.copy_to_clipboard(ClipMode::Copy),
            KeyCode::Char('x') if ctrl => self.copy_to_clipboard(ClipMode::Cut),
            KeyCode::Char('v') if ctrl => self.paste(),
            KeyCode::Char('a') if ctrl => self.mark_all(),
            KeyCode::Char('z') if ctrl => self.undo(),
            KeyCode::Delete => self.trash_targets(),
            KeyCode::F(2) => self.begin_rename(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Esc if !self.filter.is_empty() => self.set_filter(String::new()),
            KeyCode::Esc => self.marked.clear(),
            // In the grid, left/right move between tiles and up/down jump a row (like Nautilus).
            KeyCode::Right | KeyCode::Char('l') if self.drawn_view == View::Grid => {
                self.move_selection(1)
            }
            KeyCode::Left | KeyCode::Char('h') if self.drawn_view == View::Grid => {
                self.move_selection(-1)
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(self.vertical_step()),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-self.vertical_step()),
            KeyCode::PageDown => self.move_selection(self.page()),
            KeyCode::PageUp => self.move_selection(-self.page()),
            KeyCode::Home | KeyCode::Char('g') => self.select_row(0),
            KeyCode::End | KeyCode::Char('G') => self.select_row(usize::MAX),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.activate_selected(),
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => self.go_up(),
            KeyCode::Char('/') => self.filter_editing = true,
            KeyCode::Char('~') => self.load(self.places.home.clone()),
            // Ctrl variants arrive only in terminals that report them; plain keys always work.
            KeyCode::Char('+' | '=') => self.zoom(1),
            KeyCode::Char('-') => self.zoom(-1),
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
        let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
        match mouse.kind {
            MouseEventKind::ScrollUp if ctrl => self.zoom(1),
            MouseEventKind::ScrollDown if ctrl => self.zoom(-1),
            MouseEventKind::ScrollDown => self.move_selection(WHEEL_STEP),
            MouseEventKind::ScrollUp => self.move_selection(-WHEEL_STEP),
            MouseEventKind::Down(MouseButton::Left) => self.on_click(mouse.column, mouse.row),
            _ => return, // movement, drag, release: nothing to do yet
        }
        self.dirty = true;
    }

    fn on_click(&mut self, column: u16, row: u16) {
        if let Some(i) = Sidebar::row_at(self.sidebar_area, self.sidebar.len(), column, row) {
            let path = self.sidebar[i].path.clone();
            self.load(path);
            return;
        }
        if row == self.path_bar_area.y {
            let segments = self.path_segments();
            if let Some(i) = breadcrumb::segment_at(&segments, self.path_bar_area.x, column) {
                let path = segments[i].path.clone();
                if path != self.cwd {
                    self.load(path);
                }
            }
            return;
        }
        let hit = match self.drawn_view.list_mode() {
            Some(mode) => FileList::row_at(self.list_area, self.table.offset(), mode, column, row),
            None => GridView::index_at(
                self.list_area,
                self.table.offset(),
                self.drawn_icon_size,
                column,
                row,
            ),
        };
        let Some(index) = hit else {
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

    pub fn path_segments(&self) -> Vec<Segment> {
        breadcrumb::segments(&self.cwd, &self.places.home)
    }

    /// Entries that fit on one screen; at least one.
    fn page(&self) -> isize {
        let entries = match self.drawn_view.list_mode() {
            Some(mode) => usize::from(
                self.list_area
                    .height
                    .saturating_sub(liman_widgets::file_list::HEADER_HEIGHT)
                    / mode.row_height(),
            ),
            None => {
                let (_, tile_h) = liman_widgets::grid::tile_size(self.drawn_icon_size);
                usize::from(self.list_area.height / tile_h) * self.grid_columns()
            }
        };
        isize::try_from(entries).unwrap_or(1).max(1)
    }

    /// Zooms like a GUI file manager: Detailed → Normal → Grid, then larger and larger icons.
    fn zoom(&mut self, step: i8) {
        let sizes = ICON_SIZES;
        let current = sizes
            .iter()
            .position(|&s| s == self.drawn_icon_size)
            .unwrap_or(0);
        match (self.view, step.signum()) {
            (View::Detailed, 1) => self.view = View::Normal,
            (View::Normal, 1) => {
                self.view = View::Grid;
                self.grid_size = None; // fit to the window first
            }
            (View::Grid, 1) => {
                self.grid_size = Some(sizes[(current + 1).min(sizes.len() - 1)]);
            }
            (View::Grid, -1) if current == 0 => {
                self.view = View::Normal;
                self.grid_size = None;
            }
            (View::Grid, -1) => self.grid_size = Some(sizes[current - 1]),
            (View::Normal, -1) => self.view = View::Detailed,
            _ => {}
        }
    }

    /// Called after every frame: once the grid's icon size is known, asks for missing icons.
    pub fn after_draw(&mut self) {
        let wanted = (self.generation, self.drawn_icon_size);
        if self.drawn_view == View::Grid && self.icons_requested_for != Some(wanted) {
            if matches!(self.listing, Listing::Ready(_)) {
                self.icons_requested_for = Some(wanted);
            }
            self.request_icons();
        }
    }

    /// Asks the worker for icons of the drawn size that are not loaded or on their way yet.
    fn request_icons(&mut self) {
        let Listing::Ready(entries) = &self.listing else {
            return;
        };
        let size = self.drawn_icon_size;
        let mut wanted = Vec::new();
        for entry in entries {
            let key = icon_key(entry, size);
            if self.icons_requested.insert(key.clone()) {
                wanted.push((key, entry.clone()));
            }
        }
        if !wanted.is_empty() {
            worker::spawn_icons(
                self.tx.clone(),
                self.icon_theme.clone(),
                self.places.home.clone(),
                size,
                wanted,
            );
        }
    }

    /// Tiles per row in the grid as drawn last frame.
    fn grid_columns(&self) -> usize {
        GridView::columns(self.list_area.width, self.drawn_icon_size)
    }

    /// Rows move by one entry in the lists, by a whole tile row in the grid.
    fn vertical_step(&self) -> isize {
        match self.drawn_view {
            View::Grid => isize::try_from(self.grid_columns()).unwrap_or(1),
            _ => 1,
        }
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

    /// Enter / double click: open a folder here, a file in its app (or editor over SSH).
    fn activate_selected(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.is_dir {
            let path = entry.path.clone();
            self.load(path);
            return;
        }
        match open::plan(entry, &open::Env::current()) {
            OpenPlan::Desktop(path) => {
                let name = entry.name.clone();
                self.message = Some(match open::open_desktop(&path) {
                    Ok(()) => format!("Opening “{name}”…"),
                    Err(e) => format!("Cannot open “{name}”: {e}"),
                });
            }
            OpenPlan::Editor { program, path } => self.external = Some((program, path)),
            OpenPlan::Unavailable(why) => self.message = Some(why),
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
            Ok(mut entries) => {
                for entry in entries.iter_mut().filter(|e| e.is_dir) {
                    entry.special = self.places.kind_of(&entry.path);
                }
                Listing::Ready(entries)
            }
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
    use std::path::Path;
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

    /// Home is /data, Music is the XDG music dir.
    fn places() -> Places {
        Places::from_user_dirs("XDG_MUSIC_DIR=\"$HOME/Music\"", Path::new("/data"))
    }

    /// App showing /data with: Music/, Projects/, a.txt, b.pdf, c.rs
    fn app() -> (App, Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data"), places(), tx);
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
        app.list_area = Rect::new(24, 2, 60, 10); // rows start at y = 4
        app.sidebar_area = Rect::new(0, 1, 22, 10);
        app.sidebar = vec![Place {
            name: "Home".into(),
            path: PathBuf::from("/data"),
            kind: liman_core::SpecialDir::Home,
        }];
        app.path_bar_area = Rect::new(0, 0, 80, 1);
        app.dirty = false;
        (app, rx)
    }

    fn selected_name(app: &App) -> String {
        app.selected_entry().unwrap().name.clone()
    }

    #[test]
    fn q_and_ctrl_q_quit_esc_and_ctrl_c_do_not() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Esc));
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))));
        assert!(app.running);
        app.handle(key(KeyCode::Char('q')));
        assert!(!app.running);

        let (mut app, _rx) = self::app();
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
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
    fn enter_on_a_file_keeps_the_listing() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Char('G')));
        // Do not actually launch anything from a test: only check the folder did not change.
        let entry = app.selected_entry().unwrap().clone();
        assert!(!entry.is_dir);
        assert!(matches!(app.listing, Listing::Ready(_)));
    }

    #[test]
    fn going_up_selects_the_folder_we_came_from() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data/Projects"), places(), tx);
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
        app.handle(click(30, 5)); // second row: Projects
        assert_eq!(selected_name(&app), "Projects");
        assert!(matches!(app.listing, Listing::Ready(_)));
        app.handle(click(30, 5));
        assert!(matches!(app.listing, Listing::Loading));
    }

    #[test]
    fn clicks_outside_the_rows_are_ignored() {
        let (mut app, _rx) = app();
        app.handle(click(30, 2)); // header
        app.handle(click(30, 11)); // below the last entry
        assert_eq!(selected_name(&app), "Music");
    }

    #[test]
    fn special_folders_are_marked_after_loading() {
        let (app, _rx) = app();
        let music = app.visible_entries()[0];
        assert_eq!(music.name, "Music");
        assert_eq!(music.special, Some(liman_core::SpecialDir::Music));
        assert_eq!(app.visible_entries()[1].special, None);
    }

    #[test]
    fn clicking_the_sidebar_or_path_bar_navigates() {
        let (mut app, _rx) = app();
        app.handle(click(5, 1)); // sidebar row 0: Home (/data)
        assert!(matches!(app.listing, Listing::Loading));

        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data/Projects"), places(), tx);
        app.path_bar_area = Rect::new(0, 0, 80, 1);
        let generation = app.generation;
        app.handle(AppEvent::Listing {
            generation,
            path: PathBuf::from("/data/Projects"),
            result: Ok(Vec::new()),
        });
        app.handle(click(3, 0)); // "⌂ Home"
        assert!(matches!(app.listing, Listing::Loading));
    }

    #[test]
    fn tilde_goes_home() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data/Projects"), places(), tx);
        app.handle(key(KeyCode::Char('~')));
        let generation = app.generation;
        assert_eq!(generation, 2);
    }

    #[test]
    fn plus_minus_and_ctrl_wheel_switch_views() {
        let (mut app, _rx) = app();
        assert_eq!(app.view, View::Grid); // default
        app.view = View::Detailed;
        app.handle(key(KeyCode::Char('+')));
        assert_eq!(app.view, View::Normal);
        app.handle(key(KeyCode::Char('+')));
        assert_eq!((app.view, app.grid_size), (View::Grid, None)); // grid, fitted to the window
        app.handle(key(KeyCode::Char('+'))); // larger icons
        assert_eq!(app.grid_size, Some(24));
        app.drawn_icon_size = 24;
        app.handle(key(KeyCode::Char('-')));
        assert_eq!(app.grid_size, Some(16));
        app.drawn_icon_size = 16;
        app.handle(key(KeyCode::Char('-'))); // smallest icons: back to the box list
        app.handle(key(KeyCode::Char('-')));
        assert_eq!(app.view, View::Detailed);

        app.handle(AppEvent::Input(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 30,
            row: 5,
            modifiers: KeyModifiers::CONTROL,
        })));
        assert_eq!(app.view, View::Normal);
        assert_eq!(selected_name(&app), "Music"); // Ctrl+wheel does not move the selection
    }

    #[test]
    fn clicks_use_the_drawn_view() {
        let (mut app, _rx) = app();
        app.drawn_view = View::Normal;
        // list_area.y = 2, rows start at 4; entry 1 (Projects) spans rows 9..=12
        app.handle(click(30, 10));
        assert_eq!(selected_name(&app), "Projects");
    }

    #[test]
    fn grid_arrows_move_by_tile_and_by_row() {
        let (mut app, _rx) = app();
        app.drawn_view = View::Grid;
        app.list_area = Rect::new(24, 2, 40, 30); // 2 tiles per row
        app.handle(key(KeyCode::Right));
        assert_eq!(selected_name(&app), "Projects");
        app.handle(key(KeyCode::Down));
        assert_eq!(selected_name(&app), "b.pdf");
        app.handle(key(KeyCode::Left));
        assert_eq!(selected_name(&app), "a.txt");
        assert_eq!(app.cwd, PathBuf::from("/data")); // left did not go up a folder
    }

    #[test]
    fn grid_view_requests_each_icon_kind_once() {
        let (mut app, rx) = app();
        app.drawn_view = View::Grid; // as if the UI drew the grid
        app.after_draw();
        app.after_draw(); // same folder, same size: no second request
        // Other worker events (the initial listing of /data) may arrive first.
        while app.icons.is_empty() {
            let ev = rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("icons");
            app.handle(ev);
        }
        // Music (special), Projects (folder), a.txt, b.pdf, c.rs: 5 different icons.
        assert_eq!(app.icons.len(), 5);
        app.after_draw(); // nothing new to ask for
        while let Ok(ev) = rx.recv_timeout(std::time::Duration::from_millis(300)) {
            assert!(
                !matches!(ev, AppEvent::Icons(_)),
                "icons were requested twice"
            );
        }
    }

    #[test]
    fn wheel_moves_the_selection() {
        let (mut app, _rx) = app();
        app.handle(AppEvent::Input(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 30,
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
