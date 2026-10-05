//! Terminal mode from the user's side: F4 bottom panel, Ctrl+O full screen, F6 focus,
//! and keeping the shell's folder and the file view's folder in step.

use liman_core::i18n::{tr, trf};
use std::path::PathBuf;

use ratatui::crossterm::event::KeyEvent;

use super::App;
use crate::terminal::{Terminal, shell_quote};

/// How often terminal output may trigger a look at the shell's folder.
const CWD_CHECK: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermMode {
    /// No terminal on screen (the shell may still be running in the background).
    Hidden,
    /// Dolphin-style panel under the files (F4).
    Panel,
    /// The terminal takes the whole window (Ctrl+O), like Midnight Commander.
    Fullscreen,
}

/// Which panel gets the keys. Tab / Shift+Tab cycle Places → Files → Preview → Terminal
/// (the preview and the terminal only when they are open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Places,
    Files,
    Preview,
    Terminal,
}

impl App {
    /// Keys go to the shell when the terminal is on screen and focused.
    pub(super) fn terminal_has_focus(&self) -> bool {
        match self.term_mode {
            TermMode::Hidden => false,
            TermMode::Panel => self.focus == Focus::Terminal,
            TermMode::Fullscreen => true,
        }
    }

    /// F4: open the panel (and focus the shell) or close it.
    pub(super) fn toggle_panel(&mut self) {
        match self.term_mode {
            TermMode::Panel => {
                self.term_mode = TermMode::Hidden;
                self.focus = Focus::Files;
            }
            TermMode::Hidden | TermMode::Fullscreen => {
                if self.ensure_terminal() {
                    self.term_mode = TermMode::Panel;
                    self.focus = Focus::Terminal;
                }
            }
        }
    }

    /// Ctrl+O: the terminal full screen, or back to the files.
    pub(super) fn toggle_fullscreen(&mut self) {
        if self.term_mode == TermMode::Fullscreen {
            self.term_mode = self.term_before_fullscreen;
            self.focus = Focus::Files;
        } else if self.ensure_terminal() {
            self.term_before_fullscreen = self.term_mode;
            self.term_mode = TermMode::Fullscreen;
            self.focus = Focus::Terminal;
        }
    }

    /// Tab: Places → Files → Terminal (opening the panel) → Places.
    pub(super) fn focus_next(&mut self) {
        self.preview.reader = false;
        // Closed panels are skipped, never opened by Tab (F3 / F4 open them).
        let terminal = self.term_mode == TermMode::Panel;
        let preview = self.preview.shown;
        self.focus = match self.focus {
            Focus::Places => Focus::Files,
            Focus::Files if preview => Focus::Preview,
            Focus::Files | Focus::Preview if terminal => Focus::Terminal,
            Focus::Files | Focus::Preview | Focus::Terminal => self.places_or_files(),
        };
    }

    /// Shift+Tab: the other way round.
    pub(super) fn focus_prev(&mut self) {
        self.preview.reader = false;
        let terminal = self.term_mode == TermMode::Panel;
        let preview = self.preview.shown;
        self.focus = match self.focus {
            Focus::Files => self.places_or_files(),
            Focus::Places if terminal => Focus::Terminal,
            Focus::Places | Focus::Terminal if preview => Focus::Preview,
            Focus::Places | Focus::Terminal | Focus::Preview => Focus::Files,
        };
    }

    /// Places, unless the sidebar is hidden (narrow window).
    fn places_or_files(&self) -> Focus {
        if self.sidebar_area.width > 0 {
            Focus::Places
        } else {
            Focus::Files
        }
    }

    /// In the terminal Tab completes; only on an empty command line does it move on.
    pub(super) fn terminal_line_empty(&self) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|t| t.typed_text().is_empty())
    }

    /// Alt+Enter: type the marked (or selected) paths into the shell's command line, quoted and
    /// relative to the shell's folder when inside it, then give the keyboard to the shell.
    pub(super) fn paths_to_terminal(&mut self) {
        let paths = self.targets();
        if paths.is_empty() || !self.ensure_terminal() {
            return;
        }
        if self.term_mode == TermMode::Hidden {
            self.term_mode = TermMode::Panel;
        }
        self.focus = Focus::Terminal;
        let term = self.terminal.as_mut().expect("ensured above");
        let base = term.cwd().unwrap_or_else(|| self.cwd.clone());
        let mut line = String::new();
        for path in &paths {
            let shown = path.strip_prefix(&base).unwrap_or(path);
            line.push_str(&shell_quote(&shown.to_string_lossy()));
            line.push(' ');
        }
        term.type_text(&line);
        self.marked.clear();
    }

    /// F6: move focus between the files and the panel.
    pub(super) fn switch_focus(&mut self) {
        if self.term_mode == TermMode::Panel {
            self.focus = match self.focus {
                Focus::Files | Focus::Places | Focus::Preview => Focus::Terminal,
                Focus::Terminal => Focus::Files,
            };
        }
    }

    /// Starts the shell in the current folder if it is not running. False if it cannot start.
    fn ensure_terminal(&mut self) -> bool {
        if self.terminal.is_some() {
            return true;
        }
        // Real size is set by the UI on the first frame.
        match Terminal::spawn(&self.cwd, 10, 80, self.tx.clone()) {
            Ok(term) => {
                self.last_shell_cwd = Some(self.cwd.clone());
                self.terminal = Some(term);
                true
            }
            Err(e) => {
                self.message = Some(trf("Cannot start a shell: {}", &[&e]));
                false
            }
        }
    }

    pub(super) fn on_term_key(&mut self, key: KeyEvent) {
        if let Some(term) = &mut self.terminal {
            term.send_key(key);
        }
    }

    /// Shell output: update the screen, and follow the shell if it changed folder (`cd` typed there).
    pub(super) fn on_term_output(&mut self, bytes: &[u8]) {
        let Some(term) = &mut self.terminal else {
            return;
        };
        term.process(bytes);
        self.dirty = true;
        // Long output (`find /`) comes in thousands of chunks: look at the shell's folder at most
        // every CWD_CHECK; the quiet event after the output checks once more.
        if self.cwd_checked.elapsed() >= CWD_CHECK {
            self.follow_shell_cwd();
        }
    }

    /// Opens the shell's current folder in the view if the user changed it there (`cd`).
    pub(super) fn follow_shell_cwd(&mut self) {
        self.cwd_checked = std::time::Instant::now();
        let Some(term) = &mut self.terminal else {
            return;
        };
        if term.is_syncing() {
            return; // the shell is on its way to where the view already is
        }
        let Some(shell_cwd) = term.cwd() else {
            return;
        };
        if self.last_shell_cwd.as_ref() == Some(&shell_cwd) {
            return;
        }
        self.last_shell_cwd = Some(shell_cwd.clone());
        if shell_cwd != self.cwd && self.loading_path.as_ref() != Some(&shell_cwd) {
            self.load_from_shell = true;
            self.load(shell_cwd);
        }
    }

    pub(super) fn on_term_exited(&mut self) {
        self.terminal = None;
        self.term_mode = TermMode::Hidden;
        self.focus = Focus::Files;
        self.message = Some(tr("The shell exited; F4 starts a new one").into());
    }

    /// After the file view moved to `path` by itself (not following the shell), take the shell along.
    pub(super) fn sync_shell_to(&mut self, path: &PathBuf, from_shell: bool) {
        if from_shell {
            return;
        }
        let Some(term) = &mut self.terminal else {
            return;
        };
        if self.last_shell_cwd.as_ref() != Some(path) {
            term.cd(path);
            self.last_shell_cwd = Some(path.clone());
        }
    }
}
