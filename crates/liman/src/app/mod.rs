//! Application state. Rendering reads it, events change it.

mod actions;
mod file_ops;
mod git_ops;
mod preview;
mod tabs;
mod terminal_mode;

pub use actions::{Action, HELP, Menu};
pub use file_ops::{ClipMode, Clipboard, Dialog, JobStatus, RenameInput};
pub use git_ops::GitPanel;
pub use preview::{Graphic, PreviewKey, PreviewPane};
pub use tabs::{TabLabel, TabState, Tabs};
pub use terminal_mode::{Focus, TermMode};

use liman_core::i18n::{tr, trf};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use liman_core::job::Done;
use liman_core::{Entry, ListOptions, Place, Places, trash};
use liman_widgets::breadcrumb::{self, Segment};
use liman_widgets::grid::{self, GridView};
use liman_widgets::sidebar::Item as SideItem;
use liman_widgets::{FileList, ListMode, Sidebar};
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
/// While folder counts arrive, a size sort is redone at most this often.
const COUNT_SORT_INTERVAL: Duration = Duration::from_secs(1);
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
    /// The file list mode for the two list views; `None` for the grid.
    pub const fn list_mode(self) -> Option<ListMode> {
        match self {
            Self::Detailed => Some(ListMode::Detailed),
            Self::Normal => Some(ListMode::Normal),
            Self::Grid => None,
        }
    }
}

/// A key that types a character into a text field (not Ctrl+… or Alt+…, which are commands).
fn is_typing(key: KeyEvent) -> bool {
    !key.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Mouse button held on an entry.
struct Drag {
    from: usize,
    start: (u16, u16),
    /// Moved far enough to count as dragging.
    active: bool,
}

/// A listing of files found by a shell command (`find`, `fd`, `grep -l`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Results {
    pub command: String,
    pub count: usize,
    /// A live search (Ctrl+F) is still finding more.
    pub running: bool,
}

/// What the body of the window shows.
pub enum Listing {
    Loading,
    Ready(Vec<Entry>),
    Failed(String),
}

pub struct App {
    /// The active tab: its folder, listing, selection, history, shell... (ADR 0008: other tabs
    /// are parked as `TabState` and swapped in here).
    pub tab: TabState,
    pub running: bool,
    /// Set when the screen must be redrawn.
    pub dirty: bool,
    /// The listing on its way comes from back/forward (do not touch the stacks).
    moving_in_history: bool,
    /// Order of folder listings (`s` / `S` / header click); saved in the config.
    pub sort: liman_core::sort::SortOrder,
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
    /// Highlighted row of the Places panel when it has focus (see `Sidebar::item`).
    pub sidebar_selected: usize,
    /// First row of the Places panel on screen. Written by the UI.
    pub sidebar_offset: usize,
    /// The FOLDERS tree of the Places panel.
    pub tree: liman_core::tree::DirTree,
    /// Where the path bar was drawn last frame. Written by the UI.
    pub path_bar_area: Rect,
    pub places: Places,
    pub sidebar: Vec<Place>,
    /// Folders bookmarked with Ctrl+D (shown in Places with ★).
    pub bookmarks: Vec<PathBuf>,
    pub clipboard: Option<Clipboard>,
    /// The file operation running on the worker, if any.
    pub job: Option<JobStatus>,
    /// Finished operations, newest last; undo (Ctrl+Z) walks back through them.
    pub history: Vec<Done>,
    /// F2 rename in progress.
    pub rename: Option<RenameInput>,
    /// Box size level chosen with `+` / `-` in the grid; `None` = the largest that fits.
    pub grid_level: Option<usize>,
    /// Box size level drawn last frame. Written by the UI.
    pub drawn_grid_level: usize,
    /// Where the terminal was drawn last frame. Written by the UI.
    pub term_area: Rect,
    /// Panel height chosen with Ctrl+↑/↓ or by dragging its top border; `None` = 40% of the window.
    pub term_height: Option<u16>,
    dragging_panel: bool,
    /// Entry index of the last plain or Ctrl click (start of a Shift+click range).
    click_anchor: Option<usize>,
    /// Left button held on an entry (possibly the start of a drag-and-drop).
    drag: Option<Drag>,
    /// The large view `v` returns to from the compact list.
    last_large_view: View,
    /// `g`: the git panel.
    pub git_panel: Option<GitPanel>,
    /// Commit message being typed.
    pub commit_input: Option<String>,
    /// `b` in the git panel: branches and the highlighted one.
    pub branch_picker: Option<(Vec<String>, usize)>,
    /// A `git status` is running; `git_again` asks for one more when it is done.
    git_busy: bool,
    git_again: bool,
    /// Watches the open folder for changes made by anyone.
    watch: crate::watch::FolderWatch,
    /// F3: preview of the selected item.
    pub preview: PreviewPane,
    /// Ctrl+T: the tab row (ADR 0008); the active tab's state is in the fields above.
    pub tabs: Tabs,
    /// Command palette (Ctrl+P) or right-click menu.
    pub menu: Option<Menu>,
    /// Where the menu was drawn (for clicks). Written by the UI.
    pub menu_area: Rect,
    /// After the next listing, start renaming the selected entry (a folder just created).
    rename_after_load: bool,
    /// A question in the middle of the screen (paste conflicts, permanent delete).
    pub dialog: Option<Dialog>,
    /// Ctrl+F: the search text being typed.
    pub search_input: Option<String>,
    /// Ctrl+L: a path being typed into the path bar.
    pub path_input: Option<String>,
    /// Newest listing/search id, shared with search workers so old searches stop early.
    current_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// `t`: the theme list (selected row, theme to go back to on Esc).
    pub theme_picker: Option<(usize, usize)>,
    /// `?`: the shortcut overview.
    pub help_open: bool,
    /// Where the theme list was drawn (for clicks). Written by the UI.
    pub picker_area: Rect,
    /// Results on their way from the worker (becomes `results` when the listing arrives).
    results_pending: Option<Results>,
    /// When the shell's folder was last read (throttles it during long output).
    cwd_checked: Instant,
    /// Folder of the listing on its way, if any.
    loading_path: Option<PathBuf>,
    /// When a size sort last took the arriving folder counts into account.
    counts_sorted: Instant,
    /// When the newest listing was requested (it shows every change made before).
    listing_started: Instant,
    /// The listing on its way was started by following the shell (do not send `cd` back).
    load_from_shell: bool,
    trash_dir: PathBuf,
    /// Incremented on every listing request; older results are ignored.
    generation: u64,
    last_click: Option<(Instant, usize)>,
    options: ListOptions,
    tx: Sender<AppEvent>,
}

impl App {
    /// Creates the app and starts listing `cwd` in the background.
    pub fn new(cwd: PathBuf, places: Places, tx: Sender<AppEvent>) -> Self {
        let mut app = Self {
            tab: TabState::fresh(cwd.clone()),
            running: true,
            dirty: true,
            moving_in_history: false,
            sort: liman_core::sort::SortOrder::default(),
            filter_editing: false,
            message: None,
            external: None,
            // The grid of type-colored boxes is what sets liman apart, so it is the first thing you see.
            view: View::Grid,
            drawn_view: View::Detailed,
            grid_level: None,
            drawn_grid_level: 0,
            term_area: Rect::default(),
            term_height: None,
            dragging_panel: false,
            last_large_view: View::Grid,
            click_anchor: None,
            drag: None,
            theme_picker: None,
            search_input: None,
            path_input: None,
            dialog: None,
            watch: crate::watch::FolderWatch::start(tx.clone()),
            git_busy: false,
            git_panel: None,
            commit_input: None,
            branch_picker: None,
            git_again: false,
            preview: PreviewPane::new(),
            tabs: Tabs::default(),
            menu: None,
            menu_area: Rect::default(),
            rename_after_load: false,
            current_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            help_open: false,
            picker_area: Rect::default(),
            results_pending: None,
            cwd_checked: Instant::now(),
            loading_path: None,
            listing_started: Instant::now(),
            counts_sorted: Instant::now(),
            load_from_shell: false,
            list_area: Rect::default(),
            sidebar_area: Rect::default(),
            sidebar_selected: 1,
            sidebar_offset: 0,
            tree: liman_core::tree::DirTree::new(if places.home == std::path::Path::new("/") {
                vec![PathBuf::from("/")]
            } else {
                vec![places.home.clone(), PathBuf::from("/")]
            }),
            path_bar_area: Rect::default(),
            sidebar: places.sidebar_with(&[]),
            bookmarks: Vec::new(),
            clipboard: None,
            job: None,
            history: Vec::new(),
            rename: None,
            trash_dir: trash::home_trash(&places.home),
            places,
            generation: 0,
            last_click: None,
            options: ListOptions::default(),
            tx,
        };
        app.load(cwd);
        app
    }

    /// Requests a listing of `path`. The view switches when the result arrives.
    pub fn load(&mut self, path: PathBuf) {
        self.results_pending = None;
        self.start_loading(&path);
        worker::spawn_listing(self.tx.clone(), self.generation, path, self.options);
    }

    /// Asks for `git status` of the open folder (at most one at a time; asks again when it ends).
    pub fn request_git(&mut self) {
        if self.git_busy {
            self.git_again = true;
            return;
        }
        self.git_busy = true;
        worker::spawn_git_status(self.tx.clone(), self.tab.cwd.clone());
    }

