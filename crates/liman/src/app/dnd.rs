//! Drag and drop with other apps (fm-research ADR 0014).
//!
//! - Out: Ctrl+C (or dragging an entry to the edge of the window) also puts the files on the
//!   system clipboard as files; Ctrl+V in a browser, a chat or a file manager pastes them.
//! - In from the clipboard: Ctrl+V pastes files another app copied (ADR 0017).
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
    /// Puts `paths` on the system clipboard as files (`text/uri-list`), so Ctrl+V in a browser,
    /// a chat or a file manager pastes them. Quiet: Ctrl+C says what happened.
    pub(super) fn copy_to_system(&mut self, paths: &[PathBuf]) {
        // Tests never touch the real clipboard of the person running them.
        if cfg!(test) {
            return;
        }
        let list = dnd::uri_list(paths);
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let (program, args): (&str, &[&str]) = if wayland {
            ("wl-copy", &["--type", "text/uri-list"])
        } else {
            ("xclip", &["-selection", "clipboard", "-t", "text/uri-list"])
        };
        let sent = Command::new(program)
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
        self.system_clip = sent.is_ok().then_some(list);
    }

    /// Files another app put on the system clipboard (Ctrl+C in a file manager), when they are
    /// not the ones liman put there itself.
    pub(super) fn files_from_system(&self) -> Option<Vec<PathBuf>> {
        if cfg!(test) {
            return None;
        }
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let output = if wayland {
            Command::new("wl-paste")
                .args(["--no-newline", "--type", "text/uri-list"])
                .output()
        } else {
            Command::new("xclip")
                .args(["-selection", "clipboard", "-o", "-t", "text/uri-list"])
                .output()
        }
        .ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        if !output.status.success()
            || self.system_clip.as_deref().map(str::trim) == Some(text.trim())
        {
            return None;
        }
        dnd::dropped_paths(&text)
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
    pub(super) fn copy_dropped(&mut self, sources: Vec<PathBuf>) {
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
        self.clipboard = Some(super::Clipboard {
            mode: super::ClipMode::Copy,
            paths: sources.clone(),
        });
        self.copy_to_system(&sources);
        self.message = Some(trf(
            "{} copied: Ctrl+V in a folder here or in another app",
            &[&liman_core::format::items(sources.len())],
        ));
        true
    }
}
