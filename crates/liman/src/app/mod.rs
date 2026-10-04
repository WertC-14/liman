//! Application state. Rendering reads it, events change it.

mod actions;
mod file_ops;
mod git_ops;
mod preview;
mod tabs;
mod terminal_mode;

pub use actions::{Action, Menu};
pub use file_ops::{ClipMode, Clipboard, Dialog, JobStatus, RenameInput};
pub use git_ops::GitPanel;
pub use preview::{PreviewKey, PreviewPane};
pub use tabs::{TabLabel, Tabs};
pub use terminal_mode::{Focus, TermMode};

use liman_core::i18n::{tr, trf};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use liman_core::job::Done;
use liman_core::{Entry, ListOptions, Place, Places, trash};
use liman_widgets::breadcrumb::{self, Segment};
use liman_widgets::grid::{self, GridView};
use liman_widgets::{FileList, ListMode, Sidebar};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::event::AppEvent;
use crate::open::{self, OpenPlan};
use crate::terminal::Terminal;
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
    /// Folders visited before / after the current one (Alt+← / Alt+→).
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    /// The listing on its way comes from back/forward (do not touch the stacks).
    moving_in_history: bool,
    /// Last selected entry per folder, restored when coming back.
    remembered: std::collections::HashMap<PathBuf, String>,
    /// Order of folder listings (`s` / `S` / header click); saved in the config.
    pub sort: liman_core::sort::SortOrder,
    /// Lower-case entry names, made once per listing for the filter.
    names_lower: Vec<String>,
    /// The (lower-case) filter `visible` was computed for.
    visible_for: String,
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
    /// Highlighted place when the Places panel has focus.
    pub sidebar_selected: usize,
    /// Where the path bar was drawn last frame. Written by the UI.
    pub path_bar_area: Rect,
    pub places: Places,
    pub sidebar: Vec<Place>,
    /// Folders bookmarked with Ctrl+D (shown in Places with ★).
    pub bookmarks: Vec<PathBuf>,
    /// Entries marked with Space (or Ctrl+A) for a multi-item operation.
    pub marked: HashSet<PathBuf>,
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
    /// The embedded shell, started on first F4 / Ctrl+O and kept running while hidden.
    pub terminal: Option<Terminal>,
    pub term_mode: TermMode,
    term_before_fullscreen: TermMode,
    pub focus: Focus,
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
    /// Git status of the repository around the open folder (None outside one).
    pub git: Option<liman_core::git::GitStatus>,
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
    /// Newest listing/search id, shared with search workers so old searches stop early.
    current_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// `t`: the theme list (selected row, theme to go back to on Esc).
    pub theme_picker: Option<(usize, usize)>,
    /// `?`: the shortcut overview.
    pub help_open: bool,
    /// Where the theme list was drawn (for clicks). Written by the UI.
    pub picker_area: Rect,
    /// Set while the view shows files found by a shell command instead of a folder.
    pub results: Option<Results>,
    /// Results on their way from the worker (becomes `results` when the listing arrives).
    results_pending: Option<Results>,
    /// The shell's folder as last seen, so each `cd` in the shell is followed once.
    last_shell_cwd: Option<PathBuf>,
    /// When the shell's folder was last read (throttles it during long output).
    cwd_checked: Instant,
    /// Folder of the listing on its way, if any.
    loading_path: Option<PathBuf>,
    /// The listing on its way was started by following the shell (do not send `cd` back).
    load_from_shell: bool,
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
            names_lower: Vec::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            moving_in_history: false,
            remembered: std::collections::HashMap::new(),
            sort: liman_core::sort::SortOrder::default(),
            visible_for: String::new(),
            filter_editing: false,
            message: None,
            external: None,
            // The grid of type-colored boxes is what sets liman apart, so it is the first thing you see.
            view: View::Grid,
            drawn_view: View::Detailed,
            grid_level: None,
            drawn_grid_level: 0,
            terminal: None,
            term_mode: TermMode::Hidden,
            term_before_fullscreen: TermMode::Hidden,
            focus: Focus::Files,
            term_area: Rect::default(),
            term_height: None,
            dragging_panel: false,
            last_large_view: View::Grid,
            click_anchor: None,
            drag: None,
            results: None,
            theme_picker: None,
            search_input: None,
            dialog: None,
            watch: crate::watch::FolderWatch::start(tx.clone()),
            git: None,
            git_busy: false,
            git_panel: None,
            commit_input: None,
            branch_picker: None,
            git_again: false,
            preview: PreviewPane::default(),
            tabs: Tabs::default(),
            menu: None,
            menu_area: Rect::default(),
            rename_after_load: false,
            current_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            help_open: false,
            picker_area: Rect::default(),
            results_pending: None,
            last_shell_cwd: None,
            cwd_checked: Instant::now(),
            loading_path: None,
            load_from_shell: false,
            list_area: Rect::default(),
            sidebar_area: Rect::default(),
            sidebar_selected: 0,
            path_bar_area: Rect::default(),
            sidebar: places.sidebar_with(&[]),
            bookmarks: Vec::new(),
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
        worker::spawn_git_status(self.tx.clone(), self.cwd.clone());
    }

    /// Reads the open folder again without blanking the view: the old list stays until the new one
    /// arrives; selection, marks and filter are kept.
    pub fn refresh(&mut self) {
        self.invalidate_preview();
        if self.select_after_load.is_none() {
            self.select_after_load = self.selected_entry().map(|e| e.name.clone());
        }
        self.generation += 1;
        self.current_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        self.loading_path = Some(self.cwd.clone());
        worker::spawn_listing(
            self.tx.clone(),
            self.generation,
            self.cwd.clone(),
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
        if self.results.is_none()
            && let Some(entry) = self.selected_entry()
        {
            self.remembered.insert(self.cwd.clone(), entry.name.clone());
        }
        self.generation += 1;
        self.current_generation
            .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        self.listing = Listing::Loading;
        self.visible.clear();
        self.marked.clear();
        self.filter.clear();
        self.filter_editing = false;
        self.dirty = true;
        self.loading_path = Some(path.to_path_buf());
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
            AppEvent::FolderChanged(dir) => {
                if dir == self.cwd && self.results.is_none() && self.loading_path.is_none() {
                    self.refresh();
                }
            }
            AppEvent::Git { dir, status } => {
                self.git_busy = false;
                if dir == self.cwd {
                    self.git = status;
                    self.dirty = true;
                }
                if std::mem::take(&mut self.git_again) {
                    self.request_git();
                }
            }
            AppEvent::Counts { generation, counts } => self.on_counts(generation, counts),
            AppEvent::GitDone { label, result } => self.on_git_done(label, result),
            AppEvent::Preview { key, preview } => self.on_preview(key, *preview),
            AppEvent::TermQuiet(_) => {
                let Some(term) = &mut self.terminal else {
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
        self.terminal.as_ref().is_some_and(|t| t.id == id)
    }

    /// Entries currently shown, in display order.
    pub fn visible_entries(&self) -> impl Iterator<Item = &Entry> + '_ {
        let entries: &[Entry] = match &self.listing {
            Listing::Ready(entries) => entries,
            _ => &[],
        };
        self.visible.iter().filter_map(|&i| entries.get(i))
    }

    /// The entry on display row `row`.
    pub fn visible_entry(&self, row: usize) -> Option<&Entry> {
        let Listing::Ready(entries) = &self.listing else {
            return None;
        };
        self.visible.get(row).map(|&i| &entries[i])
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
        // Terminal mode keys work everywhere, whichever side has focus.
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::F(4) => return self.toggle_panel(),
            KeyCode::PageUp if ctrl => return self.cycle_tab(-1),
            KeyCode::PageDown if ctrl => return self.cycle_tab(1),
            KeyCode::F(3) => return self.toggle_preview(),
            KeyCode::Char('o') if ctrl => return self.toggle_fullscreen(),
            KeyCode::F(6) if self.term_mode == TermMode::Panel => return self.switch_focus(),
            KeyCode::Up if ctrl && self.term_mode == TermMode::Panel => {
                return self.resize_panel(2);
            }
            KeyCode::Down if ctrl && self.term_mode == TermMode::Panel => {
                return self.resize_panel(-2);
            }
            // Tab / Shift+Tab move between Places, Files and the terminal panel. In the terminal,
            // Tab completes while there is text on the command line.
            KeyCode::Tab
                if !self.terminal_has_focus()
                    || (self.term_mode == TermMode::Panel && self.terminal_line_empty()) =>
            {
                return self.focus_next();
            }
            KeyCode::BackTab if self.term_mode != TermMode::Fullscreen => return self.focus_prev(),
            _ => {}
        }
        if self.terminal_has_focus() {
            return self.on_term_key(key);
        }
        if self.focus == Focus::Places && self.on_places_key(key) {
            return;
        }
        if self.dialog.is_some() {
            return self.on_dialog_key(key);
        }
        if self.menu.is_some() {
            return self.on_menu_key(key);
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
        if self.filter_editing {
            self.on_filter_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            // Folder navigation that works in every view (GUI file manager keys).
            // Like Nautilus / a browser: Alt+← back, Alt+→ forward, Alt+↑ parent, Alt+↓ open.
            KeyCode::Char('t') if ctrl => self.new_tab(),
            KeyCode::Char('w') if ctrl => self.close_tab(),
            KeyCode::Char(c @ '1'..='9') if alt => self.switch_tab(c as usize - '1' as usize),
            KeyCode::Left if alt => self.go_back(),
            KeyCode::Right if alt => self.go_forward(),
            KeyCode::Up if alt => self.go_up(),
            KeyCode::Down if alt => self.activate_selected(),
            // GUI shortcuts: Ctrl+C copies, so quitting is q or Ctrl+Q.
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('c') if ctrl => self.copy_to_clipboard(ClipMode::Copy),
            KeyCode::Char('x') if ctrl => self.copy_to_clipboard(ClipMode::Cut),
            KeyCode::Char('v') if ctrl => self.paste(),
            KeyCode::Char('a') if ctrl => self.mark_all(),
            KeyCode::Char('z') if ctrl => self.undo(),
            KeyCode::Delete if key.modifiers.contains(KeyModifiers::SHIFT) => self.ask_delete(),
            KeyCode::Delete => self.trash_targets(),
            KeyCode::F(2) => self.begin_rename(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Esc if !self.filter.is_empty() => self.set_filter(String::new()),
            KeyCode::Esc if self.results.is_some() => self.go_up(),
            KeyCode::Esc => self.marked.clear(),
            KeyCode::Enter if alt => self.paths_to_terminal(),
            KeyCode::Char('h') if ctrl => self.toggle_hidden(),
            KeyCode::Char('.') => self.toggle_hidden(),
            KeyCode::Char('f') if ctrl => self.search_input = Some(String::new()),
            KeyCode::Char('d') if ctrl => self.toggle_bookmark(),
            KeyCode::Char('p') if ctrl => self.open_palette(),
            KeyCode::Char('g') if ctrl => self.toggle_git_panel(),
            // Ctrl+Shift+N / Ctrl+Shift+C arrive as Ctrl+N / Ctrl+C in most terminals: use Ctrl+N, Alt+C.
            KeyCode::Char('n') if ctrl => self.new_folder(),
            KeyCode::Char('c') if alt => self.copy_paths_osc52(),
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
            KeyCode::Char('v') => self.toggle_compact(),
            KeyCode::Char('s') => {
                let order = liman_core::sort::SortOrder {
                    key: self.sort.key.next(),
                    descending: false,
                };
                self.set_sort(order);
            }
            KeyCode::Char('S') => {
                let order = liman_core::sort::SortOrder {
                    descending: !self.sort.descending,
                    ..self.sort
                };
                self.set_sort(order);
            }
            KeyCode::Char('t') => self.open_theme_picker(),
            KeyCode::Char('?') => self.help_open = true,
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
            KeyCode::Char(c) if is_typing(key) => {
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
            MouseEventKind::ScrollDown => self.move_selection(self.wheel_step()),
            MouseEventKind::ScrollUp => self.move_selection(-self.wheel_step()),
            MouseEventKind::Down(button) if self.tab_row_click(mouse.column, mouse.row, button) => {
            }
            MouseEventKind::Down(MouseButton::Left)
                if self.term_mode == TermMode::Panel && mouse.row == self.term_area.y =>
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
        if self.term_mode == TermMode::Fullscreen {
            return;
        }
        if self.term_mode == TermMode::Panel {
            let in_terminal = self.term_area.contains((column, row).into());
            self.focus = if in_terminal {
                Focus::Terminal
            } else {
                Focus::Files
            };
            if in_terminal {
                return;
            }
        }
        if let Some(i) = Sidebar::row_at(self.sidebar_area, self.sidebar.len(), column, row) {
            self.sidebar_selected = i;
            self.open_place(i);
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
            if !self.marked.remove(&path) {
                self.marked.insert(path);
            }
            self.table.select(Some(index));
            self.click_anchor = Some(index);
            return;
        }
        if modifiers.contains(KeyModifiers::SHIFT) {
            let anchor = self.click_anchor.or(self.table.selected()).unwrap_or(index);
            let (from, to) = (anchor.min(index), anchor.max(index));
            let paths: Vec<_> = self
                .visible_entries()
                .skip(from)
                .take(to - from + 1)
                .map(|e| e.path.clone())
                .collect();
            self.marked.extend(paths);
            self.table.select(Some(index));
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
        self.table.select(Some(index));
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
                if path.is_some_and(|p| !self.marked.contains(&p)) {
                    self.marked.clear(); // right-click on an unmarked entry acts on that entry only
                }
                self.table.select(Some(index));
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
            Some(mode) => FileList::row_at(self.list_area, self.table.offset(), mode, column, row),
            None => GridView::index_at(
                self.list_area,
                self.table.offset(),
                self.drawn_grid_level,
                column,
                row,
            ),
        };
        hit.filter(|&i| i < self.visible.len())
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
        let sources: Vec<PathBuf> = if self.marked.contains(&dragged) {
            self.targets()
        } else {
            vec![dragged]
        };
        let folder = self
            .entry_at(column, row)
            .and_then(|i| self.visible_entry(i))
            .filter(|e| e.is_dir)
            .map(|e| e.path.clone())
            .or_else(|| {
                Sidebar::row_at(self.sidebar_area, self.sidebar.len(), column, row)
                    .map(|i| &self.sidebar[i])
                    .filter(|p| p.kind != liman_core::SpecialDir::Recent)
                    .map(|p| p.path.clone())
            });
        match folder {
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
                let (_, tile_h) = grid::tile_size(self.drawn_grid_level);
                usize::from(self.list_area.height / tile_h) * self.grid_columns()
            }
        };
        isize::try_from(entries).unwrap_or(1).max(1)
    }

    /// Keys while Places has focus: ↑↓ choose, Enter/→ opens and moves to the files.
    /// Returns false for keys Places does not use (they work as usual).
    fn on_places_key(&mut self, key: KeyEvent) -> bool {
        let last = self.sidebar.len().saturating_sub(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.sidebar_selected = self.sidebar_selected.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.sidebar_selected = (self.sidebar_selected + 1).min(last)
            }
            KeyCode::Home | KeyCode::Char('g') => self.sidebar_selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.sidebar_selected = last,
            KeyCode::Delete => {
                if let Some(place) = self.sidebar.get(self.sidebar_selected)
                    && place.kind == liman_core::SpecialDir::Bookmark
                {
                    let path = place.path.clone();
                    self.toggle_bookmark_for(path);
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if self.sidebar_selected < self.sidebar.len() {
                    self.open_place(self.sidebar_selected);
                    self.focus = Focus::Files;
                }
            }
            _ => return false,
        }
        true
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
        self.sidebar_selected = i;
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
        };
        self.load_results(results, home, paths);
    }

    /// Ctrl+D: bookmark the selected folder (or this folder), or remove it if already there.
    fn toggle_bookmark(&mut self) {
        let folder = match self.selected_entry() {
            Some(e) if e.is_dir => e.path.clone(),
            _ => self.cwd.clone(),
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
        self.sidebar_selected = self
            .sidebar_selected
            .min(self.sidebar.len().saturating_sub(1));
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
        if settings.get("preview").is_some_and(|v| v == "true") {
            self.preview.shown = true;
        }
        if settings.get("hidden").is_some_and(|v| v == "true") && !self.options.show_hidden {
            self.options.show_hidden = true;
            self.load(self.cwd.clone());
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
        self.select_after_load = self.selected_entry().map(|e| e.name.clone());
        if self.results.is_none() {
            self.load(self.cwd.clone());
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
        if let Listing::Ready(entries) = &mut self.listing {
            liman_core::sort::sort_with(entries, order);
        }
        self.names_lower.clear();
        self.visible_for.clear();
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
        let shell_now = self.terminal.as_ref().and_then(|t| t.cwd());
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
        };
        self.load_results(results, out.cwd, paths);
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        let Some(text) = &mut self.search_input else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.search_input = None,
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) if is_typing(key) => text.push(c),
            KeyCode::Enter => {
                let needle = self.search_input.take().unwrap_or_default();
                if needle.is_empty() {
                    return;
                }
                // Search the folder itself, not inside a previous result list.
                let base = self.cwd.clone();
                self.results_pending = Some(Results {
                    command: trf("search “{}”", &[&needle]),
                    count: 0, // filled in when the listing arrives
                });
                self.load_from_shell = true;
                self.start_loading(&base);
                worker::spawn_search(
                    self.tx.clone(),
                    self.generation,
                    self.current_generation.clone(),
                    base,
                    needle,
                    self.options.show_hidden,
                );
            }
            _ => {}
        }
    }

    fn go_back(&mut self) {
        if let Some(path) = self.back_stack.pop() {
            self.forward_stack.push(self.cwd.clone());
            self.moving_in_history = true;
            self.load(path);
        }
    }

    fn go_forward(&mut self) {
        if let Some(path) = self.forward_stack.pop() {
            self.back_stack.push(self.cwd.clone());
            self.moving_in_history = true;
            self.load(path);
        }
    }

    fn go_up(&mut self) {
        if self.results.is_some() {
            return self.load(self.cwd.clone()); // back from results to the folder
        }
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

    /// Recomputes `visible` from the filter. Lower-case names are made once per listing; when the
    /// filter only got longer, the search stays inside the previous matches.
    fn refresh_visible(&mut self) {
        let Listing::Ready(entries) = &self.listing else {
            self.visible.clear();
            return;
        };
        if self.names_lower.len() != entries.len() {
            self.names_lower = entries.iter().map(|e| e.name.to_lowercase()).collect();
        }
        let needle = self.filter.to_lowercase();
        let narrowing = !self.visible_for.is_empty() && needle.starts_with(&self.visible_for);
        let matches = |i: &usize| needle.is_empty() || self.names_lower[*i].contains(&needle);
        self.visible = if narrowing {
            self.visible.iter().copied().filter(matches).collect()
        } else {
            (0..entries.len()).filter(matches).collect()
        };
        self.visible_for = needle;
    }

    // ---- worker results ----

    /// Folder child counts arrived (after the listing): fill them in; a size sort uses them.
    fn on_counts(&mut self, generation: u64, counts: Vec<(PathBuf, Option<usize>)>) {
        if generation != self.generation {
            return;
        }
        let Listing::Ready(entries) = &mut self.listing else {
            return;
        };
        let counts: std::collections::HashMap<PathBuf, Option<usize>> =
            counts.into_iter().collect();
        for entry in entries.iter_mut().filter(|e| e.is_dir) {
            if let Some(count) = counts.get(&entry.path) {
                entry.item_count = *count;
            }
        }
        if self.sort.key == liman_core::sort::SortKey::Size && self.results.is_none() {
            self.set_sort_quietly(self.sort);
        }
        self.dirty = true;
    }

    fn on_listing(&mut self, generation: u64, path: PathBuf, result: Result<Vec<Entry>, String>) {
        if generation != self.generation {
            return; // stale: a newer request is on its way
        }
        self.loading_path = None;
        self.results = self.results_pending.take();
        if let (Some(results), Ok(entries)) = (&mut self.results, &result) {
            results.count = entries.len();
        }
        let from_shell = std::mem::take(&mut self.load_from_shell);
        if result.is_ok() {
            self.sync_shell_to(&path, from_shell);
            if self.results_pending.is_none() && self.results.is_none() {
                self.watch.watch(&path);
            }
        }
        // History: a move to another folder (not back/forward, not a reload) is a new step.
        let from_history = std::mem::take(&mut self.moving_in_history);
        if path != self.cwd {
            if !from_history {
                self.back_stack.push(self.cwd.clone());
                self.forward_stack.clear();
            }
            if self.select_after_load.is_none() {
                self.select_after_load = self.remembered.get(&path).cloned();
            }
        }
        // A reload of the same folder keeps the counts it had until the new ones arrive (no flicker).
        let old_counts: std::collections::HashMap<PathBuf, usize> = match &self.listing {
            Listing::Ready(old) if path == self.cwd => old
                .iter()
                .filter_map(|e| Some((e.path.clone(), e.item_count?)))
                .collect(),
            _ => Default::default(),
        };
        self.cwd = path;
        self.listing = match result {
            Ok(mut entries) => {
                for entry in entries.iter_mut().filter(|e| e.is_dir) {
                    entry.special = self.places.kind_of(&entry.path);
                    entry.item_count = old_counts.get(&entry.path).copied();
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
        if let Listing::Ready(entries) = &self.listing {
            let dirs: Vec<PathBuf> = entries
                .iter()
                .filter(|e| e.is_dir)
                .map(|e| e.path.clone())
                .collect();
            if !dirs.is_empty() {
                worker::spawn_counts(
                    self.tx.clone(),
                    self.generation,
                    self.current_generation.clone(),
                    dirs,
                    self.options,
                );
            }
        }
        self.names_lower.clear();
        self.visible_for.clear();
        self.refresh_visible();
        self.table = TableState::default();
        let came_from = self.select_after_load.take();
        let row = came_from
            .and_then(|name| self.visible_entries().position(|e| e.name == name))
            .unwrap_or(0);
        self.select_row(row);
        if self
            .git
            .as_ref()
            .is_none_or(|g| !self.cwd.starts_with(&g.root))
        {
            self.git = None; // left the repository; the new status arrives soon
        }
        self.request_git();
        if std::mem::take(&mut self.rename_after_load) {
            self.begin_rename();
        }
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
        let music = app.visible_entry(0).unwrap();
        assert_eq!(music.name, "Music");
        assert_eq!(music.special, Some(liman_core::SpecialDir::Music));
        assert_eq!(app.visible_entry(1).unwrap().special, None);
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
        assert!(matches!(app.listing, Listing::Loading)); // entered Music

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
        app.focus = Focus::Places;
        app.handle(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Files);
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        ))));
        assert_eq!(app.focus, Focus::Places);
        app.handle(key(KeyCode::Down)); // stays on the only place
        app.handle(key(KeyCode::Enter)); // opens Home (/data)
        assert_eq!(app.focus, Focus::Files);
        assert!(matches!(app.listing, Listing::Loading));
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
        assert!(matches!(app.listing, Listing::Loading));
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
    fn alt_left_right_walk_the_history_and_restore_the_selection() {
        let (mut app, _rx) = app(); // in /data, Music selected
        app.handle(key(KeyCode::Char('j'))); // Projects
        app.handle(key(KeyCode::Enter));
        arrive(&mut app, "/data/Projects", &["liman"]);
        let alt = |code| AppEvent::Input(Event::Key(KeyEvent::new(code, KeyModifiers::ALT)));
        app.handle(alt(KeyCode::Left)); // back
        arrive(&mut app, "/data", &["Music", "Projects"]);
        assert_eq!(app.cwd, PathBuf::from("/data"));
        assert_eq!(selected_name(&app), "Projects"); // remembered
        app.handle(alt(KeyCode::Right)); // forward
        arrive(&mut app, "/data/Projects", &["liman"]);
        assert_eq!(app.cwd, PathBuf::from("/data/Projects"));
        assert!(app.forward_stack.is_empty());
        assert_eq!(app.back_stack, [PathBuf::from("/data")]);
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
        app.handle(key(KeyCode::Enter));
        while app.results.is_none() {
            let ev = rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("search result");
            app.handle(ev);
        }
        assert_eq!(app.results.as_ref().unwrap().count, 1);
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
        app.term_mode = TermMode::Panel;
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
        assert_eq!(app.cwd, PathBuf::from("/data")); // left did not go up a folder
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

#[cfg(test)]
mod count_tests {
    use super::*;
    use std::sync::mpsc;

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
        let count = |app: &App| match &app.listing {
            Listing::Ready(e) => e[0].item_count,
            _ => None,
        };
        while count(&app).is_none() {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        assert_eq!(count(&app), Some(3));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
