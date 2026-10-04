//! Ratatui widgets for liman: badge, box and grid views.
//!
//! Widgets only read [`liman_core`] data; they never touch the file system.

pub mod badge;
pub mod boxes;
pub mod breadcrumb;
pub mod file_list;
pub mod sidebar;
pub mod symbols;
pub mod theme;

pub use file_list::{FileList, ListMode};
pub use sidebar::Sidebar;
