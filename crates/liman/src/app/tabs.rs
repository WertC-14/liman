//! Tabs (ADR 0008). The active tab's state lives in the `App` fields themselves, so the rest of
//! the code does not know about tabs; the other tabs are parked as [`TabState`] and swapped in
//! when the user switches. Each tab has its own folder, history, selection and shell.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use liman_core::i18n::tr;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use super::{App, Focus, Listing, Results, TermMode};
use crate::terminal::Terminal;

/// What belongs to one tab. The active one is `App::tab`; the others wait in `Tabs`.
/// When adding state to `App`, decide: per tab (here) or shared (`App`).
pub struct TabState {
    pub cwd: PathBuf,
    pub listing: Listing,
    /// Indices into the `Ready` entries that pass the filter, in display order.
    pub visible: Vec<usize>,
    /// Selected row (index into `visible`) and scroll offset; kept between frames.
    pub table: TableState,
    /// Folders visited before / after the current one (Ctrl+← / Ctrl+→).
    pub back_stack: Vec<PathBuf>,
    pub forward_stack: Vec<PathBuf>,
    /// Last selected entry per folder, restored when coming back.
    pub remembered: HashMap<PathBuf, String>,
    /// Entries marked with Space (or Ctrl+A) for a multi-item operation.
    pub marked: HashSet<PathBuf>,
    /// Set while the view shows files found by a shell command instead of a folder.
    pub results: Option<Results>,
    /// The embedded shell, started on first F4 / Ctrl+O and kept running while hidden.
    pub terminal: Option<Terminal>,
    pub term_mode: TermMode,
    pub term_before_fullscreen: TermMode,
    pub focus: Focus,
    /// The shell's folder as last seen, so each `cd` in the shell is followed once.
    pub last_shell_cwd: Option<PathBuf>,
    /// Git status of the repository around the open folder (None outside one).
    pub git: Option<liman_core::git::GitStatus>,
    /// After going up a level, the folder we came from gets selected.
    pub select_after_load: Option<String>,
}

impl TabState {
    /// A new tab showing `cwd` (listed when it becomes active).
    pub fn fresh(cwd: PathBuf) -> Self {
        Self {
            cwd,
            listing: Listing::Loading,
            visible: Vec::new(),
            table: TableState::default(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            remembered: HashMap::new(),
            marked: HashSet::new(),
            results: None,
            terminal: None,
            term_mode: TermMode::Hidden,
            term_before_fullscreen: TermMode::Hidden,
            focus: Focus::Files,
            last_shell_cwd: None,
            git: None,
            select_after_load: None,
        }
    }
}

/// One chip of the tab row, as the UI draws it.
pub struct TabLabel {
    pub title: String,
    pub has_shell: bool,
    pub active: bool,
}

/// The tab row: one slot per tab; the active slot is `None` (its state is in `App`).
#[derive(Default)]
pub struct Tabs {
    slots: Vec<Option<TabState>>,
    active: usize,
    /// Where each chip and the `+` were drawn. Written by the UI.
    pub chip_areas: Vec<Rect>,
    pub plus_area: Rect,
}

impl Tabs {
    pub fn count(&self) -> usize {
        self.slots.len().max(1)
    }
}

fn tab_title(cwd: &std::path::Path, home: &std::path::Path) -> String {
    if cwd == home {
        return format!("⌂ {}", tr("Home"));
    }
    cwd.file_name()
        .map_or_else(|| "/".into(), |n| n.to_string_lossy().into_owned())
}

impl App {
    /// Labels for the tab row (empty with a single tab: no row is drawn).
    pub fn tab_labels(&self) -> Vec<TabLabel> {
        if self.tabs.count() < 2 {
            return Vec::new();
        }
        let home = &self.places.home;
        self.tabs
            .slots
            .iter()
            .map(|slot| match slot {
                Some(t) => TabLabel {
                    title: tab_title(&t.cwd, home),
                    has_shell: t.terminal.is_some(),
                    active: false,
                },
                None => TabLabel {
                    title: tab_title(&self.tab.cwd, home),
                    has_shell: self.tab.terminal.is_some(),
                    active: true,
                },
            })
            .collect()
    }

