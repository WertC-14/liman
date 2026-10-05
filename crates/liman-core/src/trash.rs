//! Freedesktop trash (https://specifications.freedesktop.org/trash-spec/latest/), home trash only.
//!
//! Trashing moves the item to `$XDG_DATA_HOME/Trash/files/<name>` and writes
//! `Trash/info/<name>.trashinfo` with the original path and deletion date, so GUI file managers
//! can show and restore it too. [`TrashedItem`] remembers both paths, which makes undo exact.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Local;

use crate::ops;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashedItem {
    pub original: PathBuf,
    /// Where the item now lives, inside `Trash/files`.
    pub trashed: PathBuf,
    pub info: PathBuf,
}

/// `$XDG_DATA_HOME/Trash` (default `~/.local/share/Trash`).
pub fn home_trash(home: &Path) -> PathBuf {
    crate::xdg::data_home(home).join("Trash")
}

/// Moves `path` (absolute) into the trash at `trash_dir`.
pub fn trash(path: &Path, trash_dir: &Path) -> io::Result<TrashedItem> {
    let files = trash_dir.join("files");
    let info_dir = trash_dir.join("info");
    fs::create_dir_all(&files)?;
    fs::create_dir_all(&info_dir)?;

    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "cannot trash this path"))?
        .to_string_lossy()
        .into_owned();

    // The spec reserves a name by creating the .trashinfo file atomically (create_new).
    let (stored, info, mut info_file) = (1..)
        .map(|n| {
            if n == 1 {
                name.clone()
            } else {
                format!("{name}.{n}")
            }
        })
        .find_map(|candidate| {
            let info = info_dir.join(format!("{candidate}.trashinfo"));
            match OpenOptions::new().write(true).create_new(true).open(&info) {
                Ok(file) if !files.join(&candidate).exists() => Some(Ok((candidate, info, file))),
                Ok(_) => {
                    let _ = fs::remove_file(&info);
                    None
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .expect("unbounded search")?;

    write!(
        info_file,
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(&path.to_string_lossy()),
        Local::now().format("%Y-%m-%dT%H:%M:%S")
    )?;

    let trashed = files.join(&stored);
    if let Err(e) = ops::move_path(path, &trashed) {
        let _ = fs::remove_file(&info);
        return Err(e);
    }
    Ok(TrashedItem {
        original: path.to_path_buf(),
        trashed,
        info,
    })
}

/// Puts a trashed item back where it came from. Fails if something new is there now.
pub fn restore(item: &TrashedItem) -> io::Result<()> {
    if item.original.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists again", item.original.display()),
        ));
    }
    ops::move_path(&item.trashed, &item.original)?;
    let _ = fs::remove_file(&item.info);
    Ok(())
}

/// Percent-encodes everything except unreserved characters and `/` (RFC 3986, as the spec asks).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_dir;

    #[test]
    fn percent_encoding() {
        assert_eq!(percent_encode("/home/u/a b.txt"), "/home/u/a%20b.txt");
        assert_eq!(percent_encode("/İzle"), "/%C4%B0zle");
    }

    #[test]
    fn trash_and_restore_round_trip() {
        let dir = test_dir("trash");
        let trash_dir = dir.join("Trash");
        let file = dir.join("note.txt");
        fs::write(&file, "hi").unwrap();

        let item = trash(&file, &trash_dir).unwrap();
        assert!(!file.exists());
        assert_eq!(fs::read_to_string(&item.trashed).unwrap(), "hi");
        let info = fs::read_to_string(&item.info).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath="));
        assert!(info.contains("note.txt\nDeletionDate="));

        restore(&item).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "hi");
        assert!(!item.info.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_name_twice_gets_a_new_slot() {
        let dir = test_dir("trash-twice");
        let trash_dir = dir.join("Trash");
        let file = dir.join("a.txt");
        fs::write(&file, "1").unwrap();
        let first = trash(&file, &trash_dir).unwrap();
        fs::write(&file, "2").unwrap();
        let second = trash(&file, &trash_dir).unwrap();
        assert_ne!(first.trashed, second.trashed);
        assert!(second.trashed.ends_with("a.txt.2"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn restore_refuses_to_overwrite() {
        let dir = test_dir("trash-overwrite");
        let file = dir.join("a.txt");
        fs::write(&file, "old").unwrap();
        let item = trash(&file, &dir.join("Trash")).unwrap();
        fs::write(&file, "new").unwrap();
        assert!(restore(&item).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        fs::remove_dir_all(&dir).unwrap();
    }
}
