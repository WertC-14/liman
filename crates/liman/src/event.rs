//! Everything the main loop reacts to arrives as an [`AppEvent`] on one channel.
//! Terminal input is the only source for now; background workers (directory listing,
//! file operations) will send their results over the same channel.

use std::sync::mpsc::Sender;
use std::thread;

use ratatui::crossterm::event::{self, Event};

#[derive(Debug)]
pub enum AppEvent {
    Input(Event),
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
