//! Application state. Rendering reads it, events change it.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use liman_core::{Entry, ListOptions};
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;

use crate::event::AppEvent;
use crate::worker;

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
    /// Selected row and scroll offset of the detailed view; kept between frames.
    pub table: TableState,
    /// Incremented on every listing request; older results are ignored.
    generation: u64,
    options: ListOptions,
    tx: Sender<AppEvent>,
}

impl App {
    /// Creates the app and starts listing `cwd` in the background.
    pub fn new(cwd: PathBuf, tx: Sender<AppEvent>) -> Self {
        let mut app = Self {
            running: true,
            dirty: true,
            cwd: cwd.clone(),
            listing: Listing::Loading,
            table: TableState::default(),
            generation: 0,
            options: ListOptions::default(),
            tx,
        };
        app.load(cwd);
        app
    }

    /// Requests a listing of `path`. The view switches when the result arrives.
    pub fn load(&mut self, path: PathBuf) {
        self.generation += 1;
        self.listing = Listing::Loading;
        self.dirty = true;
        worker::spawn_listing(self.tx.clone(), self.generation, path, self.options);
    }

    pub fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => self.on_key(key),
            AppEvent::Input(Event::Resize(..)) => self.dirty = true,
            AppEvent::Input(_) => {}
            AppEvent::Listing {
                generation,
                path,
                result,
            } => self.on_listing(generation, path, result),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Char('c') if ctrl => self.running = false,
            _ => {}
        }
    }

    fn on_listing(&mut self, generation: u64, path: PathBuf, result: Result<Vec<Entry>, String>) {
        if generation != self.generation {
            return; // stale: a newer request is on its way
        }
        self.cwd = path;
        self.listing = match result {
            Ok(entries) => {
                self.table =
                    TableState::default().with_selected((!entries.is_empty()).then_some(0));
                Listing::Ready(entries)
            }
            Err(err) => Listing::Failed(err),
        };
        self.dirty = true;
    }

    pub fn entry_count(&self) -> Option<usize> {
        match &self.listing {
            Listing::Ready(entries) => Some(entries.len()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventState;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> AppEvent {
        AppEvent::Input(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }))
    }

    fn app() -> (App, Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(std::env::temp_dir(), tx);
        app.dirty = false;
        (app, rx)
    }

    #[test]
    fn q_esc_and_ctrl_c_quit() {
        for ev in [
            key(KeyCode::Char('q'), KeyModifiers::NONE),
            key(KeyCode::Esc, KeyModifiers::NONE),
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let (mut app, _rx) = app();
            app.handle(ev);
            assert!(!app.running);
        }
    }

    #[test]
    fn resize_redraws() {
        let (mut app, _rx) = app();
        app.handle(AppEvent::Input(Event::Resize(80, 24)));
        assert!(app.dirty);
    }

    #[test]
    fn worker_result_fills_the_listing() {
        let (mut app, rx) = app();
        let ev = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("listing result");
        app.handle(ev);
        assert!(app.dirty);
        assert!(!matches!(app.listing, Listing::Loading));
    }

    #[test]
    fn stale_results_are_ignored() {
        let (mut app, _rx) = app();
        app.load(PathBuf::from("/")); // generation 2
        app.handle(AppEvent::Listing {
            generation: 1,
            path: PathBuf::from("/old"),
            result: Ok(Vec::new()),
        });
        assert!(matches!(app.listing, Listing::Loading));
        assert_ne!(app.cwd, PathBuf::from("/old"));
    }

    #[test]
    fn first_row_is_selected_after_loading() {
        let (mut app, _rx) = app();
        let generation = app.generation;
        let entry =
            liman_core::list_dir(&std::env::temp_dir(), ListOptions::default()).unwrap_or_default();
        let non_empty = !entry.is_empty();
        app.handle(AppEvent::Listing {
            generation,
            path: std::env::temp_dir(),
            result: Ok(entry),
        });
        assert_eq!(app.table.selected().is_some(), non_empty);
    }
}