    /// Reads the open folder again without blanking the view: the old list stays until the new one
    /// arrives; selection, marks and filter are kept.
    pub fn refresh(&mut self) {
        self.invalidate_preview();
        if self.tab.select_after_load.is_none() {
            self.tab.select_after_load = self.selected_entry().map(|e| e.name.clone());
        }
        self.generation += 1;
        self.current_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        self.loading_path = Some(self.tab.cwd.clone());
        self.listing_started = Instant::now();
        worker::spawn_listing(
            self.tx.clone(),
            self.generation,
            self.tab.cwd.clone(),
            self.options,
        );
    }

    /// Shows the files a shell command printed, like a folder listing (Backspace returns to the folder).
    fn load_results(&mut self, results: Results, base: PathBuf, paths: Vec<PathBuf>) {
        self.results_pending = Some(results);
        self.load_from_shell = true; // the shell is already in `base`
        self.start_loading(&base);
        worker::spawn_results(self.tx.clone(), self.generation, base, paths);
    }

    fn start_loading(&mut self, path: &std::path::Path) {
        // Remember what was selected here, for when the user comes back.
        if self.tab.results.is_none()
            && let Some(entry) = self.selected_entry()
        {
            self.tab
                .remembered
                .insert(self.tab.cwd.clone(), entry.name.clone());
        }
        self.generation += 1;
        self.current_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        self.tab.listing = Listing::Loading;
        self.tab.visible.clear();
        self.tab.marked.clear();
        self.tab.filter.clear();
        self.filter_editing = false;
        self.dirty = true;
        self.loading_path = Some(path.to_path_buf());
        self.listing_started = Instant::now();
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
            AppEvent::JobProgress { done, total } => self.on_job_progress(done, total),
            AppEvent::JobFinished(outcome) => self.on_job_finished(outcome),
            AppEvent::TermOutput { id, bytes } if self.is_active_terminal(id) => {
                self.on_term_output(&bytes)
            }
            AppEvent::TermOutput { id, bytes } => {
                self.on_background_terminal(id, tabs::BackgroundTerm::Output(bytes))
            }
            AppEvent::TermExited(id) if self.is_active_terminal(id) => self.on_term_exited(),
            AppEvent::TermExited(id) => {
                self.on_background_terminal(id, tabs::BackgroundTerm::Exited)
            }
            AppEvent::TermQuiet(id) if !self.is_active_terminal(id) => {
                self.on_background_terminal(id, tabs::BackgroundTerm::Quiet)
            }
            // Our own jobs refresh the folder as soon as they finish; their changes reach the
            // watcher too, a moment later. A listing started after the change already shows it.
            AppEvent::FolderChanged { dir, at } => {
                let moving_away = self
                    .loading_path
                    .as_ref()
                    .is_some_and(|p| *p != self.tab.cwd);
                if dir == self.tab.cwd
                    && self.tab.results.is_none()
                    && !moving_away
                    && at > self.listing_started
                {
                    self.refresh();
                }
            }
            AppEvent::Git { dir, status } => {
                self.git_busy = false;
                if dir == self.tab.cwd {
                    self.tab.git = status;
                    self.dirty = true;
                }
                if std::mem::take(&mut self.git_again) {
                    self.request_git();
                }
            }
            AppEvent::SearchFound {
                generation,
                entries,
                done,
            } => self.on_search_found(generation, entries, done),
            AppEvent::Counts {
                generation,
                counts,
                done,
            } => self.on_counts(generation, counts, done),
            AppEvent::GitDone { label, result } => self.on_git_done(label, result),
            AppEvent::GitDiff { path, lines } => self.on_git_diff(path, lines),
            AppEvent::TreeChildren { dir, children } => {
                self.tree.set_children(dir, children);
                self.dirty = true;
            }
            AppEvent::GitRemote { root, remote } => self.on_git_remote(&root, remote),
            AppEvent::GitBranches(result) => self.on_git_branches(result),
            AppEvent::Preview {
                key,
                preview,
                graphic,
            } => self.on_preview(key, *preview, graphic),
            AppEvent::TermQuiet(_) => {
                let Some(term) = &mut self.tab.terminal else {
                    return;
                };
                term.on_quiet();
                let finished = term.take_finished_command();
                self.follow_shell_cwd();
                if let Some(out) = finished {
                    self.on_command_finished(out);
                }
            }
        }
    }

    fn is_active_terminal(&self, id: u64) -> bool {
        self.tab.terminal.as_ref().is_some_and(|t| t.id == id)
    }

    /// Entries currently shown, in display order.
    pub fn visible_entries(&self) -> impl Iterator<Item = &Entry> + '_ {
        let entries: &[Entry] = match &self.tab.listing {
            Listing::Ready(entries) => entries,
            _ => &[],
        };
        self.tab.visible.iter().filter_map(|&i| entries.get(i))
    }

    /// The entry on display row `row`.
    pub fn visible_entry(&self, row: usize) -> Option<&Entry> {
        let Listing::Ready(entries) = &self.tab.listing else {
            return None;
        };
        self.tab.visible.get(row).map(|&i| &entries[i])
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        let Listing::Ready(entries) = &self.tab.listing else {
            return None;
        };
        let row = self.tab.table.selected()?;
        self.tab.visible.get(row).map(|&i| &entries[i])
    }

    // ---- keyboard ----

    fn on_key(&mut self, key: KeyEvent) {
        self.dirty = true;
        self.message = None;
        // Terminal mode keys work everywhere, whichever side has focus.
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::F(4) => return self.toggle_panel(),
            // Alt+1…9 picks a tab from anywhere, the terminal included.
            KeyCode::Char(c @ '1'..='9') if key.modifiers.contains(KeyModifiers::ALT) => {
                return self.switch_tab(c as usize - '1' as usize);
            }
            KeyCode::F(3) => return self.toggle_preview(),
            KeyCode::Char('o') if ctrl => return self.toggle_fullscreen(),
            KeyCode::F(6) if self.tab.term_mode == TermMode::Panel => return self.switch_focus(),
            KeyCode::Up if ctrl && self.tab.term_mode == TermMode::Panel => {
                return self.resize_panel(2);
            }
            KeyCode::Down if ctrl && self.tab.term_mode == TermMode::Panel => {
                return self.resize_panel(-2);
            }
            // Tab / Shift+Tab move between Places, Files and the terminal panel. In the terminal,
            // Tab completes while there is text on the command line.
            // While a path is typed, Tab completes it instead.
            KeyCode::Tab
                if self.path_input.is_none()
                    && (!self.terminal_has_focus()
                        || (self.tab.term_mode == TermMode::Panel
                            && self.terminal_line_empty())) =>
            {
                return self.focus_next();
            }
            KeyCode::BackTab if self.tab.term_mode != TermMode::Fullscreen => {
                return self.focus_prev();
            }
            _ => {}
        }
        if self.terminal_has_focus() {
            return self.on_term_key(key);
        }
        if self.tab.focus == Focus::Places && self.on_places_key(key) {
            return;
        }
        if self.dialog.is_some() {
            return self.on_dialog_key(key);
        }
        if self.menu.is_some() {
            return self.on_menu_key(key);
        }
        if self.tab.focus == Focus::Preview {
            return self.on_preview_key(key);
        }
        if self.commit_input.is_some() {
            return self.on_commit_key(key);
        }
        if self.branch_picker.is_some() {
            return self.on_branch_key(key);
        }
        if self.git_panel.is_some() {
            return self.on_git_panel_key(key);
        }
        if self.theme_picker.is_some() {
            return self.on_picker_key(key);
        }
        if self.help_open {
            self.help_open = false; // any key closes the overview
            return;
        }
        if self.rename.is_some() {
            self.on_rename_key(key);
            return;
        }
        if self.search_input.is_some() {
            return self.on_search_key(key);
        }
        if self.path_input.is_some() {
            return self.on_path_key(key);
        }
        if self.filter_editing {
            self.on_filter_key(key);
            return;
        }
        // Keys that are actions run exactly what the palette and the menus run.
        if let Some(action) = actions::for_key(key) {
            return self.run_action(action);
        }
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Alt+arrows move in space: ← / ↑ parent (an action), → enters the selected folder,
            // ↓ opens the selection. History is Ctrl+← / Ctrl+→ (fm-research LOG 2026-10-05).
            KeyCode::Right if alt => {
                if self.selected_entry().is_some_and(|e| e.is_dir) {
                    self.activate_selected();
                }
            }
            KeyCode::Down if alt => self.activate_selected(),
            KeyCode::Char('p') if ctrl => self.open_palette(),
            KeyCode::Char(' ') if ctrl => self.toggle_mark_here(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Esc if !self.tab.filter.is_empty() => self.set_filter(String::new()),
            KeyCode::Esc if self.tab.results.is_some() => self.go_up(),
            KeyCode::Esc => self.tab.marked.clear(),
            // Shift+arrows / Home / End / PgUp / PgDn: mark everything from where Shift started.
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
                if key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.extend_selection(key.code)
            }
            // In the grid, left/right move between tiles and up/down jump a row (like Nautilus).
            KeyCode::Right | KeyCode::Char('l') if self.drawn_view == View::Grid => {
                self.move_selection(1)
            }
            KeyCode::Left | KeyCode::Char('h') if self.drawn_view == View::Grid => {
                self.move_selection(-1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.click_anchor = None;
                self.move_selection(self.vertical_step())
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.click_anchor = None;
                self.move_selection(-self.vertical_step())
            }
            KeyCode::PageDown => self.move_selection(self.page()),
            KeyCode::PageUp => self.move_selection(-self.page()),
            KeyCode::Home | KeyCode::Char('g') => self.select_row(0),
            KeyCode::End | KeyCode::Char('G') => self.select_row(usize::MAX),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.activate_selected(),
            KeyCode::Left | KeyCode::Char('h') => self.go_up(),
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
            KeyCode::Backspace if self.tab.filter.is_empty() => self.filter_editing = false,
            KeyCode::Backspace => {
                let mut f = self.tab.filter.clone();
                f.pop();
                self.set_filter(f);
            }
            // Arrows still move the selection while typing.
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Char(c) if is_typing(key) => {
                let f = format!("{}{c}", self.tab.filter);
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
            MouseEventKind::ScrollDown
                if self.preview.area.contains((mouse.column, mouse.row).into()) =>
            {
                self.scroll_preview(3)
            }
            MouseEventKind::ScrollUp
                if self.preview.area.contains((mouse.column, mouse.row).into()) =>
            {
                self.scroll_preview(-3)
            }
            // The wheel over the tab row walks through the tabs.
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                if !self.tabs.chip_areas.is_empty() && self.tabs.chip_areas[0].y == mouse.row =>
            {
                let step = if mouse.kind == MouseEventKind::ScrollDown {
                    1
                } else {
                    -1
                };
                self.cycle_tab(step)
            }
            MouseEventKind::ScrollDown => self.move_selection(self.wheel_step()),
            MouseEventKind::ScrollUp => self.move_selection(-self.wheel_step()),
            MouseEventKind::Down(button) if self.tab_row_click(mouse.column, mouse.row, button) => {
            }
            MouseEventKind::Down(MouseButton::Left)
                if self.tab.term_mode == TermMode::Panel && mouse.row == self.term_area.y =>
            {
                self.dragging_panel = true; // grabbed the panel's top border
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging_panel => {
                self.term_height = Some(self.term_area.bottom().saturating_sub(mouse.row));
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging_panel => {
                self.dragging_panel = false
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.on_click(mouse.column, mouse.row, mouse.modifiers)
            }
            // Middle click on a folder (list or Places): open it in a new background tab.
            MouseEventKind::Down(MouseButton::Middle) => {
                if let Some(path) = self.folder_at(mouse.column, mouse.row) {
                    self.open_in_background_tab(path);
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.open_context_menu(mouse.column, mouse.row)
            }
            MouseEventKind::Drag(MouseButton::Left) if self.drag.is_some() => {
                self.on_drag(mouse.column, mouse.row)
            }
            MouseEventKind::Up(MouseButton::Left) if self.drag.is_some() => {
                let copy = mouse.modifiers.contains(KeyModifiers::CONTROL);
                self.on_drop(mouse.column, mouse.row, copy);
            }
            _ => return, // movement, other buttons: nothing to do
        }
        self.dirty = true;
    }

    fn on_click(&mut self, column: u16, row: u16, modifiers: KeyModifiers) {
        if self.menu.is_some() {
            let row_index = usize::from(row.saturating_sub(self.menu_area.y));
            let chosen = self
                .menu
                .as_ref()
                .and_then(|m| m.items.get(m.offset + row_index))
                .copied();
            self.menu = None;
            if self.menu_area.contains((column, row).into())
                && let Some(action) = chosen
                && self.action_available(action)
            {
                self.run_action(action);
            }
            return;
        }
        if let Some((selected, _)) = self.theme_picker {
            let inside = self.picker_area.contains((column, row).into());
            let index = usize::from(row.saturating_sub(self.picker_area.y));
            if inside && index < liman_widgets::theme::THEMES.len() {
                if index == selected {
                    self.save_theme();
                } else {
                    self.pick_theme(index);
                }
            }
            return;
        }
        if self.help_open {
            self.help_open = false;
            return;
        }
        if self.tab.term_mode == TermMode::Fullscreen {
            return;
        }
        if self.tab.term_mode == TermMode::Panel {
            let in_terminal = self.term_area.contains((column, row).into());
            self.tab.focus = if in_terminal {
                Focus::Terminal
            } else {
                Focus::Files
            };
            if in_terminal {
                return;
            }
        }
        if let Some(i) = self.sidebar_row_at(column, row) {
            // Keys go where you clicked: Del then means the sidebar item, never the selected
            // file in the list (a Del meant for the sidebar trashed ~/Projects).
            self.tab.focus = Focus::Places;
            self.on_sidebar_click(i, column);
            return;
        }
        if row == self.path_bar_area.y {
            let segments = self.path_segments();
            if let Some(i) = breadcrumb::segment_at(&segments, self.path_bar_area.x, column) {
                let path = segments[i].path.clone();
                if path != self.tab.cwd {
                    self.load(path);
                }
            }
            return;
        }
        if self.drawn_view == View::Detailed
            && let Some(key) =
                FileList::sort_key_at(self.list_area, ListMode::Detailed, column, row)
        {
            let descending = key == self.sort.key && !self.sort.descending;
            return self.set_sort(liman_core::sort::SortOrder { key, descending });
        }
        let Some(index) = self.entry_at(column, row) else {
            return;
        };
        // Ctrl+click: mark / unmark one; Shift+click: mark the range from the last click.
        if modifiers.contains(KeyModifiers::CONTROL) {
            let Some(path) = self.visible_entry(index).map(|e| e.path.clone()) else {
                return;
            };
            if !self.tab.marked.remove(&path) {
                self.tab.marked.insert(path);
            }
            self.tab.table.select(Some(index));
            self.click_anchor = Some(index);
            return;
        }
        if modifiers.contains(KeyModifiers::SHIFT) {
            let anchor = self
                .click_anchor
                .or(self.tab.table.selected())
                .unwrap_or(index);
            let (from, to) = (anchor.min(index), anchor.max(index));
            let paths: Vec<_> = self
                .visible_entries()
                .skip(from)
                .take(to - from + 1)
                .map(|e| e.path.clone())
                .collect();
            self.tab.marked.extend(paths);
            self.tab.table.select(Some(index));
            return;
        }
        self.click_anchor = Some(index);
        self.drag = Some(Drag {
            from: index,
            start: (column, row),
            active: false,
        });
        let now = Instant::now();
        let double = self
            .last_click
            .is_some_and(|(at, i)| i == index && now.duration_since(at) <= DOUBLE_CLICK);
        self.tab.table.select(Some(index));
        if double {
            self.last_click = None;
            self.activate_selected();
        } else {
            self.last_click = Some((now, index));
        }
    }

    /// Ctrl+P: every action, filtered by what you type.
    fn open_palette(&mut self) {
        self.menu = Some(Menu {
            items: actions::matching(""),
            selected: 0,
            query: Some(String::new()),
            at: (0, 0),
            offset: 0,
        });
    }

    /// Right click: actions for the entry under the mouse, or for the folder on empty space.
    fn open_context_menu(&mut self, column: u16, row: u16) {
        let items = match self.entry_at(column, row) {
            Some(index) => {
                let path = self.visible_entry(index).map(|e| e.path.clone());
                if path.is_some_and(|p| !self.tab.marked.contains(&p)) {
                    self.tab.marked.clear(); // right-click on an unmarked entry acts on that entry only
                }
                self.tab.table.select(Some(index));
                Action::ON_ENTRY.to_vec()
            }
            None if self.list_area.contains((column, row).into()) => Action::ON_FOLDER.to_vec(),
            None => return,
        };
        self.menu = Some(Menu {
            items,
            selected: 0,
            query: None,
            at: (column, row),
            offset: 0,
        });
    }

    fn on_menu_key(&mut self, key: KeyEvent) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let last = menu.items.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.menu = None,
            KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
            KeyCode::Down => menu.selected = (menu.selected + 1).min(last),
            KeyCode::Enter => {
                let chosen = menu.items.get(menu.selected).copied();
                self.menu = None;
                if let Some(action) = chosen
                    && self.action_available(action)
                {
                    self.run_action(action);
                }
            }
            KeyCode::Backspace if menu.query.is_some() => {
                let query = menu.query.as_mut().expect("checked");
                query.pop();
                menu.items = actions::matching(query);
                menu.selected = 0;
            }
            KeyCode::Char(c) if menu.query.is_some() && is_typing(key) => {
                let query = menu.query.as_mut().expect("checked");
                query.push(c);
                menu.items = actions::matching(query);
                menu.selected = 0;
            }
            _ => {}
        }
    }

    /// The visible entry under terminal cell (`column`, `row`) in the list or grid.
    fn entry_at(&self, column: u16, row: u16) -> Option<usize> {
        let hit = match self.drawn_view.list_mode() {
            Some(mode) => {
                FileList::row_at(self.list_area, self.tab.table.offset(), mode, column, row)
            }
            None => GridView::index_at(
                self.list_area,
                self.tab.table.offset(),
                self.drawn_grid_level,
                column,
                row,
            ),
        };
        hit.filter(|&i| i < self.tab.visible.len())
    }

    /// The folder under the mouse: a folder in the list or a place in the sidebar (Recent is a
    /// list, not a folder).
    fn folder_at(&self, column: u16, row: u16) -> Option<PathBuf> {
        let in_list = self
            .entry_at(column, row)
            .and_then(|i| self.visible_entry(i))
            .filter(|e| e.is_dir)
            .map(|e| e.path.clone());
        in_list.or_else(
            || match self.sidebar_item(self.sidebar_row_at(column, row)?) {
                SideItem::Place(i) => Some(&self.sidebar[i])
                    .filter(|p| p.kind != liman_core::SpecialDir::Recent)
                    .map(|p| p.path.clone()),
                SideItem::Dir(d) => Some(self.tree.rows()[d].path.clone()),
                SideItem::Label => None,
            },
        )
    }

    /// A drag becomes real after the mouse moved a couple of cells (a click may wobble).
    fn on_drag(&mut self, column: u16, row: u16) {
        if let Some(drag) = &mut self.drag
            && !drag.active
            && (column.abs_diff(drag.start.0) + row.abs_diff(drag.start.1)) >= 2
        {
            drag.active = true;
            self.message = Some(tr("Drop on a folder to move (hold Ctrl to copy)").into());
        }
    }

    /// Mouse released after a drag: move / copy onto the folder or place under the pointer.
    fn on_drop(&mut self, column: u16, row: u16, copy: bool) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        let Some(dragged) = self.visible_entry(drag.from).map(|e| e.path.clone()) else {
            return;
        };
        let sources: Vec<PathBuf> = if self.tab.marked.contains(&dragged) {
            self.targets()
        } else {
            vec![dragged]
        };
        if !copy && let Some(path) = sources.iter().find(|p| self.is_protected(p)) {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            self.message = Some(trf(
                "“{}” is protected: it cannot be removed or moved",
                &[&name],
            ));
            return;
        }
        match self.folder_at(column, row) {
            // Dropped on Trash: a real (confirmed, undoable) trash, not a plain move into it.
            Some(dest) if dest.starts_with(&self.trash_dir) => {
                self.dialog = Some(Dialog::ConfirmTrash {
                    paths: sources,
                    inside: None,
                });
            }
            Some(dest)
                if !sources.contains(&dest) && !sources.iter().any(|s| dest.starts_with(s)) =>
            {
                self.drop_onto(sources, dest, copy);
            }
            _ => self.message = Some(tr("Dropped outside a folder: nothing done").into()),
        }
    }

    // ---- navigation ----

    pub fn path_segments(&self) -> Vec<Segment> {
        breadcrumb::segments(&self.tab.cwd, &self.places.home)
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
                let (_, tile_h) = grid::tile_size(self.drawn_grid_level);
                usize::from(self.list_area.height / tile_h) * self.grid_columns()
            }
        };
        isize::try_from(entries).unwrap_or(1).max(1)
    }

    /// Keys while Places has focus: ↑↓ choose (titles are skipped), Enter opens and moves to the
    /// files. In the folder tree → opens a branch (or steps into it), ← closes it (or goes to the
    /// parent). Returns false for keys Places does not use (they work as usual).
    fn on_places_key(&mut self, key: KeyEvent) -> bool {
        let row = self.sidebar_selected;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.sidebar_selected = self.sidebar_step(row, -1),
            KeyCode::Down | KeyCode::Char('j') => self.sidebar_selected = self.sidebar_step(row, 1),
            KeyCode::Home | KeyCode::Char('g') => self.sidebar_selected = self.sidebar_step(0, 1),
            KeyCode::End | KeyCode::Char('G') => {
                self.sidebar_selected = self.sidebar_step(self.sidebar_len(), -1);
            }
            KeyCode::Delete => {
                if let SideItem::Place(i) = self.sidebar_item(row)
                    && self.sidebar[i].kind == liman_core::SpecialDir::Bookmark
                {
                    let path = self.sidebar[i].path.clone();
                    self.toggle_bookmark_for(path);
                }
            }
            KeyCode::Right | KeyCode::Char('l')
                if matches!(self.sidebar_item(row), SideItem::Dir(_)) =>
            {
                let SideItem::Dir(d) = self.sidebar_item(row) else {
                    return true;
                };
                let dir = self.tree.rows()[d].clone();
                if dir.expanded {
                    self.sidebar_selected = self.sidebar_step(row, 1);
                } else {
                    self.expand_tree(&dir.path);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let SideItem::Dir(d) = self.sidebar_item(row) {
                    let dir = self.tree.rows()[d].clone();
                    if dir.expanded {
                        self.tree.collapse(&dir.path);
                    } else if let Some(parent) = self.tree.rows()[..d]
                        .iter()
                        .rposition(|r| r.depth + 1 == dir.depth)
                    {
                        self.sidebar_selected = Sidebar::dir_row(self.sidebar.len(), parent);
                    }
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                match self.sidebar_item(row) {
                    SideItem::Place(i) => self.open_place(i),
                    SideItem::Dir(d) => self.load(self.tree.rows()[d].path.clone()),
                    SideItem::Label => return true,
                }
                self.tab.focus = Focus::Files;
            }
            _ => return false,
        }
        true
    }

    // ---- Places panel: quick access and the folder tree ----

    fn sidebar_len(&self) -> usize {
        Sidebar::len(self.sidebar.len(), self.tree.rows().len())
    }

    fn sidebar_item(&self, row: usize) -> SideItem {
        Sidebar::item(self.sidebar.len(), self.tree.rows().len(), row).unwrap_or(SideItem::Label)
    }

    fn sidebar_row_at(&self, column: u16, row: u16) -> Option<usize> {
        Sidebar::row_at(
            self.sidebar_area,
            self.sidebar_offset,
            self.sidebar_len(),
            column,
            row,
        )
    }

    /// The next selectable row from `from` in direction `step` (titles and the rule are skipped);
    /// `from` itself when there is none.
    fn sidebar_step(&self, from: usize, step: isize) -> usize {
        let mut row = from;
        loop {
            let Some(next) = row
                .checked_add_signed(step)
                .filter(|&r| r < self.sidebar_len())
            else {
                return from;
            };
            row = next;
            if self.sidebar_item(row) != SideItem::Label {
                return row;
            }
        }
    }

    /// A click in Places: a place opens; in the tree, the ▸/▾ opens or closes the branch and the
    /// name opens the folder.
    fn on_sidebar_click(&mut self, row: usize, column: u16) {
        match self.sidebar_item(row) {
            SideItem::Place(i) => self.open_place(i),
            SideItem::Dir(d) => {
                self.sidebar_selected = row;

                let dir = self.tree.rows()[d].clone();
                if column == Sidebar::arrow_column(self.sidebar_area, dir.depth) {
                    if dir.expanded {
                        self.tree.collapse(&dir.path);
                    } else {
                        self.expand_tree(&dir.path);
                    }
                } else {
                    self.load(dir.path);
                }
            }
            SideItem::Label => {}
        }
    }

    /// Opens a branch of the tree; its subfolders are read on a worker the first time.
    fn expand_tree(&mut self, dir: &std::path::Path) {
        if self.tree.expand(dir) {
            worker::spawn_tree_children(
                self.tx.clone(),
                dir.to_path_buf(),
                self.options.show_hidden,
            );
        }
    }

    /// After moving to `cwd`: its subfolders are known from the listing, and the tree opens the way
    /// down to it.
    fn update_tree(&mut self, mut dirs: Vec<(String, PathBuf)>) {
        dirs.sort_by(|a, b| liman_core::sort::natural_cmp(&a.0, &b.0));
        let dirs = dirs.into_iter().map(|(_, path)| path).collect();
        self.tree.set_children(self.tab.cwd.clone(), dirs);
        for dir in self.tree.reveal(&self.tab.cwd) {
            worker::spawn_tree_children(self.tx.clone(), dir, self.options.show_hidden);
        }
    }

    /// `t`: the theme list. Moving through it applies each theme at once (live preview).
    fn open_theme_picker(&mut self) {
        let current = liman_widgets::theme::current_index();
        self.theme_picker = Some((current, current));
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let Some((selected, original)) = self.theme_picker else {
            return;
        };
        let last = liman_widgets::theme::THEMES.len() - 1;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.pick_theme(selected.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.pick_theme((selected + 1).min(last)),
            KeyCode::Enter => self.save_theme(),
            KeyCode::Esc | KeyCode::Char('q') => {
                liman_widgets::theme::set_index(original);
                self.theme_picker = None;
            }
            _ => {}
        }
    }

    fn pick_theme(&mut self, index: usize) {
        if let Some((selected, _)) = &mut self.theme_picker {
            *selected = index;
            liman_widgets::theme::set_index(index);
        }
    }

    /// Keeps the previewed theme and writes it to the config file.
    fn save_theme(&mut self) {
        self.theme_picker = None;
        let name = liman_widgets::theme::palette().name;
        self.save_setting("theme", name);
        self.message = Some(trf("Theme: {}", &[&name]));
    }

    /// Opens sidebar place `i`: a folder, or the Recent list.
    fn open_place(&mut self, i: usize) {
        self.sidebar_selected = i + 1; // the row under the QUICK ACCESS title
        let place = self.sidebar[i].clone();
        if place.kind == liman_core::SpecialDir::Recent {
            self.show_recent();
        } else {
            self.load(place.path);
        }
    }

    /// Recently used files (GTK's list) as a results view.
    fn show_recent(&mut self) {
        let home = self.places.home.clone();
        let paths = liman_core::recent::recent_files(&home);
        if paths.is_empty() {
            self.message = Some(tr("No recent files").into());
            return;
        }
        let results = Results {
            command: tr("Recent files").into(),
            count: paths.len(),
            running: false,
        };
        self.load_results(results, home, paths);
    }

    /// Ctrl+D: bookmark the selected folder (or this folder), or remove it if already there.
    fn toggle_bookmark(&mut self) {
        let folder = match self.selected_entry() {
            Some(e) if e.is_dir => e.path.clone(),
            _ => self.tab.cwd.clone(),
        };
        self.toggle_bookmark_for(folder);
    }

    fn toggle_bookmark_for(&mut self, folder: PathBuf) {
        let name = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        if let Some(i) = self.bookmarks.iter().position(|b| *b == folder) {
            self.bookmarks.remove(i);
            self.message = Some(trf("Removed bookmark “{}”", &[&name]));
        } else {
            self.bookmarks.push(folder);
            self.message = Some(trf("Bookmarked “{}”", &[&name]));
        }
        self.sidebar = self.places.sidebar_with(&self.bookmarks);
        self.sidebar_selected = self.sidebar_selected.min(self.sidebar_len() - 1);
        if !cfg!(test) {
            let file = liman_core::config::bookmarks_path(&self.places.home);
            if let Err(e) = liman_core::config::save_bookmarks(&file, &self.bookmarks) {
                self.message = Some(trf("Bookmark not saved: {}", &[&e]));
            }
        }
    }

    /// Settings from the config file (theme is applied earlier, in `main`).
    pub fn apply_settings(&mut self, settings: &std::collections::BTreeMap<String, String>) {
        if !cfg!(test) {
            self.bookmarks = liman_core::config::load_bookmarks(
                &liman_core::config::bookmarks_path(&self.places.home),
            );
            self.sidebar = self.places.sidebar_with(&self.bookmarks);
        }
        if let Some(order) = settings
            .get("sort")
            .and_then(|s| liman_core::sort::SortOrder::from_config(s))
        {
            self.sort = order;
        }
        // `images = halfblocks` never asks the terminal for a graphics protocol (ADR 0009).
        self.preview.images_auto = settings.get("images").is_none_or(|v| v != "halfblocks");
        if settings.get("preview").is_some_and(|v| v == "true") {
            self.preview.shown = true;
        }
        if settings.get("hidden").is_some_and(|v| v == "true") && !self.options.show_hidden {
            self.options.show_hidden = true;
            self.load(self.tab.cwd.clone());
        }
    }

    fn save_setting(&mut self, key: &str, value: &str) {
        if cfg!(test) {
            return; // never touch the real config from tests
        }
        let file = liman_core::config::path(&self.places.home);
        if let Err(e) = liman_core::config::set(&file, key, value) {
            self.message = Some(trf("Setting not saved: {}", &[&e]));
        }
    }

    /// Ctrl+H: show or hide dot files (reloads, keeps the selection).
    fn toggle_hidden(&mut self) {
        self.options.show_hidden = !self.options.show_hidden;
        let value = if self.options.show_hidden {
            "true"
        } else {
            "false"
        };
        self.save_setting("hidden", value);
        self.message = Some(
            tr(if self.options.show_hidden {
                "Showing hidden files"
            } else {
                "Hiding hidden files"
            })
            .into(),
        );
        self.tab.select_after_load = self.selected_entry().map(|e| e.name.clone());
        if self.tab.results.is_none() {
            self.load(self.tab.cwd.clone());
        }
    }

    /// Re-sorts the listing in place (no reload) and keeps the selected entry selected.
    fn set_sort(&mut self, order: liman_core::sort::SortOrder) {
        self.save_setting("sort", &order.to_config());
        self.set_sort_quietly(order);
        let template = if order.descending {
            "Sorted by {}, descending"
        } else {
            "Sorted by {}"
        };
        self.message = Some(trf(template, &[&tr(order.key.name())]));
    }

    /// Sorts again, keeping the selected entry selected (no message, not saved).
    fn set_sort_quietly(&mut self, order: liman_core::sort::SortOrder) {
        self.sort = order;
        let keep = self.selected_entry().map(|e| e.path.clone());
        if let Listing::Ready(entries) = &mut self.tab.listing {
            liman_core::sort::sort_with(entries, order);
        }
        self.tab.names_lower.clear();
        self.tab.visible_for.clear();
        self.refresh_visible();
        let row = keep
            .and_then(|p| self.visible_entries().position(|e| e.path == p))
            .unwrap_or(0);
        self.select_row(row);
    }

    /// `v`: one key between the compact list (like cardea) and the large view last used.
    fn toggle_compact(&mut self) {
        if self.view == View::Detailed {
            self.view = self.last_large_view;
        } else {
            self.last_large_view = self.view;
            self.view = View::Detailed;
        }
    }

    /// Ctrl+↑ / Ctrl+↓: grow or shrink the terminal panel by `rows`.
    fn resize_panel(&mut self, rows: i16) {
        let current = self.term_height.unwrap_or(self.term_area.height);
        self.term_height = Some(current.saturating_add_signed(rows).max(4));
    }

    /// Zooms like a GUI file manager: Detailed → Normal → Grid, then larger and larger boxes.
    fn zoom(&mut self, step: i8) {
        let current = self.drawn_grid_level;
        let largest = grid::BOX_SIZES.len() - 1;
        match (self.view, step.signum()) {
            (View::Detailed, 1) => self.view = View::Normal,
            (View::Normal, 1) => {
                self.view = View::Grid;
                self.grid_level = None; // fit to the window first
            }
            (View::Grid, 1) => self.grid_level = Some((current + 1).min(largest)),
            (View::Grid, -1) if current == 0 => {
                self.view = View::Normal;
                self.grid_level = None;
            }
            (View::Grid, -1) => self.grid_level = Some(current - 1),
            (View::Normal, -1) => self.view = View::Detailed,
            _ => {}
        }
    }

    /// Tiles per row in the grid as drawn last frame.
    fn grid_columns(&self) -> usize {
        GridView::columns(self.list_area.width, self.drawn_grid_level)
    }

    /// One wheel notch: three lines in the lists, one tile row in the grid.
    fn wheel_step(&self) -> isize {
        match self.drawn_view {
            View::Grid => self.vertical_step(),
            _ => WHEEL_STEP,
        }
    }

    /// Rows move by one entry in the lists, by a whole tile row in the grid.
    fn vertical_step(&self) -> isize {
        match self.drawn_view {
            View::Grid => isize::try_from(self.grid_columns()).unwrap_or(1),
            _ => 1,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.tab.visible.is_empty() {
            return;
        }
        let current = self.tab.table.selected().unwrap_or(0);
        self.select_row(current.saturating_add_signed(delta));
    }

    /// Shift+movement: the anchor (where the range started: the last plain click or the selection
    /// when Shift was first pressed) stays, the cursor moves, and exactly the range is marked.
    fn extend_selection(&mut self, code: KeyCode) {
        let Some(current) = self.tab.table.selected() else {
            return;
        };
        let anchor = *self.click_anchor.get_or_insert(current);
        let grid = self.drawn_view == View::Grid;
        match code {
            KeyCode::Up => self.move_selection(-self.vertical_step()),
            KeyCode::Down => self.move_selection(self.vertical_step()),
            KeyCode::Left if grid => self.move_selection(-1),
            KeyCode::Right if grid => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-self.page()),
            KeyCode::PageDown => self.move_selection(self.page()),
            KeyCode::Home => self.select_row(0),
            KeyCode::End => self.select_row(usize::MAX),
            _ => return,
        }
        let to = self.tab.table.selected().unwrap_or(anchor);
        let (from, to) = (anchor.min(to), anchor.max(to));
        self.tab.marked = self
            .visible_entries()
            .skip(from)
            .take(to - from + 1)
            .map(|e| e.path.clone())
            .collect();
    }

    /// Selects `row`, clamped to the last row.
    fn select_row(&mut self, row: usize) {
        if self.tab.visible.is_empty() {
            self.tab.table.select(None);
        } else {
            self.tab
                .table
                .select(Some(row.min(self.tab.visible.len() - 1)));
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
                    Ok(()) => trf("Opening “{}”…", &[&name]),
                    Err(e) => trf("Cannot open “{}”: {}", &[&name, &e]),
                });
            }
            OpenPlan::Editor { program, path } => self.external = Some((program, path)),
            OpenPlan::Unavailable(why) => self.message = Some(why),
        }
    }

    /// A command run in the terminal finished: if it printed paths, show them up here.
    pub(super) fn on_command_finished(&mut self, out: crate::terminal::CommandOutput) {
        // A command that moved the shell (`cd /usr/share`) is navigation, not a list of files:
        // the view follows the folder instead.
        let shell_now = self.tab.terminal.as_ref().and_then(|t| t.cwd());
        if shell_now.is_some_and(|now| now != out.cwd) {
            return;
        }
        let text = liman_core::results::strip_ansi(&out.bytes);
        let paths = liman_core::results::paths_from_output(&text, &out.cwd);
        if paths.is_empty() {
            return;
        }
        // Typed fast (or pasted), the line may not have been echoed yet when Enter was pressed:
        // then the command is the first printed line that is not one of the results.
        let command = if out.command.is_empty() {
            text.lines()
                .map(crate::terminal::strip_prompt)
                .find(|l| {
                    !l.is_empty()
                        && !paths
                            .iter()
                            .any(|p| p.ends_with(l.trim_start_matches("./")))
                })
                .unwrap_or_else(|| "command".into())
        } else {
            out.command
        };
        let results = Results {
            command,
            count: paths.len(),
            running: false,
        };
        self.load_results(results, out.cwd, paths);
    }

    /// Ctrl+F: every key searches again (live, like cardea); matches stream into a results view.
    /// ↑↓ move in the results while typing, Enter keeps them, Esc goes back to the folder.
    fn on_search_key(&mut self, key: KeyEvent) {
        let Some(text) = &mut self.search_input else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.search_input = None;
                if self.tab.results.is_some() {
                    self.load(self.tab.cwd.clone());
                }
            }
            KeyCode::Enter => self.search_input = None,
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Backspace => {
                text.pop();
                self.search_live();
            }
            KeyCode::Char(c) if is_typing(key) => {
                text.push(c);
                self.search_live();
            }
            _ => {}
        }
    }

    /// Starts a search for the typed text (the older one stops: new generation). The view becomes
    /// an empty results list that fills as matches arrive.
    fn search_live(&mut self) {
        let needle = self.search_input.clone().unwrap_or_default();
        if needle.is_empty() {
            if self.tab.results.is_some() {
                self.load(self.tab.cwd.clone()); // nothing to search: back to the folder
            }
            return;
        }
        self.generation += 1;
        self.current_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        self.loading_path = None;
        self.results_pending = None;
        self.tab.results = Some(Results {
            command: trf("search “{}”", &[&needle]),
            count: 0,
            running: true,
        });
        self.tab.listing = Listing::Ready(Vec::new());
        self.tab.visible.clear();
        self.tab.names_lower.clear();
        self.tab.visible_for.clear();
        self.tab.filter.clear();
        self.tab.marked.clear();
        self.tab.table = TableState::default();
        // Search the folder itself, not inside a previous result list.
        worker::spawn_search(
            self.tx.clone(),
            self.generation,
            self.current_generation.clone(),
            self.tab.cwd.clone(),
            needle,
            self.options.show_hidden,
        );
    }

    fn on_search_found(&mut self, generation: u64, entries: Vec<Entry>, done: bool) {
        if generation != self.generation {
            return; // an older search
        }
        let Listing::Ready(list) = &mut self.tab.listing else {
            return;
        };
        list.extend(entries);
        let count = list.len();
        let dirs: Vec<PathBuf> = if done {
            list.iter()
                .filter(|e| e.is_dir)
                .map(|e| e.path.clone())
                .collect()
        } else {
            Vec::new()
        };
        if let Some(results) = &mut self.tab.results {
            results.count = count;
            results.running = !done;
        }
        self.refresh_visible();
        if self.tab.table.selected().is_none() {
            self.select_row(0);
        }
        if !dirs.is_empty() {
            worker::spawn_counts(
                self.tx.clone(),
                self.generation,
                self.current_generation.clone(),
                dirs,
                self.options,
            );
        }
        self.dirty = true;
    }

    /// Ctrl+L: the path bar becomes a text field with the current folder.
    pub(super) fn begin_path_input(&mut self) {
        // Under the home folder the path starts with `~/`, like in a shell.
        let mut text = match self.tab.cwd.strip_prefix(&self.places.home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => self.tab.cwd.display().to_string(),
        };
        if !text.ends_with('/') {
            text.push('/');
        }
        self.path_input = Some(text);
    }

    fn on_path_key(&mut self, key: KeyEvent) {
        use liman_core::path_input;
        let Some(text) = &mut self.path_input else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.path_input = None,
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Tab => {
                if let Some(done) = path_input::complete(text, &self.tab.cwd, &self.places.home) {
                    *text = done;
                }
            }
            KeyCode::Char(c) if is_typing(key) => text.push(c),
            KeyCode::Enter => {
                let typed = self.path_input.take().unwrap_or_default();
                let path = path_input::resolve(&typed, &self.tab.cwd, &self.places.home);
                // A file opens its folder with the file selected.
                match (path.is_dir(), path.parent(), path.file_name()) {
                    (true, _, _) => self.load(path),
                    (false, Some(dir), Some(name)) if path.exists() => {
                        self.tab.select_after_load = Some(name.to_string_lossy().into_owned());
                        self.load(dir.to_path_buf());
                    }
                    _ => self.message = Some(trf("No such file or folder: {}", &[&typed])),
                }
            }
            _ => {}
        }
    }

    fn go_back(&mut self) {
        if let Some(path) = self.tab.back_stack.pop() {
            self.tab.forward_stack.push(self.tab.cwd.clone());
            self.moving_in_history = true;
            self.load(path);
        }
    }

    fn go_forward(&mut self) {
        if let Some(path) = self.tab.forward_stack.pop() {
            self.tab.back_stack.push(self.tab.cwd.clone());
            self.moving_in_history = true;
            self.load(path);
        }
    }

    fn go_up(&mut self) {
        if self.tab.results.is_some() {
            return self.load(self.tab.cwd.clone()); // back from results to the folder
        }
        let Some(parent) = self.tab.cwd.parent().map(PathBuf::from) else {
            return;
        };
        self.tab.select_after_load = self
            .tab
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.load(parent);
    }

    fn set_filter(&mut self, filter: String) {
        self.tab.filter = filter;
        self.refresh_visible();
        self.select_row(0);
    }

    /// Recomputes `visible` from the filter. Lower-case names are made once per listing, when a
    /// filter is first typed; when the filter only got longer, the search stays inside the
    /// previous matches.
    fn refresh_visible(&mut self) {
        let Listing::Ready(entries) = &self.tab.listing else {
            self.tab.visible.clear();
            return;
        };
        let needle = self.tab.filter.to_lowercase();
        if needle.is_empty() {
            self.tab.visible = (0..entries.len()).collect();
            self.tab.visible_for.clear();
            return;
        }
        // Made on the first filter key, not for every listing.
        if self.tab.names_lower.len() != entries.len() {
            self.tab.names_lower = entries.iter().map(|e| e.name.to_lowercase()).collect();
        }
        let narrowing =
            !self.tab.visible_for.is_empty() && needle.starts_with(&self.tab.visible_for);
        let matches = |i: &usize| self.tab.names_lower[*i].contains(&needle);
        self.tab.visible = if narrowing {
            self.tab.visible.iter().copied().filter(matches).collect()
        } else {
            (0..entries.len()).filter(matches).collect()
        };
        self.tab.visible_for = needle;
    }

    // ---- worker results ----

    /// Folder child counts arrived (after the listing): fill them in; a size sort uses them.
    fn on_counts(
        &mut self,
        generation: u64,
        counts: Vec<(PathBuf, Option<usize>, Option<liman_core::FileType>)>,
        done: bool,
    ) {
        if generation != self.generation {
            return;
        }
        let Listing::Ready(entries) = &mut self.tab.listing else {
            return;
        };
        let counts: std::collections::HashMap<PathBuf, _> =
            counts.into_iter().map(|(p, n, t)| (p, (n, t))).collect();
        for entry in entries.iter_mut().filter(|e| e.is_dir) {
            if let Some((count, contents)) = counts.get(&entry.path) {
                entry.item_count = *count;
                entry.contents = *contents;
            }
        }
        // A size sort uses the counts. Sorting again moves the selection around, so it happens
        // once at the end, and at most once a second while a long count is still running.
        let due = done || self.counts_sorted.elapsed() >= COUNT_SORT_INTERVAL;
        if self.sort.key == liman_core::sort::SortKey::Size && self.tab.results.is_none() && due {
            self.counts_sorted = Instant::now();
            self.set_sort_quietly(self.sort);
        }
        self.dirty = true;
    }

    fn on_listing(&mut self, generation: u64, path: PathBuf, result: Result<Vec<Entry>, String>) {
        if generation != self.generation {
            return; // stale: a newer request is on its way
        }
        self.loading_path = None;
        self.tab.results = self.results_pending.take();
        if let (Some(results), Ok(entries)) = (&mut self.tab.results, &result) {
            results.count = entries.len();
        }
        let from_shell = std::mem::take(&mut self.load_from_shell);
        if result.is_ok() {
            self.sync_shell_to(&path, from_shell);
            if self.results_pending.is_none() && self.tab.results.is_none() {
                self.watch.watch(&path);
            }
        }
        // History: a move to another folder (not back/forward, not a reload) is a new step.
        let from_history = std::mem::take(&mut self.moving_in_history);
        if path != self.tab.cwd {
            if !from_history {
                self.tab.back_stack.push(self.tab.cwd.clone());
                self.tab.forward_stack.clear();
            }
            if self.tab.select_after_load.is_none() {
                self.tab.select_after_load = self.tab.remembered.get(&path).cloned();
            }
        }
        // A reload of the same folder keeps the counts it had until the new ones arrive (no flicker).
        // Counts of subfolders whose modification time is unchanged are still right: a folder's
        // time changes whenever something inside it is added, removed or renamed.
        type Known = (
            usize,
            Option<liman_core::FileType>,
            Option<std::time::SystemTime>,
        );
        let old_counts: std::collections::HashMap<PathBuf, Known> = match &self.tab.listing {
            Listing::Ready(old) if path == self.tab.cwd => old
                .iter()
                .filter_map(|e| Some((e.path.clone(), (e.item_count?, e.contents, e.modified))))
                .collect(),
            _ => Default::default(),
        };
        let mut to_count = Vec::new();
        self.tab.cwd = path;
        self.tab.listing = match result {
            Ok(mut entries) => {
                for entry in entries.iter_mut().filter(|e| e.is_dir) {
                    entry.special = self.places.kind_of(&entry.path);
                    let old = old_counts.get(&entry.path);
                    if let Some((count, contents, _)) = old {
                        entry.item_count = Some(*count);
                        entry.contents = *contents;
                    }
                    let unchanged = old.is_some_and(|(_, _, modified)| {
                        modified.is_some() && *modified == entry.modified
                    });
                    if !unchanged {
                        to_count.push(entry.path.clone());
                    }
                }
                // Folders come sorted by name; results keep the command's order.
                if self.results_pending.is_none()
                    && self.sort != liman_core::sort::SortOrder::default()
                {
                    liman_core::sort::sort_with(&mut entries, self.sort);
                }
                Listing::Ready(entries)
            }
            Err(err) => Listing::Failed(err),
        };
        if let (Listing::Ready(entries), None) = (&self.tab.listing, &self.tab.results) {
            let dirs = entries
                .iter()
                .filter(|e| e.is_dir)
                .map(|e| (e.name.clone(), e.path.clone()))
                .collect();
            self.update_tree(dirs);
        }
        if !to_count.is_empty() {
            self.counts_sorted = Instant::now();
            worker::spawn_counts(
                self.tx.clone(),
                self.generation,
                self.current_generation.clone(),
                to_count,
                self.options,
            );
        }
        self.tab.names_lower.clear();
        self.tab.visible_for.clear();
        self.refresh_visible();
        self.tab.table = TableState::default();
        let came_from = self.tab.select_after_load.take();
        let row = came_from
            .and_then(|name| self.visible_entries().position(|e| e.name == name))
            .unwrap_or(0);
        self.select_row(row);
        if self
            .tab
            .git
            .as_ref()
            .is_none_or(|g| !self.tab.cwd.starts_with(&g.root))
        {
            self.tab.git = None; // left the repository; the new status arrives soon
        }
        self.request_git();
        if std::mem::take(&mut self.rename_after_load) {
            self.begin_rename();
        }
        self.dirty = true;
    }

    pub fn entry_count(&self) -> Option<usize> {
        match &self.tab.listing {
            Listing::Ready(_) => Some(self.tab.visible.len()),
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
            contents: None,
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
        assert!(matches!(app.tab.listing, Listing::Loading));
    }

    #[test]
    fn enter_on_a_file_keeps_the_listing() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Char('G')));
        // Do not actually launch anything from a test: only check the folder did not change.
        let entry = app.selected_entry().unwrap().clone();
        assert!(!entry.is_dir);
        assert!(matches!(app.tab.listing, Listing::Ready(_)));
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
        assert_eq!(app.tab.cwd, PathBuf::from("/data"));
        assert_eq!(selected_name(&app), "Projects");
    }

    #[test]
    fn filter_narrows_the_list_and_esc_clears_it() {
        let (mut app, _rx) = app();
        app.handle(key(KeyCode::Char('/')));
        for c in ['P', 'r', 'o'] {
            app.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(app.tab.filter, "Pro");
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
        assert_eq!(app.tab.filter, "");
        assert_eq!(app.entry_count(), Some(5));
    }

    #[test]
    fn click_selects_and_double_click_opens() {
        let (mut app, _rx) = app();
        app.handle(click(30, 5)); // second row: Projects
        assert_eq!(selected_name(&app), "Projects");
        assert!(matches!(app.tab.listing, Listing::Ready(_)));
        app.handle(click(30, 5));
        assert!(matches!(app.tab.listing, Listing::Loading));
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
        let music = app.visible_entry(0).unwrap();
        assert_eq!(music.name, "Music");
        assert_eq!(music.special, Some(liman_core::SpecialDir::Music));
        assert_eq!(app.visible_entry(1).unwrap().special, None);
    }

    #[test]
    fn clicking_the_sidebar_or_path_bar_navigates() {
        let (mut app, _rx) = app();
        app.handle(click(5, 1)); // the QUICK ACCESS title: nothing
        assert!(!matches!(app.tab.listing, Listing::Loading));
        app.handle(click(5, 2)); // the first place: Home (/data)
        assert!(matches!(app.tab.listing, Listing::Loading));

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
        assert!(matches!(app.tab.listing, Listing::Loading));
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
        assert_eq!((app.view, app.grid_level), (View::Grid, None)); // grid, fitted to the window
        app.handle(key(KeyCode::Char('+'))); // larger boxes
        assert_eq!(app.grid_level, Some(1));
        app.drawn_grid_level = 1;
        app.handle(key(KeyCode::Char('-')));
        assert_eq!(app.grid_level, Some(0));
        app.drawn_grid_level = 0;
        app.handle(key(KeyCode::Char('-'))); // smallest boxes: back to the box list
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
    fn alt_arrows_enter_and_leave_folders_in_every_view() {
        let (mut app, _rx) = app();
        app.drawn_view = View::Grid;
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::ALT,
        ))));
        assert!(matches!(app.tab.listing, Listing::Loading)); // entered Music

        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/data/Projects"), places(), tx);
        app.drawn_view = View::Grid;
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::ALT,
        ))));
        assert_eq!(app.generation, 2); // asked for /data
    }

    #[test]
    fn v_switches_between_compact_list_and_large_view() {
        let (mut app, _rx) = app();
        assert_eq!(app.view, View::Grid);
        app.handle(key(KeyCode::Char('v')));
        assert_eq!(app.view, View::Detailed);
        app.handle(key(KeyCode::Char('v')));
        assert_eq!(app.view, View::Grid);
    }

    #[test]
    fn tab_cycles_places_files_and_places_keys_open_a_place() {
        let (mut app, _rx) = app();
        app.sidebar_area = Rect::new(0, 1, 22, 10);
        app.tab.focus = Focus::Places;
        app.handle(key(KeyCode::Tab));
        assert_eq!(app.tab.focus, Focus::Files);
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        ))));
        assert_eq!(app.tab.focus, Focus::Places);
        app.handle(key(KeyCode::Down)); // stays on the only place
        app.handle(key(KeyCode::Enter)); // opens Home (/data)
        assert_eq!(app.tab.focus, Focus::Files);
        assert!(matches!(app.tab.listing, Listing::Loading));
    }

    #[test]
    fn theme_picker_previews_and_esc_restores() {
        let (mut app, _rx) = app();
        let before = liman_widgets::theme::current_index();
        app.handle(key(KeyCode::Char('t')));
        app.handle(key(KeyCode::Down));
        assert_ne!(liman_widgets::theme::current_index(), before); // previewed
        app.handle(key(KeyCode::Esc));
        assert_eq!(liman_widgets::theme::current_index(), before);
        assert!(app.theme_picker.is_none());
    }

    #[test]
    fn ctrl_h_toggles_hidden_files() {
        let (mut app, _rx) = app();
        let ctrl_h = AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('h'),
            KeyModifiers::CONTROL,
        )));
        app.places.home = std::env::temp_dir().join("liman-test-no-config"); // keep the real config untouched
        app.handle(ctrl_h);
        assert!(app.options.show_hidden);
        assert!(matches!(app.tab.listing, Listing::Loading));
    }

    /// Delivers a listing of `path` with the given names (all folders) for the newest request.
    fn arrive(app: &mut App, path: &str, names: &[&str]) {
        let generation = app.generation;
        app.handle(AppEvent::Listing {
            generation,
            path: PathBuf::from(path),
            result: Ok(names.iter().map(|n| entry(n, true)).collect()),
        });
    }

    #[test]
    fn alt_left_right_are_up_and_into_never_back_into_a_folder() {
        let (mut app, _rx) = app(); // in /data, Music selected
        let alt = |code| AppEvent::Input(Event::Key(KeyEvent::new(code, KeyModifiers::ALT)));
        app.handle(key(KeyCode::Char('j'))); // Projects
        app.handle(alt(KeyCode::Right)); // into Projects
        arrive(&mut app, "/data/Projects", &["liman"]);
        app.handle(alt(KeyCode::Left)); // up to /data, Projects selected
        arrive(&mut app, "/data", &["Music", "Projects"]);
        assert_eq!(selected_name(&app), "Projects");
        app.handle(alt(KeyCode::Left)); // up again: /, not back into Projects
        arrive(&mut app, "/", &["data"]);
        assert_eq!(app.tab.cwd, PathBuf::from("/"));
    }

    #[test]
    fn shift_arrows_mark_a_range_from_where_shift_started() {
        let (mut app, _rx) = app(); // /data: Music, Projects
        arrive(&mut app, "/data", &["a", "b", "c", "d"]);
        app.drawn_view = View::Detailed;
        let shift = |code| AppEvent::Input(Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT)));
        app.handle(key(KeyCode::Char('j'))); // b
        app.handle(shift(KeyCode::Down)); // b..c
        app.handle(shift(KeyCode::Down)); // b..d
        let marked = |app: &App| {
            let mut names: Vec<String> = app
                .tab
                .marked
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(marked(&app), ["b", "c", "d"]);
        app.handle(shift(KeyCode::Up)); // back to b..c
        assert_eq!(marked(&app), ["b", "c"]);
        app.handle(shift(KeyCode::Home)); // a..b
        assert_eq!(marked(&app), ["a", "b"]);
    }

    #[test]
    fn ctrl_left_right_walk_the_history_and_restore_the_selection() {
        let (mut app, _rx) = app(); // in /data, Music selected
        app.handle(key(KeyCode::Char('j'))); // Projects
        app.handle(key(KeyCode::Enter));
        arrive(&mut app, "/data/Projects", &["liman"]);
        let alt = |code| AppEvent::Input(Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL)));
        app.handle(alt(KeyCode::Left)); // back
        arrive(&mut app, "/data", &["Music", "Projects"]);
        assert_eq!(app.tab.cwd, PathBuf::from("/data"));
        assert_eq!(selected_name(&app), "Projects"); // remembered
        app.handle(alt(KeyCode::Right)); // forward
        arrive(&mut app, "/data/Projects", &["liman"]);
        assert_eq!(app.tab.cwd, PathBuf::from("/data/Projects"));
        assert!(app.tab.forward_stack.is_empty());
        assert_eq!(app.tab.back_stack, [PathBuf::from("/data")]);
    }

    #[test]
    fn ctrl_f_searches_subfolders_into_a_results_view() {
        let dir = std::env::temp_dir().join(format!("liman-ctrlf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/report.pdf"), "x").unwrap();
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::CONTROL,
        ))));
        for c in "REPORT".chars() {
            app.handle(key(KeyCode::Char(c)));
        }
        // Live: the results view is there while typing, Enter only keeps it.
        assert!(app.tab.results.is_some());
        app.handle(key(KeyCode::Enter));
        assert!(app.search_input.is_none());
        while app.tab.results.as_ref().is_none_or(|r| r.running) {
            let ev = rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("search result");
            app.handle(ev);
        }
        assert_eq!(app.tab.results.as_ref().unwrap().count, 1);
        assert_eq!(app.visible_entry(0).unwrap().name, "sub/report.pdf");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ctrl_d_bookmarks_the_selected_folder_and_again_removes_it() {
        let (mut app, _rx) = app();
        let bookmarks_in_sidebar = |app: &App| {
            app.sidebar
                .iter()
                .filter(|p| p.kind == liman_core::SpecialDir::Bookmark)
                .count()
        };
        let ctrl_d = || {
            AppEvent::Input(Event::Key(KeyEvent::new(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL,
            )))
        };
        app.handle(key(KeyCode::Char('j'))); // Projects
        app.handle(ctrl_d());
        assert_eq!(app.bookmarks, [PathBuf::from("/data/Projects")]);
        assert_eq!(bookmarks_in_sidebar(&app), 1);
        assert!(
            app.sidebar
                .iter()
                .any(|p| p.kind == liman_core::SpecialDir::Bookmark && p.name == "Projects")
        );
        app.handle(ctrl_d());
        assert!(app.bookmarks.is_empty());
        assert_eq!(bookmarks_in_sidebar(&app), 0);
    }

    #[test]
    fn palette_runs_the_chosen_action_and_right_click_offers_entry_actions() {
        let (mut app, _rx) = app();
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ))));
        for c in "small".chars() {
            app.handle(key(KeyCode::Char(c)));
        }
        app.handle(key(KeyCode::Enter));
        assert!(app.menu.is_none());
        assert_eq!(app.view, View::Detailed); // "Small list / large view" ran

        app.drawn_view = View::Detailed;
        app.handle(AppEvent::Input(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 30,
            row: 5, // Projects
            modifiers: KeyModifiers::NONE,
        })));
        let menu = app.menu.as_ref().expect("context menu");
        assert_eq!(menu.items, Action::ON_ENTRY);
        assert_eq!(selected_name(&app), "Projects");
    }

    #[test]
    fn s_sorts_in_place_and_keeps_the_selection() {
        let (mut app, _rx) = app();
        app.sort.key = liman_core::sort::SortKey::Modified; // so 's' goes to Type next
        app.handle(key(KeyCode::Char('G'))); // c.rs
        app.set_sort(liman_core::sort::SortOrder {
            key: liman_core::sort::SortKey::Type,
            descending: true,
        });
        let names: Vec<_> = app.visible_entries().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["Music", "Projects", "a.txt", "c.rs", "b.pdf"]);
        assert_eq!(selected_name(&app), "c.rs");
    }

    #[test]
    fn ctrl_arrows_resize_the_panel_within_limits() {
        let (mut app, _rx) = app();
        app.tab.term_mode = TermMode::Panel;
        app.term_area = Rect::new(0, 20, 80, 10);
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::CONTROL,
        ))));
        assert_eq!(app.term_height, Some(12));
        for _ in 0..10 {
            app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
                KeyCode::Down,
                KeyModifiers::CONTROL,
            ))));
        }
        assert_eq!(app.term_height, Some(4));
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
        assert_eq!(app.tab.cwd, PathBuf::from("/data")); // left did not go up a folder
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
    fn folder_changes_already_in_the_listing_do_not_reload() {
        let (mut app, _rx) = app();
        let generation = app.generation;
        // A change seen before the listing was requested is in it: nothing to do.
        let before = app.listing_started - Duration::from_millis(1);
        app.handle(AppEvent::FolderChanged {
            dir: PathBuf::from("/data"),
            at: before,
        });
        assert_eq!(app.generation, generation);
        // A newer change reloads, even while that listing is still on its way.
        app.handle(AppEvent::FolderChanged {
            dir: PathBuf::from("/data"),
            at: Instant::now(),
        });
        assert_eq!(app.generation, generation + 1);
        app.handle(AppEvent::FolderChanged {
            dir: PathBuf::from("/data"),
            at: Instant::now(),
        });
        assert_eq!(app.generation, generation + 2);
        // Changes in the folder we are leaving do not cancel the move.
        app.handle(key(KeyCode::Enter)); // into Music
        let moving = app.generation;
        app.handle(AppEvent::FolderChanged {
            dir: PathBuf::from("/data"),
            at: Instant::now(),
        });
        assert_eq!(app.generation, moving);
    }

    #[test]
    fn rename_in_a_results_view_starts_from_the_file_name() {
        let (mut app, _rx) = app();
        let generation = app.generation;
        let mut found = entry("a.rs", false);
        found.name = "src/a.rs".into();
        found.path = PathBuf::from("/data/src/a.rs");
        app.handle(AppEvent::Listing {
            generation,
            path: PathBuf::from("/data"),
            result: Ok(vec![found]),
        });
        app.handle(key(KeyCode::F(2)));
        assert_eq!(app.rename.as_ref().map(|r| r.text.as_str()), Some("a.rs"));
    }

    #[test]
    fn ctrl_l_types_a_path_and_enter_goes_there() {
        let (mut app, _rx) = app();
        let ctrl_l = AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        )));
        app.handle(ctrl_l);
        assert_eq!(app.path_input.as_deref(), Some("~/")); // /data is home in these tests
        app.handle(key(KeyCode::Esc));
        assert_eq!(app.path_input, None);
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::CONTROL,
        ))));
        for _ in 0.."~/".len() {
            app.handle(key(KeyCode::Backspace));
        }
        app.handle(key(KeyCode::Char('/')));
        app.handle(key(KeyCode::Enter)); // "/" is a folder everywhere
        assert_eq!(app.path_input, None);
        assert_eq!(app.loading_path.as_deref(), Some(Path::new("/")));
    }

    #[test]
    fn the_folder_tree_opens_the_way_and_arrows_open_and_close_branches() {
        let dir = std::env::temp_dir().join(format!("liman-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.join("a"), Places::from_user_dirs("", &dir), tx);
        let names = |app: &App| -> Vec<String> {
            app.tree
                .rows()
                .iter()
                .map(|r| format!("{}{}", r.depth, r.name))
                .collect()
        };
        // The listing of `a` and the subfolders of home arrive: home is open down to `a`.
        while !names(&app).contains(&"1a".to_string()) {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        assert!(!names(&app).contains(&"2b".to_string()));
        let a = app.tree.rows().iter().position(|r| r.name == "a").unwrap();
        app.tab.focus = Focus::Places;
        app.sidebar_selected = Sidebar::dir_row(app.sidebar.len(), a);
        app.handle(key(KeyCode::Right)); // open `a`: its subfolders are known from its listing
        assert!(names(&app).contains(&"2b".to_string()));
        app.handle(key(KeyCode::Left)); // close it again
        assert!(!names(&app).contains(&"2b".to_string()));
        app.handle(key(KeyCode::Left)); // closed: go to the parent row
        assert_eq!(
            app.sidebar_selected,
            Sidebar::dir_row(app.sidebar.len(), a - 1)
        );
        std::fs::remove_dir_all(&dir).unwrap();
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
        assert!(matches!(app.tab.listing, Listing::Loading));
    }
}

#[cfg(test)]
mod count_tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn tab_never_opens_a_closed_terminal() {
        let dir = std::env::temp_dir();
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        app.sidebar_area = Rect::new(0, 0, 20, 10); // sidebar visible
        let tab = AppEvent::Input(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        app.handle(tab);
        assert!(app.tab.terminal.is_none());
        assert_eq!(app.tab.term_mode, TermMode::Hidden);
        assert_eq!(app.tab.focus, Focus::Places);
    }

    #[test]
    fn folder_counts_arrive_after_the_listing() {
        let dir = std::env::temp_dir().join(format!("liman-counts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["a", "b", "c"] {
            std::fs::write(dir.join("sub").join(f), "").unwrap();
        }
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        let count = |app: &App| match &app.tab.listing {
            Listing::Ready(e) => e[0].item_count,
            _ => None,
        };
        while count(&app).is_none() {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        assert_eq!(count(&app), Some(3));

        // A reload with "sub" untouched counts nothing again.
        let next = |app: &mut App| {
            let ev = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let counts = matches!(ev, AppEvent::Counts { .. });
            app.handle(ev);
            counts
        };
        app.refresh();
        while app.loading_path.is_some() {
            assert!(!next(&mut app), "unchanged folder was counted again");
        }
        let late = rx.recv_timeout(Duration::from_millis(200));
        assert!(!late.is_ok_and(|ev| matches!(ev, AppEvent::Counts { .. })));
        // A new file inside changes the folder's time: counted again.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("sub/d"), "").unwrap();
        app.refresh();
        while count(&app) != Some(4) {
            next(&mut app);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
