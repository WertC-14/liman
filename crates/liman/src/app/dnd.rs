//! Drag and drop with other apps (fm-research ADR 0014).
//!
//! - Out: Alt+F (or dragging an entry to the edge of the window) puts the files on the clipboard
//!   as files; Ctrl+V in a browser, a chat or a file manager pastes them (ADR 0017: no ripdrag).
//! - In: files dropped on the terminal arrive as a paste of their paths; they are copied into the
//!   open folder. Any other paste goes where typing goes (the shell, an input line).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use liman_core::dnd;
use liman_core::i18n::{tr, trf};
use liman_core::job::Job;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::App;

impl App {
    /// Alt+F: the marked (or selected) files on the clipboard as files (`text/uri-list`), so
    /// Ctrl+V in a browser, a chat or a file manager pastes the files themselves.
    pub(super) fn copy_as_files(&mut self) {
        let paths = self.targets();
        self.copy_files_to_clipboard(paths);
    }

    fn copy_files_to_clipboard(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let list = dnd::uri_list(&paths);
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let (program, args): (&str, &[&str]) = if wayland {
            ("wl-copy", &["--type", "text/uri-list"])
        } else {
            ("xclip", &["-selection", "clipboard", "-t", "text/uri-list"])
        };
        let result = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .and_then(|mut child| {
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(list.as_bytes())?;
                }
                // wl-copy stays to serve the clipboard; it is reaped in the background.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                Ok(())
            });
        self.message = Some(match result {
            Ok(()) => match paths.as_slice() {
                [one] => trf(
                    "Copied {} as a file: Ctrl+V in the other app",
                    &[&one.file_name().unwrap_or_default().to_string_lossy()],
                ),
                many => trf("Copied {} files: Ctrl+V in the other app", &[&many.len()]),
            },
            Err(_) => trf("Cannot copy as file: {} is missing", &[&program]),
        });
    }

    /// A paste (bracketed). Files dropped on the window are copied into the open folder; other
    /// text goes where typed keys go.
    pub(super) fn on_paste(&mut self, text: String) {
        self.dirty = true;
        if self.terminal_has_focus() {
            if let Some(term) = &mut self.tab.terminal {
                term.paste(&text);
            }
            return;
        }
        let typing = self.path_input.is_some()
            || self.search_input.is_some()
            || self.rename.is_some()
            || self.dialog.is_some();
        if !typing && let Some(paths) = dnd::dropped_paths(&text) {
            return self.copy_dropped(paths);
        }
        // As if typed: the old behaviour from before bracketed paste.
        for c in text.chars().filter(|c| !c.is_control()) {
            self.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    /// Files dropped from another app: copied (never moved) into the open folder; a name that
    /// exists already gets a new one, as with Ctrl+V.
    fn copy_dropped(&mut self, sources: Vec<PathBuf>) {
        if self.tab.results.is_some() {
            self.message = Some(tr("Open a folder to drop files into").into());
            return;
        }
        let dest = self.tab.cwd.clone();
        // Dropped back onto the folder they come from: nothing to do.
        if sources.iter().all(|s| s.parent() == Some(dest.as_path())) {
            return;
        }
        let n = liman_core::format::items(sources.len());
        self.start_job(Job::Copy { sources, dest }, trf("Copying {}", &[&n]));
    }

    /// While an entry is dragged: reaching the edge of the window copies it as a file, so Ctrl+V
    /// in the app next to this one pastes it (no helper window: ADR 0017). True when that happened.
    pub(super) fn drag_left_window(&mut self, column: u16, row: u16) -> bool {
        let Ok((width, height)) = ratatui::crossterm::terminal::size() else {
            return false;
        };
        if column > 0 && row > 0 && column + 1 < width && row + 1 < height {
            return false;
        }
        let Some(drag) = self.drag.take() else {
            return false;
        };
        let Some(dragged) = self.visible_entry(drag.from).map(|e| e.path.clone()) else {
            return false;
        };
        let sources = if self.tab.marked.contains(&dragged) {
            self.targets()
        } else {
            vec![dragged]
        };
        self.copy_files_to_clipboard(sources);
        true
    }
}
