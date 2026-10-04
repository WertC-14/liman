//! liman: a terminal file manager with the feel of a GUI file manager.

mod app;
mod event;
mod tui;
mod ui;
mod worker;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;

use liman_core::Places;

use app::App;
use tui::Tui;

fn main() -> io::Result<()> {
    let mut tui = Tui::new()?;
    let (tx, rx) = mpsc::channel();
    event::spawn_input_thread(tx.clone());

    let cwd = std::env::current_dir()?;
    let home = std::env::var_os("HOME").map_or_else(|| cwd.clone(), PathBuf::from);
    let mut app = App::new(cwd, Places::detect(&home), tx);
    while app.running {
        // Draw only when something changed (dirty flag), never on a fixed tick.
        if app.dirty {
            tui.draw(|frame| ui::render(frame, &mut app))?;
            app.dirty = false;
        }

        // Block until the next event, then drain everything that queued up meanwhile,
        // so a burst of events (e.g. fast scrolling) costs a single redraw.
        let Ok(first) = rx.recv() else { break };
        app.handle(first);
        while let Ok(next) = rx.try_recv() {
            app.handle(next);
        }
    }
    Ok(())
}
