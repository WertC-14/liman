//! File operations: rename, copy, move. Blocking: run them on a worker thread.
//! Every function returns the final path, which the undo stack needs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Checks a user-typed file name.
pub fn validate_name(name: &str) -> io::Result<()> {
    let bad =
        name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0');
    if bad {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::trf("“{}” is not a valid name", &[&name]),
        ));
    }
    Ok(())
}

/// Renames `path` inside its folder. Never overwrites.
pub fn rename(path: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_name(new_name)?;
    let target = path.with_file_name(new_name);
    if target == path {
        return Ok(target);
    }
    if target.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            crate::i18n::trf("“{}” already exists", &[&new_name]),
        ));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

/// A name in `dir` based on `name` that does not exist yet: `a.txt`, `a (2).txt`, `a (3).txt`, ...
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if first.symlink_metadata().is_err() {
        return first;
    }
    // Keep the extension at the end; dotfiles like `.bashrc` have no extension.
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| p.symlink_metadata().is_err())
        .expect("unbounded search")
}

/// Total bytes of regular files under `path` (for progress). Symlinks are not followed.
pub fn total_size(path: &Path) -> u64 {
    let Ok(meta) = path.symlink_metadata() else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .map(|it| {
                it.filter_map(Result::ok)
                    .map(|e| total_size(&e.path()))
                    .sum()
            })
            .unwrap_or(0)
    } else if meta.is_file() {
        meta.len()
    } else {
        0 // symlinks, sockets, devices: nothing is copied byte by byte
    }
}

/// Copies `src` into `dest_dir`, picking a free name on conflict. `progress` gets bytes copied so far.
pub fn copy_into(
    src: &Path,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64),
) -> io::Result<PathBuf> {
    let name = file_name(src)?;
    if dest_dir.starts_with(src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::tr("cannot copy a folder into itself"),
        ));
    }
    let target = free_name(dest_dir, &name);
    let mut done = 0;
    copy_recursive(src, &target, &mut done, progress)?;
    Ok(target)
}

/// Moves `src` into `dest_dir`, picking a free name on conflict.
pub fn move_into(
    src: &Path,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64),
) -> io::Result<PathBuf> {
    let name = file_name(src)?;
    if dest_dir.starts_with(src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            crate::i18n::tr("cannot move a folder into itself"),
        ));
    }
    if src.parent() == Some(dest_dir) {
        return Ok(src.to_path_buf()); // already there
    }
    let target = free_name(dest_dir, &name);
    match fs::rename(src, &target) {
        Ok(()) => Ok(target),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            let mut done = 0;
            copy_recursive(src, &target, &mut done, progress)?;
            remove_all(src)?;
            Ok(target)
        }
        Err(e) => Err(e),
    }
}

/// Moves `from` to exactly `to` (which must not exist), across file systems if needed.
pub fn move_path(from: &Path, to: &Path) -> io::Result<()> {
    if to.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            crate::i18n::trf("{} already exists", &[&to.display()]),
        ));
    }
    match fs::rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            let mut done = 0;
            copy_recursive(from, to, &mut done, &mut |_| {})?;
            remove_all(from)
        }
        other => other,
    }
}

pub fn remove_all(path: &Path) -> io::Result<()> {
    if path.symlink_metadata()?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn file_name(path: &Path) -> io::Result<String> {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))
}

fn copy_recursive(
    src: &Path,
    dst: &Path,
    done: &mut u64,
    progress: &mut dyn FnMut(u64),
) -> io::Result<()> {
    let meta = src.symlink_metadata()?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)?;
    } else if kind.is_dir() {
        fs::create_dir(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()), done, progress)?;
        }
        fs::set_permissions(dst, meta.permissions())?;
    } else {
        *done += fs::copy(src, dst)?;
        progress(*done);
    }
    Ok(())
}

/// Fresh, empty directory under the system temp dir for tests.
#[cfg(test)]
pub(crate) fn test_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("liman-ops-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_names_keep_the_extension() {
        let dir = test_dir("free");
        assert_eq!(free_name(&dir, "a.txt"), dir.join("a.txt"));
        fs::write(dir.join("a.txt"), "").unwrap();
        assert_eq!(free_name(&dir, "a.txt"), dir.join("a (2).txt"));
        fs::write(dir.join("a (2).txt"), "").unwrap();
        assert_eq!(free_name(&dir, "a.txt"), dir.join("a (3).txt"));
        fs::write(dir.join(".bashrc"), "").unwrap();
        assert_eq!(free_name(&dir, ".bashrc"), dir.join(".bashrc (2)"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_validates_and_never_overwrites() {
        let dir = test_dir("rename");
        let a = dir.join("a.txt");
        fs::write(&a, "a").unwrap();
        fs::write(dir.join("b.txt"), "b").unwrap();
        assert!(rename(&a, "").is_err());
        assert!(rename(&a, "x/y").is_err());
        assert!(rename(&a, "b.txt").is_err());
        let c = rename(&a, "c.txt").unwrap();
        assert_eq!(fs::read_to_string(c).unwrap(), "a");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_folder_recursively_with_progress() {
        let dir = test_dir("copy");
        let src = dir.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("one.txt"), "12345").unwrap();
        fs::write(src.join("sub/two.txt"), "123").unwrap();
        std::os::unix::fs::symlink("one.txt", src.join("link")).unwrap();
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();

        assert_eq!(total_size(&src), 8);
        let mut last = 0;
        let copied = copy_into(&src, &dest, &mut |b| last = b).unwrap();
        assert_eq!(copied, dest.join("src"));
        assert_eq!(last, 8);
        assert_eq!(
            fs::read_to_string(copied.join("sub/two.txt")).unwrap(),
            "123"
        );
        assert_eq!(
            fs::read_link(copied.join("link")).unwrap(),
            PathBuf::from("one.txt")
        );

        // Second copy gets a free name; the original is untouched.
        assert_eq!(
            copy_into(&src, &dest, &mut |_| {}).unwrap(),
            dest.join("src (2)")
        );
        assert!(src.join("one.txt").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copying_or_moving_into_itself_fails() {
        let dir = test_dir("self");
        let folder = dir.join("f");
        fs::create_dir_all(folder.join("inner")).unwrap();
        assert!(copy_into(&folder, &folder.join("inner"), &mut |_| {}).is_err());
        assert!(move_into(&folder, &folder, &mut |_| {}).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn move_into_another_folder() {
        let dir = test_dir("move");
        let file = dir.join("a.txt");
        fs::write(&file, "x").unwrap();
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let moved = move_into(&file, &dest, &mut |_| {}).unwrap();
        assert_eq!(moved, dest.join("a.txt"));
        assert!(!file.exists());
        // Moving to the folder it is already in is a no-op.
        assert_eq!(move_into(&moved, &dest, &mut |_| {}).unwrap(), moved);
        fs::remove_dir_all(&dir).unwrap();
    }
}
