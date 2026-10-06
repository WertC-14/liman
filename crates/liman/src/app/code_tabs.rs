//! Code tabs (fm-research ADR 0011): a text file opened from liman gets its own tab, right of the
//! folder tabs, holding a fener editor (`fener-core` + `fener-widgets`). The whole area under the
//! tab row is the editor; liman's panels are not shown. Alt+1 is always back to liman.
//!
//! Keys of an active code tab go to the editor, except Alt+1…9 (tabs). The editor's slow work
//! (Find Files, grep) runs on worker threads and comes back as [`AppEvent::CodeItems`].

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fener_core::picker::{self, Item, Kind};
use fener_core::{Document, Editor, Key, Mode, Request, State, files, tree};
use liman_core::FileType;
use liman_core::i18n::trf;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use super::App;
use crate::event::AppEvent;

/// Files bigger than this open the old way (desktop app / `$EDITOR`), not in a code tab.
const MAX_SIZE: u64 = 20 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_HITS: usize = 2_000;

pub struct CodeTab {
    /// Stays the same while tabs before it close (worker results find their tab by it).
    pub id: u64,
    pub editor: Editor,
    pub view: fener_widgets::View,
}

#[derive(Default)]
pub struct CodeTabs {
    pub tabs: Vec<CodeTab>,
    /// The code tab shown, or `None` when a folder tab is.
    pub active: Option<usize>,
    next_id: u64,
    /// Bumped by each worker job; a job that sees it change stops.
    generation: Arc<AtomicU64>,
    /// Tests set this: recent files are not written to the real home folder.
    pub(super) no_state: bool,
}

impl CodeTabs {
    pub fn active_editor(&self) -> Option<&Editor> {
        self.active.map(|i| &self.tabs[i].editor)
    }
}

/// Whether `entry` opens in a code tab: text, code and config files of a sensible size.
pub fn opens_in_code_tab(entry: &liman_core::Entry) -> bool {
    !entry.is_dir
        && matches!(
            entry.file_type,
            FileType::Code | FileType::Config | FileType::Text
        )
        && entry.size <= MAX_SIZE
}

impl App {
    /// Opens `path` in a code tab (the existing one if it is open already) and shows it.
    pub(super) fn open_code_tab(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self
            .code
            .tabs
            .iter()
            .position(|t| t.editor.doc.path() == Some(path.as_path()))
        {
            self.code.active = Some(i);
            return;
        }
        let doc = match Document::open(&path) {
            Ok(doc) => doc,
            Err(e) => {
                self.message = Some(trf("Cannot open “{}”: {}", &[&path.display(), &e]));
                return;
            }
        };
        let mut editor = Editor::new(doc);
        editor.root = tree::project_root(&path);
        editor.state = State::load();
        // The folder the file is in, beside it (the user asked to see it; fener alone keeps
        // the tree closed, as LazyVim).
        editor.show_tree(false);
        editor.remember(&path);
        self.code.next_id += 1;
        self.code.tabs.push(CodeTab {
            id: self.code.next_id,
            editor,
            view: fener_widgets::View::default(),
        });
        let i = self.code.tabs.len() - 1;
        self.code.active = Some(i);
        self.code_requests(i);
    }

    /// Alt+N or a click on chip N: folder tabs first, then code tabs.
    pub(super) fn switch_any_tab(&mut self, to: usize) {
        let folders = self.tabs.count();
        if to < folders {
            self.code.active = None;
            self.switch_tab(to);
        } else if to - folders < self.code.tabs.len() {
            self.code.active = Some(to - folders);
        }
        self.dirty = true;
    }

    /// Closes code tab `i`. Unsaved changes keep it open unless `force` (`:q!` already chose
    /// to drop them).
    pub(super) fn close_code_tab(&mut self, i: usize, force: bool) {
        let tab = &self.code.tabs[i];
        if tab.editor.doc.is_modified() && !force {
            self.code.active = Some(i);
            self.code.tabs[i].editor.message =
                Some("No write since last change (:w saves, :q! drops the changes)".into());
            return;
        }
        self.code.tabs.remove(i);
        self.code.active = match self.code.active {
            Some(a) if a == i => None,
            Some(a) if a > i => Some(a - 1),
            other => other,
        };
    }

    /// Before liman quits: the first code tab with unsaved changes, shown with a warning.
    pub(super) fn unsaved_code_tab(&mut self) -> bool {
        let Some(i) = self
            .code
            .tabs
            .iter()
            .position(|t| t.editor.doc.is_modified())
        else {
            return false;
        };
        self.code.active = Some(i);
        self.code.tabs[i].editor.message =
            Some("Unsaved changes: :w saves, :q! drops them, then quit liman again".into());
        self.dirty = true;
        true
    }

