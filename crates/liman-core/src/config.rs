//! A tiny settings file: `$XDG_CONFIG_HOME/liman/config` (default `~/.config/liman/config`),
//! one `key = value` per line, `#` comments. Kept in our own place, never in the browsed folders
//! (ADR 0004: global preferences, no `.directory` files).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn path(home: &Path) -> PathBuf {
    crate::xdg::config_home(home).join("liman/config")
}

/// All settings; a missing or unreadable file is just empty.
pub fn load(file: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// Sets one key, keeping the others.
pub fn set(file: &Path, key: &str, value: &str) -> io::Result<()> {
    let mut all = load(file);
    all.insert(key.to_string(), value.to_string());
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let text: String = all.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    fs::write(file, format!("# liman settings\n{text}"))
}

/// Bookmarked folders, one path per line, in `$XDG_CONFIG_HOME/liman/bookmarks`.
pub fn bookmarks_path(home: &Path) -> PathBuf {
    path(home).with_file_name("bookmarks")
}

pub fn load_bookmarks(file: &Path) -> Vec<PathBuf> {
    fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

pub fn save_bookmarks(file: &Path, bookmarks: &[PathBuf]) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let text: String = bookmarks
        .iter()
        .map(|p| format!("{}\n", p.display()))
        .collect();
    fs::write(file, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_dir;

    #[test]
    fn set_and_load_keep_other_keys() {
        let dir = test_dir("config");
        let file = dir.join("liman/config");
        assert!(load(&file).is_empty());
        set(&file, "theme", "nord").unwrap();
        set(&file, "view", "grid").unwrap();
        set(&file, "theme", "gruvbox").unwrap();
        let all = load(&file);
        assert_eq!(all.get("theme").map(String::as_str), Some("gruvbox"));
        assert_eq!(all.get("view").map(String::as_str), Some("grid"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
