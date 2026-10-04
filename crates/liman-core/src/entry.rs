//! One row in a directory listing.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::FileType;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Number of visible children for directories; `None` for files or unreadable directories.
    pub item_count: Option<usize>,
    pub modified: Option<SystemTime>,
    pub file_type: FileType,
}

impl Entry {
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }

    /// Extension as written in the name, without the dot (`None` for `Makefile` or `.bashrc`).
    pub fn extension(&self) -> Option<&str> {
        if self.is_dir {
            return None;
        }
        let stem_start = usize::from(self.is_hidden());
        self.name[stem_start..]
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .filter(|ext| !ext.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir: false,
            is_symlink: false,
            size: 0,
            item_count: None,
            modified: None,
            file_type: FileType::Other,
        }
    }

    #[test]
    fn extension() {
        assert_eq!(file("main.rs").extension(), Some("rs"));
        assert_eq!(file("a.tar.gz").extension(), Some("gz"));
        assert_eq!(file("Makefile").extension(), None);
        assert_eq!(file(".bashrc").extension(), None);
        assert_eq!(file(".config.toml").extension(), Some("toml"));
        assert_eq!(file("trailing.").extension(), None);
    }
}
