//! Reading a directory into sorted [`Entry`] values. Blocking: call it from a worker thread.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::sort::sort_entries;
use crate::{Entry, FileType};

#[derive(Debug, Clone, Copy, Default)]
pub struct ListOptions {
    pub show_hidden: bool,
}

pub fn list_dir(dir: &Path, opts: ListOptions) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if !opts.show_hidden && name.starts_with('.') {
            continue;
        }
        // The link bit comes with the directory entry (no extra `stat`).
        let is_symlink = item.file_type().is_ok_and(|t| t.is_symlink());
        entries.push(entry_with(item.path(), name, is_symlink));
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// One entry with its metadata, for a path that did not come from `read_dir`.
/// `name` is what the user sees (normally the file name).
pub fn entry_for(path: PathBuf, name: String) -> Entry {
    let is_symlink = fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
    entry_with(path, name, is_symlink)
}

/// One `stat` (following links) per entry; a broken link falls back to the link itself.
fn entry_with(path: PathBuf, name: String, is_symlink: bool) -> Entry {
    let meta = fs::metadata(&path).ok().or_else(|| {
        is_symlink
            .then(|| fs::symlink_metadata(&path).ok())
            .flatten()
    });
    let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());

    Entry {
        file_type: FileType::from_path(&path, is_dir),
        size: if is_dir {
            0
        } else {
            meta.as_ref().map_or(0, |m| m.len())
        },
        // Counted later by `folder_summary` on a worker, so a folder with many subfolders (or a
        // slow mount) shows its list at once.
        item_count: None,
        contents: None,
        modified: meta.and_then(|m| m.modified().ok()),
        name,
        path,
        is_dir,
        is_symlink,
        special: None,
    }
}

/// Number of (visible) children and the type most of its files have, if one type is at least
/// half of them (by extension, no extra reads). One `read_dir`, not recursive.
pub fn folder_summary(dir: &Path, opts: ListOptions) -> Option<(usize, Option<FileType>)> {
    let mut count = 0;
    let mut files = 0;
    let mut by_type: Vec<(FileType, usize)> = Vec::new();
    for item in fs::read_dir(dir).ok()?.flatten() {
        let name = item.file_name();
        let name = name.to_string_lossy();
        if !opts.show_hidden && name.starts_with('.') {
            continue;
        }
        count += 1;
        if item.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        files += 1;
        let t = FileType::from_path(Path::new(name.as_ref()), false);
        if t == FileType::Other {
            continue;
        }
        match by_type.iter_mut().find(|(k, _)| *k == t) {
            Some((_, n)) => *n += 1,
            None => by_type.push((t, 1)),
        }
    }
    let dominant = by_type
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n * 2 >= files)
        .map(|(t, _)| t);
    Some((count, dominant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_summary_finds_the_main_file_type() {
        let dir = tempdir("summary");
        for name in ["a.md", "b.md", "c.png", "notes.txt"] {
            fs::write(dir.join(name), "").unwrap();
        }
        fs::create_dir(dir.join("sub")).unwrap();
        // 3 of 4 files are text (md, txt), the folder counts too: 5 items.
        assert_eq!(
            folder_summary(&dir, ListOptions::default()),
            Some((5, Some(FileType::Text)))
        );
        fs::write(dir.join("d.rs"), "").unwrap();
        fs::write(dir.join("e.rs"), "").unwrap();
        fs::write(dir.join("f.rs"), "").unwrap();
        // 3 text, 3 code, 1 image: no type has half of the files.
        assert_eq!(
            folder_summary(&dir, ListOptions::default()),
            Some((8, None))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("liman-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_folders_first_and_hides_dotfiles() {
        let dir = tempdir("list");
        fs::create_dir(dir.join("Music")).unwrap();
        fs::write(dir.join("Music/song.mp3"), b"x").unwrap();
        fs::write(dir.join("Music/.hidden"), b"x").unwrap();
        fs::write(dir.join("b.txt"), b"hello").unwrap();
        fs::write(dir.join("a10.pdf"), b"").unwrap();
        fs::write(dir.join("a2.pdf"), b"").unwrap();
        fs::write(dir.join(".secret"), b"").unwrap();

        let entries = list_dir(&dir, ListOptions::default()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Music", "a2.pdf", "a10.pdf", "b.txt"]);
        assert_eq!(entries[0].item_count, None); // counted separately
        assert_eq!(
            folder_summary(&entries[0].path, ListOptions::default()).map(|(n, _)| n),
            Some(1)
        );
        assert_eq!(entries[0].file_type, FileType::Folder);
        assert_eq!(entries[3].size, 5);

        let all = list_dir(&dir, ListOptions { show_hidden: true }).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(
            folder_summary(&all[0].path, ListOptions { show_hidden: true }).map(|(n, _)| n),
            Some(2)
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn links_are_followed_and_broken_ones_still_listed() {
        let dir = tempdir("links");
        fs::write(dir.join("target.txt"), b"12345").unwrap();
        std::os::unix::fs::symlink("target.txt", dir.join("good")).unwrap();
        std::os::unix::fs::symlink("nowhere", dir.join("broken")).unwrap();
        let entries = list_dir(&dir, ListOptions::default()).unwrap();
        let get = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
        assert!(get("good").is_symlink);
        assert_eq!(get("good").size, 5); // the target's size
        assert!(get("broken").is_symlink);
        assert!(!get("target.txt").is_symlink);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(list_dir(Path::new("/definitely/not/here"), ListOptions::default()).is_err());
    }
}
