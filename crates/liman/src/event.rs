//! Everything the main loop reacts to arrives as an [`AppEvent`] on one channel.
//! Terminal input comes from an input thread, background work (directory listing, later file
//! operations) from worker threads; both send over the same channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use liman_core::Entry;
use liman_core::job::Outcome;
use ratatui::crossterm::event::{self, Event};

#[derive(Debug)]
pub enum AppEvent {
    Input(Event),
    /// A directory listing finished on a worker thread.
    Listing {
        generation: u64,
        path: PathBuf,
        result: Result<Vec<Entry>, String>,
    },
    /// The running job got further (bytes for copy/move, items for the rest).
    JobProgress {
        done: u64,
    },
    JobFinished(Outcome),
    /// Bytes the embedded shell wrote.
    TermOutput(Vec<u8>),
    /// A preview finished building (F3 panel).
    Preview {
        key: crate::app::PreviewKey,
        preview: Box<liman_core::preview::Preview>,
    },
    /// `git status` finished for the repository around `dir`.
    Git {
        dir: PathBuf,
        status: Option<liman_core::git::GitStatus>,
    },
    /// A git command finished: what was done and its output (or error text).
    GitDone {
        label: String,
        result: Result<String, String>,
    },
    /// Something changed in this folder (after a short quiet period).
    FolderChanged(PathBuf),
    /// The embedded shell exited.
    TermExited,
    /// The embedded shell finished its start-up output (time to clear the greeting).
    TermQuiet,
}

/// Input poll timeout right after the user did something: a pause request is noticed quickly.
const ACTIVE_POLL: Duration = Duration::from_millis(50);
/// Poll timeout when the user has been idle for [`ACTIVE_FOR`]: one wake-up a second.
const IDLE_POLL: Duration = Duration::from_secs(1);
const ACTIVE_FOR: Duration = Duration::from_secs(2);

/// Lets the main thread take stdin away from the input thread (for `$EDITOR`).
#[derive(Default)]
pub struct InputGate {
    paused: AtomicBool,
    /// Set by the input thread once it has seen `paused` and stopped reading.
    parked: AtomicBool,
}

impl InputGate {
    /// Stops the input thread and waits until it no longer reads stdin. A pause always follows
    /// a key or click, so the thread is in its fast poll and parks within ~50 ms.
    pub fn pause(&self) {
        self.parked.store(false, Ordering::Release);
        self.paused.store(true, Ordering::Release);
        let start = Instant::now();
        while !self.parked.load(Ordering::Acquire) && start.elapsed() < IDLE_POLL * 2 {
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }
}

/// Reads terminal input on its own thread so the main loop never blocks on stdin.
///
/// The poll timeout only bounds how fast a pause is noticed: short while the user is active,
/// long when idle, so an idle liman wakes about once a second.
pub fn spawn_input_thread(tx: Sender<AppEvent>, gate: Arc<InputGate>) {
    thread::spawn(move || {
        let mut last_input = Instant::now();
        loop {
            if gate.paused.load(Ordering::Acquire) {
                gate.parked.store(true, Ordering::Release);
                thread::sleep(ACTIVE_POLL);
                last_input = Instant::now();
                continue;
            }
            let timeout = if last_input.elapsed() < ACTIVE_FOR {
                ACTIVE_POLL
            } else {
                IDLE_POLL
            };
            match event::poll(timeout) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(_) => break,
            }
            let Ok(ev) = event::read() else { break };
            last_input = Instant::now();
            if tx.send(AppEvent::Input(ev)).is_err() {
                break; // main loop is gone
            }
        }
    });
}
