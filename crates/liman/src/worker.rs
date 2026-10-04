//! Background work. Results come back as [`AppEvent`]s on the main channel, so the UI never waits on the disk.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use std::time::{Duration, Instant};

use liman_core::job::Job;
use liman_core::{ListOptions, list_dir};

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

/// Builds entries for paths a shell command printed (metadata reads can be slow for thousands).
pub fn spawn_results(tx: Sender<AppEvent>, generation: u64, base: PathBuf, paths: Vec<PathBuf>) {
    thread::spawn(move || {
        let entries = liman_core::results::entries_for(&paths, &base);
        let _ = tx.send(AppEvent::Listing {
            generation,
            path: base,
            result: Ok(entries),
        });
    });
}

/// Searches names under `root` (Ctrl+F). `current` holds the newest search id; an older search
/// sees it change and stops early.
pub fn spawn_search(
    tx: Sender<AppEvent>,
    generation: u64,
    current: std::sync::Arc<std::sync::atomic::AtomicU64>,
    root: PathBuf,
    needle: String,
    show_hidden: bool,
) {
    use std::sync::atomic::Ordering;
    thread::spawn(move || {
        let cancelled = || current.load(Ordering::Relaxed) != generation;
        let paths = liman_core::results::search_names(&root, &needle, show_hidden, &cancelled);
        if cancelled() {
            return;
        }
        let entries = liman_core::results::entries_for(&paths, &root);
        let _ = tx.send(AppEvent::Listing {
            generation,
            path: root,
            result: Ok(entries),
        });
    });
}
