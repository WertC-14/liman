//! Watching the open folder (inotify through `notify`). Changes from anywhere — the embedded shell,
//! other programs, our own jobs — refresh the list. Bursts are merged: one refresh after the folder
//! has been quiet for `SETTLE`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::event::AppEvent;

/// A copy of many files sends hundreds of events; wait until they stop.
const SETTLE: Duration = Duration::from_millis(150);

pub struct FolderWatch {
    watcher: Option<RecommendedWatcher>,
    current: Option<PathBuf>,
}

impl FolderWatch {
    pub fn start(tx: Sender<AppEvent>) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel::<PathBuf>();
        // Debounce thread: collect events, report each folder once it has been quiet.
        thread::spawn(move || {
            let mut pending: Option<PathBuf> = None;
            loop {
                match raw_rx.recv_timeout(SETTLE) {
                    Ok(dir) => pending = Some(dir),
                    Err(RecvTimeoutError::Timeout) => {
                        if let Some(dir) = pending.take()
                            && tx.send(AppEvent::FolderChanged(dir)).is_err()
                        {
                            return;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && !matches!(event.kind, notify::EventKind::Access(_))
                && let Some(dir) = event.paths.first().and_then(|p| p.parent())
            {
                let _ = raw_tx.send(dir.to_path_buf());
            }
        })
        .ok();
        Self {
            watcher,
            current: None,
        }
    }

    /// Watches `dir` (not its subfolders) instead of the previous folder.
    pub fn watch(&mut self, dir: &Path) {
        if self.current.as_deref() == Some(dir) {
            return;
        }
        let Some(watcher) = &mut self.watcher else {
            return;
        };
        if let Some(old) = self.current.take() {
            let _ = watcher.unwatch(&old);
        }
        if watcher.watch(dir, RecursiveMode::NonRecursive).is_ok() {
            self.current = Some(dir.to_path_buf());
        }
    }
}
