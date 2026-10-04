//! Everything the main loop reacts to arrives as an [`AppEvent`] on one channel.
//! Terminal input comes from an input thread, background work (directory listing, later file
//! operations) from worker threads; both send over the same channel.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use liman_core::Entry;
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
}

/// Reads terminal input on its own thread so the main loop never blocks on stdin.
pub fn spawn_input_thread(tx: Sender<AppEvent>) {
    thread::spawn(move || {
        while let Ok(ev) = event::read() {
            if tx.send(AppEvent::Input(ev)).is_err() {
                break; // main loop is gone
            }
        }
    });
}
