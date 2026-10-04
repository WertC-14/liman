//! Everything the main loop reacts to arrives as an [`AppEvent`] on one channel.
//! Terminal input comes from an input thread, background work (directory listing, later file
//! operations) from worker threads; both send over the same channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

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
    /// The embedded shell exited.
    TermExited,
    /// The embedded shell finished its start-up output (time to clear the greeting).
    TermQuiet,
}

/// How long the input thread waits for input before checking the pause flag again.
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Reads terminal input on its own thread so the main loop never blocks on stdin.
///
/// While `paused` is set the thread does not touch stdin, so an external program
/// (e.g. `$EDITOR`) gets every key. Polling with a timeout makes the flag take effect
/// within [`POLL_INTERVAL`].
pub fn spawn_input_thread(tx: Sender<AppEvent>, paused: Arc<AtomicBool>) {
    thread::spawn(move || {
        loop {
            if paused.load(Ordering::Acquire) {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            match event::poll(POLL_INTERVAL) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(_) => break,
            }
            let Ok(ev) = event::read() else { break };
            if tx.send(AppEvent::Input(ev)).is_err() {
                break; // main loop is gone
            }
        }
    });
}
