//! Background work. Results come back as [`AppEvent`]s on the main channel, so the UI never waits on the disk.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use std::time::{Duration, Instant};

use std::sync::{Arc, OnceLock};

use liman_core::icons::{IconTheme, detect_theme_name, load_icon};
use liman_core::job::Job;
use liman_core::{Entry, ListOptions, list_dir};

use crate::event::AppEvent;

/// Lists `path` on a new thread. `generation` lets the app drop results of requests it no longer cares
/// about (e.g. the user already moved to another folder before this one finished loading).
pub fn spawn_listing(tx: Sender<AppEvent>, generation: u64, path: PathBuf, opts: ListOptions) {
    thread::spawn(move || {
        let result = list_dir(&path, opts).map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Listing {
            generation,
            path,
            result,
        });
    });
}

/// Progress events are sent at most this often, so a copy of many small files does not flood the UI.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Runs a file operation on a new thread and reports progress and the result.
pub fn spawn_job(tx: Sender<AppEvent>, job: Job, trash_dir: PathBuf) {
    thread::spawn(move || {
        let mut last = Instant::now();
        let progress_tx = tx.clone();
        let outcome = job.run(&trash_dir, &mut |done| {
            if last.elapsed() >= PROGRESS_INTERVAL {
                last = Instant::now();
                let _ = progress_tx.send(AppEvent::JobProgress { done });
            }
        });
        let _ = tx.send(AppEvent::JobFinished(outcome));
    });
}

/// Renders icons for the grid. The icon theme is looked up once (it spawns `gsettings` and reads
/// many directories), by whichever icon worker runs first.
pub fn spawn_icons(
    tx: Sender<AppEvent>,
    theme: Arc<OnceLock<IconTheme>>,
    home: PathBuf,
    wanted: Vec<(String, Entry)>,
) {
    thread::spawn(move || {
        let theme =
            theme.get_or_init(|| IconTheme::load(detect_theme_name(&home).as_deref(), &home));
        let icons = wanted
            .into_iter()
            .map(|(key, entry)| (key, load_icon(&entry, theme)))
            .collect();
        let _ = tx.send(AppEvent::Icons(icons));
    });
}
