//! Application state. Rendering reads it, events change it.

use std::path::PathBuf;

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

use crate::event::AppEvent;

pub struct App {
    pub running: bool,
    /// Set when the screen must be redrawn.
    pub dirty: bool,
    pub cwd: PathBuf,
    /// Short description of the last input, shown in the status bar (temporary, for the skeleton).
    pub last_input: String,
}

impl App {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            running: true,
            dirty: true,
            cwd,
            last_input: String::new(),
        }
    }

    pub fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => self.on_key(key),
            AppEvent::Input(Event::Mouse(mouse)) => self.on_mouse(mouse),
            AppEvent::Input(Event::Resize(..)) => self.dirty = true,
            AppEvent::Input(_) => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Char('c') if ctrl => self.running = false,
            code => {
                self.last_input = format!("key {code}");
                self.dirty = true;
            }
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        // Mouse movement is reported constantly; only clicks and scrolling matter.
        if matches!(mouse.kind, MouseEventKind::Moved) {
            return;
        }
        self.last_input = format!("mouse {:?} at {},{}", mouse.kind, mouse.column, mouse.row);
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventState, MouseButton};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> AppEvent {
        AppEvent::Input(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }))
    }

    fn app() -> App {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.dirty = false;
        app
    }

    #[test]
    fn q_esc_and_ctrl_c_quit() {
        for ev in [
            key(KeyCode::Char('q'), KeyModifiers::NONE),
            key(KeyCode::Esc, KeyModifiers::NONE),
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut app = app();
            app.handle(ev);
            assert!(!app.running);
        }
    }

    #[test]
    fn other_keys_redraw_but_keep_running() {
        let mut app = app();
        app.handle(key(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(app.running);
        assert!(app.dirty);
    }

    #[test]
    fn resize_redraws() {
        let mut app = app();
        app.handle(AppEvent::Input(Event::Resize(80, 24)));
        assert!(app.dirty);
    }

    #[test]
    fn mouse_move_is_ignored_but_click_redraws() {
        let mouse = |kind| {
            AppEvent::Input(Event::Mouse(MouseEvent {
                kind,
                column: 3,
                row: 4,
                modifiers: KeyModifiers::NONE,
            }))
        };
        let mut app = app();
        app.handle(mouse(MouseEventKind::Moved));
        assert!(!app.dirty);
        app.handle(mouse(MouseEventKind::Down(MouseButton::Left)));
        assert!(app.dirty);
    }
}
