//! Ratatui widgets for liman: badge, box and grid views.
//!
//! Widgets only read [`liman_core`] data; they never touch the file system.

pub mod badge;
pub mod boxes;
pub mod breadcrumb;
pub mod colors;
pub mod file_list;
pub mod gitmark;
pub mod grid;
pub mod highlight;
pub mod markdown;
pub mod preview;
pub mod rows;
pub mod sidebar;
pub mod symbols;
pub mod theme;

pub use file_list::{FileList, ListMode};
pub use grid::GridView;
pub use rows::Rows;
pub use sidebar::Sidebar;