    /// Ctrl+T: a new tab next to this one, in the same folder.
    pub(super) fn new_tab(&mut self) {
        if self.tabs.slots.is_empty() {
            self.tabs.slots.push(None);
        }
        let parked = self.park();
        let at = self.tabs.active;
        self.tabs.slots[at] = Some(parked);
        self.tabs.slots.insert(at + 1, None);
        self.tabs.active = at + 1;
        let fresh = TabState::fresh(self.tab.cwd.clone());
        self.unpark(fresh);
    }

    /// Middle click on a folder: a new tab for it next to this one, in the background (like a
    /// browser link), so the user stays where they are.
    pub(super) fn open_in_background_tab(&mut self, path: PathBuf) {
        if self.tabs.slots.is_empty() {
            self.tabs.slots.push(None);
        }
        let at = self.tabs.active;
        self.tabs.slots.insert(at + 1, Some(TabState::fresh(path)));
        self.message = Some(tr("Opened in a new tab (Alt+2… or the wheel on the tabs)").into());
        self.dirty = true;
    }

    /// Ctrl+W: closes this tab (its shell ends with it). The last tab stays.
    pub(super) fn close_tab(&mut self) {
        if self.tabs.count() < 2 {
            self.message = Some(tr("This is the last tab (q quits)").into());
            return;
        }
        drop(self.park()); // the shell (if any) is killed when its Terminal drops
        let at = self.tabs.active;
        self.tabs.slots.remove(at);
        let next = at.min(self.tabs.slots.len() - 1);
        self.tabs.active = next;
        let state = self.tabs.slots[next].take().expect("parked tab");
        self.unpark(state);
        if self.tabs.slots.len() == 1 {
            self.tabs.slots.clear(); // back to a single tab: no row
        }
    }

    /// Alt+1…9, the wheel over the tab row, a click on a chip.
    pub(super) fn switch_tab(&mut self, to: usize) {
        if to == self.tabs.active || to >= self.tabs.slots.len() {
            return;
        }
        let parked = self.park();
        let from = self.tabs.active;
        self.tabs.slots[from] = Some(parked);
        let state = self.tabs.slots[to].take().expect("parked tab");
        self.tabs.active = to;
        self.unpark(state);
    }

    pub(super) fn cycle_tab(&mut self, step: isize) {
        let n = self.tabs.slots.len();
        if n < 2 {
            return;
        }
        let to = (self.tabs.active as isize + step).rem_euclid(n as isize) as usize;
        self.switch_tab(to);
    }

    /// Takes the active tab's state out of `App` (a fresh tab in the same folder stays behind).
    fn park(&mut self) -> TabState {
        let cwd = self.tab.cwd.clone();
        std::mem::replace(&mut self.tab, TabState::fresh(cwd))
    }

    /// Makes `t` the active tab; work in flight for the old tab is dropped (new generation) and
    /// the folder is read again in place.
    fn unpark(&mut self, t: TabState) {
        self.tab = t;

        // Things that belonged to the moment, not to the tab.
        self.loading_path = None;
        self.results_pending = None;
        self.load_from_shell = false;
        self.moving_in_history = false;
        self.rename = None;
        self.search_input = None;
        self.path_input = None;
        self.commit_input = None;
        self.git_panel = None;
        self.branch_picker = None;
        self.menu = None;
        self.click_anchor = None;
        self.drag = None;
        self.dirty = true;

        if matches!(self.tab.listing, Listing::Loading) {
            self.load(self.tab.cwd.clone());
        } else if self.tab.results.is_none() {
            self.refresh(); // new generation: listings for the old tab are ignored
            self.watch.watch(&self.tab.cwd.clone());
        } else {
            self.generation += 1;
            self.current_generation
                .store(self.generation, std::sync::atomic::Ordering::Relaxed);
        }
        self.request_git();
        self.invalidate_preview();
    }

