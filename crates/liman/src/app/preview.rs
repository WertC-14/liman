//! F3: the preview panel next to the files. The UI tells us the panel size every frame; a worker
//! builds the preview, one at a time, and the newest wish wins.

use std::path::PathBuf;

use liman_core::preview::Preview;
use ratatui::layout::Rect;

use super::App;
use crate::worker;

/// What a preview was built for: path and panel size (a resize needs a new image).
pub type PreviewKey = (PathBuf, u16, u16);

#[derive(Default)]
pub struct PreviewPane {
    pub shown: bool,
    /// The last finished preview; kept on screen until the next one arrives (no flicker).
    pub current: Option<Preview>,
    /// First line shown (mouse wheel over the panel).
    pub scroll: usize,
    /// Where the panel was drawn. Written by the UI.
    pub area: Rect,
    wanted: Option<PreviewKey>,
    busy: bool,
}

impl App {
    /// F3.
    pub(super) fn toggle_preview(&mut self) {
        self.preview.shown = !self.preview.shown;
        let value = if self.preview.shown { "true" } else { "false" };
        self.save_setting("preview", value);
        if !self.preview.shown {
            self.preview.current = None;
            self.preview.wanted = None;
            self.preview.area = Rect::default();
        }
    }

    /// Called by the UI with the room the preview content has. Starts a build when the selection or
    /// the size changed.
    pub fn want_preview(&mut self, cols: u16, rows: u16) {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            self.preview.current = None;
            self.preview.wanted = None;
            return;
        };
        let key = (path, cols, rows);
        if self.preview.wanted.as_ref() == Some(&key) {
            return;
        }
        if self.preview.wanted.as_ref().is_none_or(|w| w.0 != key.0) {
            self.preview.scroll = 0;
        }
        self.preview.wanted = Some(key.clone());
        if !self.preview.busy {
            self.preview.busy = true;
            worker::spawn_preview(self.tx.clone(), key);
        }
    }

    pub(super) fn on_preview(&mut self, key: PreviewKey, preview: Preview) {
        self.preview.busy = false;
        match &self.preview.wanted {
            Some(wanted) if *wanted == key => {
                self.preview.current = Some(preview);
                self.dirty = true;
            }
            // The selection moved on while this one was built: build the newest one now.
            Some(wanted) => {
                self.preview.busy = true;
                worker::spawn_preview(self.tx.clone(), wanted.clone());
            }
            None => {}
        }
    }

    /// The folder changed on disk: build the preview again on the next frame.
    pub(super) fn invalidate_preview(&mut self) {
        self.preview.wanted = None;
    }

    pub(super) fn scroll_preview(&mut self, delta: isize) {
        self.preview.scroll = self.preview.scroll.saturating_add_signed(delta);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Listing};
    use crate::event::AppEvent;
    use liman_core::Places;
    use liman_core::preview::Content;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn f3_shows_the_selected_file_and_follows_the_selection() {
        let dir = std::env::temp_dir().join(format!("liman-pv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), "first file").unwrap();
        fs::write(dir.join("b.txt"), "second file").unwrap();
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        while !matches!(app.listing, Listing::Ready(_)) {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        let key = |code| AppEvent::Input(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        app.handle(key(KeyCode::F(3)));
        assert!(app.preview.shown);

        let text = |app: &App| match app.preview.current.as_ref().map(|p| &p.content) {
            Some(Content::Text { lines, .. }) => lines.first().cloned(),
            _ => None,
        };
        for (expected, step) in [("first file", None), ("second file", Some(KeyCode::Down))] {
            if let Some(code) = step {
                app.handle(key(code));
            }
            app.want_preview(40, 10); // what the UI does each frame
            while text(&app).as_deref() != Some(expected) {
                app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
                app.want_preview(40, 10);
            }
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
