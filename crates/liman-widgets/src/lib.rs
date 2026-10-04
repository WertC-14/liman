//! Ratatui widgets for liman: badge, box and grid views.
//!
//! Widgets only read [`liman_core`] data; they never touch the file system.

pub mod badge;
pub mod breadcrumb;
pub mod detailed;
pub mod sidebar;
pub mod symbols;
pub mod theme;

pub use detailed::DetailedView;
pub use sidebar::Sidebar;
