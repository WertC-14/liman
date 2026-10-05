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

/// Counts the children of `dirs` (the folders of listing `generation`). Stops as soon as the
/// app moved on (`current` changed); sends what it has every COUNT_BATCH so the first counts
/// appear quickly in a folder with thousands of subfolders.
pub fn spawn_counts(
    tx: Sender<AppEvent>,
    generation: u64,
    current: std::sync::Arc<std::sync::atomic::AtomicU64>,
    dirs: Vec<PathBuf>,
    opts: ListOptions,
) {
    const COUNT_BATCH: Duration = Duration::from_millis(50);
    thread::spawn(move || {
        let mut batch = Vec::new();
        let mut last = Instant::now();
        for dir in dirs {
            if current.load(std::sync::atomic::Ordering::Relaxed) != generation {
                return;
            }
            let summary = liman_core::listing::folder_summary(&dir, opts);
            batch.push((dir, summary.map(|(n, _)| n), summary.and_then(|(_, t)| t)));
            if last.elapsed() >= COUNT_BATCH {
                last = Instant::now();
                let counts = std::mem::take(&mut batch);
                if tx.send(AppEvent::Counts { generation, counts }).is_err() {
                    return;
                }
            }
        }
        if !batch.is_empty() {
            let _ = tx.send(AppEvent::Counts {
                generation,
                counts: batch,
            });
        }
    });
}

/// Progress events are sent at most this often, so a copy of many small files does not flood the UI.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Runs a file operation on a new thread and reports progress and the result. The total (a walk
/// of every source tree for a copy) is worked out here too, never on the UI thread.
pub fn spawn_job(tx: Sender<AppEvent>, job: Job, trash_dir: PathBuf) {
    thread::spawn(move || {
        let total = job.total();
        let _ = tx.send(AppEvent::JobProgress { done: 0, total });
        let mut last = Instant::now();
        let progress_tx = tx.clone();
        let outcome = job.run(&trash_dir, &mut |done| {
            if last.elapsed() >= PROGRESS_INTERVAL {
                last = Instant::now();
                let _ = progress_tx.send(AppEvent::JobProgress { done, total });
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

/// `git status` for the repository around `dir` (None outside a repository or without git).
pub fn spawn_git_status(tx: Sender<AppEvent>, dir: PathBuf) {
    thread::spawn(move || {
        let status = liman_core::git::status(&dir);
        let _ = tx.send(AppEvent::Git { dir, status });
    });
}

/// Runs one git command (stage, commit, push, ...) and reports its output.
pub fn spawn_git_command(tx: Sender<AppEvent>, dir: PathBuf, args: Vec<String>, label: String) {
    thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = liman_core::git::run(&dir, &refs);
        let _ = tx.send(AppEvent::GitDone { label, result });
    });
}

/// Builds the preview of one path (F3 panel).
pub fn spawn_preview(tx: Sender<AppEvent>, key: crate::app::PreviewKey) {
    thread::spawn(move || {
        let preview = liman_core::preview::build(&key.0, key.1, key.2);
        let _ = tx.send(AppEvent::Preview {
            key,
            preview: Box::new(preview),
        });
    });
}
