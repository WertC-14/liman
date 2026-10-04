//! liman: a terminal file manager with the feel of a GUI file manager.

mod app;
mod event;
mod open;
mod terminal;
mod tui;
mod ui;
mod watch;
mod worker;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use liman_core::Places;

use app::App;
use tui::Tui;

fn main() -> io::Result<()> {
    let mut tui = Tui::new()?;
    let (tx, rx) = mpsc::channel();
    let input_paused = Arc::new(AtomicBool::new(false));
    event::spawn_input_thread(tx.clone(), input_paused.clone());

    let cwd = std::env::current_dir()?;
    let home = std::env::var_os("HOME").map_or_else(|| cwd.clone(), PathBuf::from);
    let settings = liman_core::config::load(&liman_core::config::path(&home));
    if let Some(theme) = settings.get("theme") {
        liman_widgets::theme::set_by_name(theme);
    }
    let mut app = App::new(cwd, Places::detect(&home), tx);
    app.apply_settings(&settings);
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

        if let Some((program, path)) = app.external.take() {
            run_external(&mut tui, &input_paused, &program, &path, &mut app)?;
        }
    }
    Ok(())
}

/// Hands the terminal to `program` (e.g. `$EDITOR`) and takes it back afterwards.
fn run_external(
    tui: &mut Tui,
    input_paused: &AtomicBool,
    program: &str,
    path: &std::path::Path,
    app: &mut App,
) -> io::Result<()> {
    input_paused.store(true, Ordering::Release);
    // Let the input thread finish its current poll so it does not steal the editor's first keys.
    std::thread::sleep(event::POLL_INTERVAL * 2);
    tui.suspend()?;
    let result = open::run_editor(program, path);
    tui.resume()?;
    input_paused.store(false, Ordering::Release);
    if let Err(e) = result {
        app.message = Some(format!("Editor failed: {e}"));
    }
    app.dirty = true;
    Ok(())
}
