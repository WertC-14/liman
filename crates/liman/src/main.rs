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
use std::sync::mpsc;
use std::time::{Duration, Instant};

use liman_core::Places;

use app::App;
use tui::Tui;

const HELP: &str = "liman: a terminal file manager with the feel of a GUI file manager

Usage: liman            open the current folder
       liman --version  print the version

Inside: ? shows all keys, Ctrl+P all commands.
Config: ~/.config/liman/config (theme, lang = tr | en, colors = truecolor | 256, hidden, sort, preview)";

/// Shortest time between two frames (~60 per second).
const FRAME: Duration = Duration::from_millis(16);

fn main() -> io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("liman {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            return Ok(());
        }
        _ => {}
    }
    let mut tui = Tui::new()?;
    let (tx, rx) = mpsc::channel();
    let input = Arc::new(event::InputGate::default());
    event::spawn_input_thread(tx.clone(), input.clone());

    let cwd = std::env::current_dir()?;
    let home = std::env::var_os("HOME").map_or_else(|| cwd.clone(), PathBuf::from);
    let settings = liman_core::config::load(&liman_core::config::path(&home));
    if let Some(theme) = settings.get("theme") {
        liman_widgets::theme::set_by_name(theme);
    }
    // `lang = tr | en` in the config, otherwise the locale ($LC_ALL, $LC_MESSAGES, $LANG).
    let lang = settings
        .get("lang")
        .and_then(|l| liman_core::i18n::parse(l))
        .unwrap_or_else(|| liman_core::i18n::from_env(|v| std::env::var(v).ok()));
    liman_core::i18n::set(lang);
    // `colors = truecolor | 256` in the config overrides the guess.
    let env = |name| std::env::var(name).ok();
    let truecolor = match settings.get("colors").map(String::as_str) {
        Some("truecolor" | "24bit") => true,
        Some("256") => false,
        _ => liman_widgets::colors::detect_truecolor(
            env("COLORTERM").as_deref(),
            env("TERM").as_deref(),
            env("TERM_PROGRAM").as_deref(),
        ),
    };
    liman_widgets::colors::set_truecolor(truecolor);
    let mut app = App::new(cwd, Places::detect(&home), tx);
    app.apply_settings(&settings);
    let mut last_draw: Option<Instant> = None;
    while app.running {
        // Draw only when something changed (dirty flag), never on a fixed tick, and at most
        // once per FRAME: a shell printing thousands of chunks costs 60 frames a second.
        let mut wait = None;
        if app.dirty {
            let since = last_draw.map_or(FRAME, |t| t.elapsed());
            if since >= FRAME {
                tui.draw(|frame| ui::render(frame, &mut app))?;
                app.dirty = false;
                last_draw = Some(Instant::now());
            } else {
                wait = Some(FRAME - since);
            }
        }

        // Block until the next event (or the next frame is due), then drain everything that
        // queued up meanwhile, so a burst of events (e.g. fast scrolling) costs a single redraw.
        let first = match wait {
            None => match rx.recv() {
                Ok(ev) => ev,
                Err(_) => break,
            },
            Some(timeout) => match rx.recv_timeout(timeout) {
                Ok(ev) => ev,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
        };
        app.handle(first);
        while let Ok(next) = rx.try_recv() {
            app.handle(next);
        }

        if let Some((program, path)) = app.external.take() {
            run_external(&mut tui, &input, &program, &path, &mut app)?;
        }
    }
    Ok(())
}

/// Hands the terminal to `program` (e.g. `$EDITOR`) and takes it back afterwards.
fn run_external(
    tui: &mut Tui,
    input: &event::InputGate,
    program: &str,
    path: &std::path::Path,
    app: &mut App,
) -> io::Result<()> {
    input.pause();
    tui.suspend()?;
    let result = open::run_editor(program, path);
    tui.resume()?;
    input.resume();
    if let Err(e) = result {
        app.message = Some(liman_core::i18n::trf("Editor failed: {}", &[&e]));
    }
    app.dirty = true;
    Ok(())
}
