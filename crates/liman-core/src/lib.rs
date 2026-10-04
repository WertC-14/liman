//! UI-independent core of liman: directory listing, file types, formatting and (later) file operations and undo.
//!
//! This crate must not depend on ratatui or any terminal library.

pub mod entry;
pub mod file_type;
pub mod format;
pub mod listing;
pub mod sort;

pub use entry::Entry;
pub use file_type::FileType;
pub use listing::{ListOptions, list_dir};
