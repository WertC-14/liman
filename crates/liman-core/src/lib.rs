//! UI-independent core of liman: directory listing, file types, formatting and (later) file operations and undo.
//!
//! This crate must not depend on ratatui or any terminal library.

pub mod config;
pub mod entry;
pub mod file_type;
pub mod format;
pub mod git;
pub mod i18n;
pub mod job;
pub mod listing;
pub mod ops;
pub mod places;
pub mod preview;
pub mod recent;
pub mod results;
pub mod sort;
pub mod trash;

pub use entry::Entry;
pub use file_type::FileType;
pub use listing::{ListOptions, list_dir};
pub use places::{Place, Places, SpecialDir};