    /// A click on the tab row: chip = switch (middle button = close), `+` = new tab.
    /// Returns false when the click was not on the row.
    pub(super) fn tab_row_click(
        &mut self,
        column: u16,
        row: u16,
        button: ratatui::crossterm::event::MouseButton,
    ) -> bool {
        use ratatui::crossterm::event::MouseButton;
        let pos = (column, row).into();
        if self.tabs.plus_area.contains(pos) {
            self.new_tab();
            return true;
        }
        let Some(i) = self.tabs.chip_areas.iter().position(|r| r.contains(pos)) else {
            return false;
        };
        self.switch_tab(i);
        if button == MouseButton::Middle {
            self.close_tab();
        }
        self.dirty = true;
        true
    }

    /// Output, quiet or exit of a shell that is not the active tab's.
    pub(super) fn on_background_terminal(&mut self, id: u64, event: BackgroundTerm) {
        let Some(tab) = self
            .tabs
            .slots
            .iter_mut()
            .flatten()
            .find(|t| t.terminal.as_ref().is_some_and(|term| term.id == id))
        else {
            return;
        };
        match event {
            BackgroundTerm::Output(bytes) => {
                if let Some(term) = &mut tab.terminal {
                    term.process(&bytes);
                }
            }
            BackgroundTerm::Quiet => {
                if let Some(term) = &mut tab.terminal {
                    term.on_quiet();
                    let _ = term.take_finished_command(); // results only for the tab in view
                }
            }
            BackgroundTerm::Exited => {
                tab.terminal = None;
                tab.term_mode = TermMode::Hidden;
                tab.focus = Focus::Files;
                self.dirty = true; // the ● on the chip goes away
            }
        }
        // Output of a hidden shell changes nothing on screen: no redraw (a build running in a
        // background tab would otherwise cost 60 frames a second).
    }
}

pub(super) enum BackgroundTerm {
    Output(Vec<u8>),
    Quiet,
    Exited,
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Listing};
    use crate::event::AppEvent;
    use liman_core::Places;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(code, modifiers))));
    }

    fn settle(app: &mut App, rx: &Receiver<AppEvent>) {
        while !matches!(app.tab.listing, Listing::Ready(_)) || app.loading_path.is_some() {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
    }

    #[test]
    fn each_tab_keeps_its_folder() {
        let dir = std::env::temp_dir().join(format!("liman-tabs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/inner.txt"), "").unwrap();
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        settle(&mut app, &rx);
        assert!(app.tab_labels().is_empty(), "one tab: no tab row");

        press(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL);
        settle(&mut app, &rx);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE); // into "sub" in tab 2
        settle(&mut app, &rx);
        assert_eq!(app.tab.cwd, dir.join("sub"));
        let labels = app.tab_labels();
        assert_eq!(labels.len(), 2);
        assert!(labels[1].active && labels[1].title == "sub");

        press(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
        settle(&mut app, &rx);
        assert_eq!(app.tab.cwd, dir);
        press(&mut app, KeyCode::Char('2'), KeyModifiers::ALT);
        settle(&mut app, &rx);
        assert_eq!(app.tab.cwd, dir.join("sub"));
        assert_eq!(app.selected_entry().unwrap().name, "inner.txt");

        press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
        settle(&mut app, &rx);
        assert_eq!(app.tab.cwd, dir);
        assert!(app.tab_labels().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod middle_click_tests {
    use crate::app::App;
    use liman_core::Places;
    use std::path::PathBuf;
    use std::sync::mpsc;

    #[test]
    fn a_background_tab_waits_next_to_the_current_one() {
        let (tx, _rx) = mpsc::channel();
        let dir = std::env::temp_dir();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        app.open_in_background_tab(PathBuf::from("/usr"));
        let labels = app.tab_labels();
        assert_eq!(labels.len(), 2);
        assert!(labels[0].active, "the user stays in the first tab");
        assert_eq!(labels[1].title, "usr");
        assert_eq!(app.tab.cwd, dir);
    }
}
