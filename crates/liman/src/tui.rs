//! Terminal setup and teardown.

use std::io::{self, stdout};

use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::{DefaultTerminal, Frame};

/// Owns the terminal while the app runs. Restores it on drop, on error and on panic.
pub struct Tui {
    terminal: DefaultTerminal,
}

impl Tui {
    /// Enters raw mode and the alternate screen, and enables mouse reporting.
    pub fn new() -> io::Result<Self> {
        // Installs a panic hook that leaves raw mode and the alternate screen.
        let terminal = ratatui::try_init()?;

        // Our hook runs first, then ratatui's: mouse reporting must be switched off too,
        // otherwise the shell receives mouse escape codes after a crash.
        let ratatui_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = execute!(stdout(), DisableMouseCapture);
            ratatui_hook(info);
        }));

        execute!(stdout(), EnableMouseCapture)?;
        Ok(Self { terminal })
    }

    /// Draws one frame inside a synchronized update (DEC mode 2026), so the terminal shows
    /// the whole frame at once instead of a half-drawn one. Terminals without support ignore it.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> io::Result<()> {
        execute!(stdout(), BeginSynchronizedUpdate)?;
        let result = self.terminal.draw(render).map(|_| ());
        execute!(stdout(), EndSynchronizedUpdate)?;
        result
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();
    }
}
