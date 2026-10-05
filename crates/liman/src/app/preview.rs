//! F3: the preview panel next to the files. The UI tells us the panel size every frame; a worker
//! builds the preview, one at a time, and the newest wish wins.

use std::path::PathBuf;

use liman_core::preview::{Content, Preview};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

use super::App;
use crate::worker;

/// What a preview was built for: path and panel size (images only; 0 × 0 for the rest).
pub type PreviewKey = (PathBuf, u16, u16);

#[derive(Default)]
pub struct PreviewPane {
    pub shown: bool,
    /// Full-screen reader (Enter in the preview, `r` on a file).
    pub reader: bool,
    /// The last finished preview; kept on screen until the next one arrives (no flicker).
    pub current: Option<Preview>,
    /// First line shown (mouse wheel over the panel).
    pub scroll: usize,
    /// `m`: Markdown as source instead of formatted.
    pub raw_markdown: bool,
    /// Where the panel was drawn. Written by the UI.
    pub area: Rect,
    wanted: Option<PreviewKey>,
    busy: bool,
}

impl App {
    /// F3.
    pub(super) fn toggle_preview(&mut self) {
        self.preview.shown = !self.preview.shown;
        self.preview.reader = false;
        if !self.preview.shown && self.tab.focus == super::Focus::Preview {
            self.tab.focus = super::Focus::Files;
        }
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
        let Some(entry) = self.selected_entry() else {
            self.preview.current = None;
            self.preview.wanted = None;
            return;
        };
        // Only an image is built for the panel size; text and folders are cut by the UI, so a
        // resize (dragging the terminal panel) must not read them again.
        let (cols, rows) = if entry.file_type == liman_core::FileType::Image {
            (cols, rows)
        } else {
            (0, 0)
        };
        let key = (entry.path.clone(), cols, rows);
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
        let max = match self.preview.current.as_ref().map(|p| &p.content) {
            Some(Content::Text { lines, .. }) => lines.len().saturating_sub(1),
            Some(Content::Folder { names, .. }) => names.len().saturating_sub(1),
            Some(Content::Hex { lines, .. }) => lines.len().saturating_sub(1),
            _ => 0,
        };
        self.preview.scroll = self.preview.scroll.saturating_add_signed(delta).min(max);
    }

    /// `r` on a file: read it full screen.
    pub(super) fn open_reader(&mut self) {
        if self.selected_entry().is_some() {
            self.preview.reader = true;
            self.tab.focus = super::Focus::Preview;
        }
    }

    /// Keys while the preview (panel or reader) has the focus.
    pub(super) fn on_preview_key(&mut self, key: KeyEvent) {
        let page = self.preview.area.height.saturating_sub(4).max(1) as isize;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll_preview(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_preview(1),
            KeyCode::PageUp | KeyCode::Char('b') => self.scroll_preview(-page),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_preview(page),
            KeyCode::Home | KeyCode::Char('g') => self.preview.scroll = 0,
            KeyCode::Char('m') => self.preview.raw_markdown = !self.preview.raw_markdown,
            KeyCode::End | KeyCode::Char('G') => self.scroll_preview(isize::MAX / 2),
            KeyCode::Enter | KeyCode::Char('f') if self.preview.shown => {
                self.preview.reader = !self.preview.reader;
            }
            KeyCode::Enter | KeyCode::Char('f') if !self.preview.reader => {
                self.preview.reader = true
            }
            KeyCode::Enter | KeyCode::Char('f') | KeyCode::Esc | KeyCode::Char('q')
                if self.preview.reader =>
            {
                self.preview.reader = false;
                if !self.preview.shown {
                    self.tab.focus = super::Focus::Files;
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => self.tab.focus = super::Focus::Files,
            _ => {}
        }
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
        while !matches!(app.tab.listing, Listing::Ready(_)) {
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

#[cfg(test)]
mod reader_tests {
    use crate::app::{App, Focus, Listing};
    use crate::event::AppEvent;
    use liman_core::Places;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn r_opens_the_reader_and_esc_returns_to_the_files() {
        let dir = std::env::temp_dir().join(format!("liman-reader-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("a.txt"), text).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        while !matches!(app.tab.listing, Listing::Ready(_)) {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        let key = |code| AppEvent::Input(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        app.handle(key(KeyCode::Char('r')));
        assert!(app.preview.reader);
        assert_eq!(app.tab.focus, Focus::Preview);
        app.want_preview(80, 20);
        while app.preview.current.is_none() {
            app.handle(rx.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        app.handle(key(KeyCode::End));
        assert_eq!(app.preview.scroll, 99);
        app.handle(key(KeyCode::Char('j')));
        assert_eq!(app.preview.scroll, 99, "stops at the last line");
        app.handle(key(KeyCode::Esc));
        assert!(!app.preview.reader);
        assert_eq!(app.tab.focus, Focus::Files);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
