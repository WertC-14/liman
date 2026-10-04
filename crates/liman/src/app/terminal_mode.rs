//! Terminal mode from the user's side: F4 bottom panel, Ctrl+O full screen, F6 focus,
//! and keeping the shell's folder and the file view's folder in step.

use std::path::PathBuf;

use ratatui::crossterm::event::KeyEvent;

use super::App;
use crate::terminal::Terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermMode {
    /// No terminal on screen (the shell may still be running in the background).
    Hidden,
    /// Dolphin-style panel under the files (F4).
    Panel,
    /// The terminal takes the whole window (Ctrl+O), like Midnight Commander.
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Files,
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

    /// F6: move focus between the files and the panel.
    pub(super) fn switch_focus(&mut self) {
        if self.term_mode == TermMode::Panel {
            self.focus = match self.focus {
                Focus::Files => Focus::Terminal,
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
                self.message = Some(format!("Cannot start a shell: {e}"));
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
        self.message = Some("The shell exited; F4 starts a new one".into());
    }

    /// After the file view moved to `path` by itself (not following the shell), take the shell along.
    pub(super) fn sync_shell_to(&mut self, path: &PathBuf, from_shell: bool) {
        if from_shell {
            return;
        }
        let Some(term) = &mut self.terminal else {
            return;
        };
        if self.last_shell_cwd.as_ref() != Some(path) && term.cd(path) {
            self.last_shell_cwd = Some(path.clone());
        }
    }
}
