//! liman: a terminal file manager with the feel of a GUI file manager.

mod app;
mod event;
mod tui;
mod ui;
mod worker;

use std::io;
use std::sync::mpsc;

use app::App;
use tui::Tui;

fn main() -> io::Result<()> {
    let mut tui = Tui::new()?;
    let (tx, rx) = mpsc::channel();
    event::spawn_input_thread(tx.clone());

    let mut app = App::new(std::env::current_dir()?, tx);
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
