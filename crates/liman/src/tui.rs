//! Terminal setup and teardown.

use std::io::{self, stdout};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen,
    enable_raw_mode,
};
use ratatui::{DefaultTerminal, Frame, Terminal};

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

impl Tui {
    /// Gives the terminal back (normal screen, cooked mode) so another program can use it.
    pub fn suspend(&mut self) -> io::Result<()> {
        execute!(stdout(), DisableMouseCapture)?;
        ratatui::try_restore()
    }

    /// Takes the terminal again after [`Tui::suspend`]. The next frame is drawn in full,
    /// because the other program left arbitrary content on the screen.
    ///
    /// A fresh `Terminal` has empty frame buffers, so ratatui's diff sends every cell.
    /// (`Terminal::clear` would do the same but asks the terminal for the cursor position,
    /// which can time out on slow SSH links.)
    pub fn resume(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(
            stdout(),
            EnterAlternateScreen,
            Clear(ClearType::All),
            EnableMouseCapture
        )?;
        self.terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = execute!(stdout(), DisableMouseCapture);
        ratatui::restore();
    }
}
