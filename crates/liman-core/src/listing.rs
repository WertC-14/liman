//! Reading a directory into sorted [`Entry`] values. Blocking: call it from a worker thread.

use std::fs;
use std::io;
use std::path::Path;

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
        entries.push(entry_for(item.path(), name, opts));
    }
    sort_entries(&mut entries);
    Ok(entries)
}

/// One entry with its metadata. `name` is what the user sees (normally the file name).
pub fn entry_for(path: std::path::PathBuf, name: String, _opts: ListOptions) -> Entry {
    let link = fs::symlink_metadata(&path).ok();
    let is_symlink = link.as_ref().is_some_and(|m| m.file_type().is_symlink());
    // Follow symlinks for size and type; a broken link falls back to the link itself.
    let meta = fs::metadata(&path).ok().or(link);
    let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());

    Entry {
        file_type: FileType::from_path(&path, is_dir),
        size: if is_dir {
            0
        } else {
            meta.as_ref().map_or(0, |m| m.len())
        },
        // Counted later by `count_children` on a worker, so a folder with many subfolders (or a
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

/// Number of (visible) children of `dir`: one `read_dir`, not recursive. `None` if unreadable.
pub fn count_children(dir: &Path, opts: ListOptions) -> Option<usize> {
    folder_summary(dir, opts).map(|(count, _)| count)
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
            count_children(&entries[0].path, ListOptions::default()),
            Some(1)
        );
        assert_eq!(entries[0].file_type, FileType::Folder);
        assert_eq!(entries[3].size, 5);

        let all = list_dir(&dir, ListOptions { show_hidden: true }).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(
            count_children(&all[0].path, ListOptions { show_hidden: true }),
            Some(2)
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(list_dir(Path::new("/definitely/not/here"), ListOptions::default()).is_err());
    }
}