    /// A key while a code tab is shown.
    pub(super) fn on_code_key(&mut self, key: KeyEvent) {
        self.dirty = true;
        if let KeyCode::Char(c @ '1'..='9') = key.code
            && key.modifiers.contains(KeyModifiers::ALT)
        {
            return self.switch_any_tab(c as usize - '1' as usize);
        }
        let Some(i) = self.code.active else {
            return;
        };
        let Some(key) = convert(key) else {
            return;
        };
        self.code.tabs[i].editor.handle_key(key);
        self.code_requests(i);
        // fener sets `quit` only when nothing is unsaved or `:q!` said to drop it.
        if self.code.tabs[i].editor.quit {
            self.close_code_tab(i, true);
        }
    }

    pub(super) fn on_code_paste(&mut self, text: &str) {
        let Some(i) = self.code.active else {
            return;
        };
        for c in text.chars() {
            let key = if c == '\n' { Key::Enter } else { Key::Char(c) };
            self.code.tabs[i].editor.handle_key(key);
        }
        self.dirty = true;
    }

    /// A worker's list for a code tab's picker.
    pub(super) fn on_code_items(&mut self, id: u64, kind: Kind, query: &str, items: Vec<Item>) {
        if let Some(tab) = self.code.tabs.iter_mut().find(|t| t.id == id) {
            tab.editor.receive(kind, query, items);
            self.dirty = true;
        }
    }

    /// Does what code tab `i`'s editor asked for (state on the spot, lists on a worker).
    fn code_requests(&mut self, i: usize) {
        let tab = &mut self.code.tabs[i];
        for request in std::mem::take(&mut tab.editor.requests) {
            if request == Request::SaveState {
                // Remembering is a convenience: a read-only home folder must not stop editing.
                if !self.code.no_state {
                    let _ = tab.editor.state.save();
                }
                continue;
            }
            let (id, root, tx) = (tab.id, tab.editor.root.clone(), self.tx.clone());
            let mine = self.code.generation.fetch_add(1, Ordering::Relaxed) + 1;
            let generation = Arc::clone(&self.code.generation);
            let cancel = move || generation.load(Ordering::Relaxed) != mine;
            std::thread::spawn(move || {
                let (kind, query, items) = match request {
                    Request::ListFiles => {
                        let paths = files::list(&root, MAX_FILES, &cancel);
                        (Kind::Files, String::new(), picker::file_items(&root, paths))
                    }
                    Request::Grep(query) => {
                        let hits = files::grep(&root, &query, MAX_HITS, &cancel);
                        (Kind::Grep, query, picker::grep_items(&root, hits))
                    }
                    Request::SaveState => return,
                };
                if !cancel() {
                    let _ = tx.send(AppEvent::CodeItems {
                        id,
                        kind,
                        query,
                        items,
                    });
                }
            });
        }
    }

    /// Draws the active code tab into `area`; returns where the terminal cursor goes.
    pub fn render_code_tab(
        &mut self,
        buf: &mut ratatui::buffer::Buffer,
        area: Rect,
    ) -> Option<(u16, u16)> {
        let i = self.code.active?;
        let theme = theme_from_liman();
        let tab = &mut self.code.tabs[i];
        fener_widgets::render(buf, area, &mut tab.editor, &mut tab.view, &theme)
    }

    /// Insert mode and the command line want a bar cursor; `None` when no code tab is shown.
    pub fn code_cursor_bar(&self) -> Option<bool> {
        let editor = self.code.active_editor()?;
        Some(matches!(
            editor.mode,
            Mode::Insert | Mode::Command | Mode::Search
        ))
    }
}

/// fener's colors made from liman's theme, so a code tab looks like the rest of liman.
fn theme_from_liman() -> fener_widgets::Theme {
    use liman_widgets::theme;
    let p = theme::palette();
    fener_widgets::Theme {
        name: p.name,
        bg: p.bg,
        bg_dark: p.bar_bg,
        bg_line: p.selected_bg,
        bg_visual: p.marked_bg,
        bg_search: p.border,
        fg: p.fg,
        fg_dim: p.dim,
        gutter: p.border,
        orange: theme::type_color(FileType::Presentation),
        blue: theme::accent(),
        green: theme::type_color(FileType::Spreadsheet),
        magenta: theme::type_color(FileType::Code),
        yellow: theme::type_color(FileType::Archive),
        red: theme::type_color(FileType::Pdf),
        cyan: theme::type_color(FileType::Image),
    }
}

/// A terminal key as fener's key (`None` for keys the editor does not use).
fn convert(key: KeyEvent) -> Option<Key> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char(c) if ctrl => Key::Ctrl(c.to_ascii_lowercase()),
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Esc => Key::Esc,
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Tab => Key::Tab,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        _ => return None,
    })
}
