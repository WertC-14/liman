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
                let done = false;
                if tx
                    .send(AppEvent::Counts {
                        generation,
                        counts,
                        done,
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
        let _ = tx.send(AppEvent::Counts {
            generation,
            counts: batch,
            done: true,
        });
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

/// Searches names under `root` (Ctrl+F) and sends what it finds while it searches, in batches
/// (live results). `current` holds the newest search id; an older search sees it change and
/// stops early.
pub fn spawn_search(
    tx: Sender<AppEvent>,
    generation: u64,
    current: std::sync::Arc<std::sync::atomic::AtomicU64>,
    root: PathBuf,
    needle: String,
    show_hidden: bool,
) {
    use std::sync::atomic::Ordering;
    const BATCH: Duration = Duration::from_millis(80);
    thread::spawn(move || {
        let cancelled = || current.load(Ordering::Relaxed) != generation;
        let mut batch = Vec::new();
        let mut last = Instant::now();
        let send = |paths: &mut Vec<PathBuf>, done: bool| {
            let entries = liman_core::results::entries_for(&std::mem::take(paths), &root);
            let _ = tx.send(AppEvent::SearchFound {
                generation,
                entries,
                done,
            });
        };
        liman_core::results::search_names_with(&root, &needle, show_hidden, &cancelled, &mut |p| {
            batch.push(p);
            if last.elapsed() >= BATCH {
                last = Instant::now();
                send(&mut batch, false);
            }
        });
        if !cancelled() {
            send(&mut batch, true);
        }
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
    spawn_git_task(tx, label, move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        liman_core::git::run(&dir, &refs)
    });
}

/// Runs `task` (one or more git commands) and reports it like a single command.
pub fn spawn_git_task(
    tx: Sender<AppEvent>,
    label: String,
    task: impl FnOnce() -> Result<String, String> + Send + 'static,
) {
    thread::spawn(move || {
        let result = task();
        let _ = tx.send(AppEvent::GitDone { label, result });
    });
}

/// The git panel's diff of one file.
pub fn spawn_git_diff(
    tx: Sender<AppEvent>,
    root: PathBuf,
    path: PathBuf,
    mark: liman_core::git::GitMark,
) {
    thread::spawn(move || {
        let lines = liman_core::git::file_diff(&root, &path, mark);
        let _ = tx.send(AppEvent::GitDiff { path, lines });
    });
}

/// Where `p` in the git panel pushes (remote name and URL).
pub fn spawn_git_remote(tx: Sender<AppEvent>, root: PathBuf, upstream: Option<String>) {
    thread::spawn(move || {
        let remote = liman_core::git::push_remote(&root, upstream.as_deref());
        let _ = tx.send(AppEvent::GitRemote { root, remote });
    });
}

/// Local branches for the branch picker.
pub fn spawn_git_branches(tx: Sender<AppEvent>, root: PathBuf) {
    thread::spawn(move || {
        let result = liman_core::git::run(&root, &["branch", "--format=%(refname:short)"])
            .map(|out| out.lines().map(str::to_string).collect());
        let _ = tx.send(AppEvent::GitBranches(result));
    });
}

/// Reads the subfolders of `dir` for the folder tree.
pub fn spawn_tree_children(tx: Sender<AppEvent>, dir: PathBuf, show_hidden: bool) {
    thread::spawn(move || {
        let children = liman_core::tree::subfolders(&dir, show_hidden);
        let _ = tx.send(AppEvent::TreeChildren { dir, children });
    });
}

/// Builds the preview of one path (F3 panel). With a graphics-capable `picker`, an image is also
/// encoded for the terminal (Kitty / Sixel / iTerm2) at the panel size, here and not on the UI
/// thread: the encoding is the expensive part (ADR 0009).
pub fn spawn_preview(
    tx: Sender<AppEvent>,
    key: crate::app::PreviewKey,
    picker: Option<ratatui_image::picker::Picker>,
) {
    thread::spawn(move || {
        let is_image =
            liman_core::FileType::from_path(&key.0, false) == liman_core::FileType::Image;
        // With a graphics protocol the image is decoded once, for the protocol only.
        let (preview, graphic) = match picker.filter(|_| is_image) {
            Some(picker) => {
                let (preview, image) = liman_core::preview::build_with_image(&key.0);
                let size = ratatui::layout::Size::new(key.1, key.2);
                let fit = ratatui_image::Resize::Fit(None);
                match image.and_then(|img| picker.new_protocol(img, size, fit).ok()) {
                    Some(protocol) => (preview, Some(Box::new(crate::app::Graphic(protocol)))),
                    // Could not encode: half blocks after all.
                    None => (liman_core::preview::build(&key.0, key.1, key.2), None),
                }
            }
            None => (liman_core::preview::build(&key.0, key.1, key.2), None),
        };
        let _ = tx.send(AppEvent::Preview {
            key,
            preview: Box::new(preview),
            graphic,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_image::picker::{Picker, ProtocolType};
    use std::sync::mpsc;

    #[test]
    fn with_a_graphics_protocol_an_image_is_encoded_once_on_the_worker() {
        let dir = std::env::temp_dir().join(format!("liman-graphic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("dot.png");
        // A 1×1 red PNG (checked: valid chunks and CRCs).
        const RED_DOT: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
            0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&png, RED_DOT).unwrap();
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let (tx, rx) = mpsc::channel();
        spawn_preview(tx, (png, 20, 10), Some(picker));
        let AppEvent::Preview {
            preview, graphic, ..
        } = rx.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected a preview")
        };
        assert!(graphic.is_some());
        let liman_core::preview::Content::Image {
            original, pixels, ..
        } = preview.content
        else {
            panic!("expected an image")
        };
        assert_eq!(original, (1, 1));
        assert!(pixels.is_empty()); // no half-block copy was made
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
