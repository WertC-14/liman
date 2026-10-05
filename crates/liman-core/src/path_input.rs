//! A path typed by the user (Ctrl+L): `~` and relative paths are resolved, Tab completes the
//! last part from the folder's entries, like a shell.

use std::fs;
use std::path::{Path, PathBuf};

/// The path `text` means: `~` is the home folder, a relative path starts in `cwd`.
pub fn resolve(text: &str, cwd: &Path, home: &Path) -> PathBuf {
    let text = text.trim();
    if text == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return home.join(rest);
    }
    let path = Path::new(text);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Completes the last part of `text` (Tab). Several matches: their common start; a single
/// folder: its name and a `/`. `None` when nothing matches. Hidden entries only when the typed
/// part starts with a dot.
pub fn complete(text: &str, cwd: &Path, home: &Path) -> Option<String> {
    let (dir_text, part) = match text.rfind('/') {
        Some(i) => (&text[..=i], &text[i + 1..]),
        None => ("", text),
    };
    let dir = if dir_text.is_empty() {
        cwd.to_path_buf()
    } else {
        resolve(dir_text, cwd, home)
    };
    let mut matches: Vec<(String, bool)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let hidden_ok = !name.starts_with('.') || part.starts_with('.');
            (name.starts_with(part) && hidden_ok).then(|| (name, e.path().is_dir()))
        })
        .collect();
    matches.sort();
    let first = matches.first()?;
    let common = matches
        .iter()
        .skip(1)
        .fold(first.0.clone(), |acc, (name, _)| {
            acc.chars()
                .zip(name.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect()
        });
    let slash = if matches.len() == 1 && first.1 {
        "/"
    } else {
        ""
    };
    Some(format!("{dir_text}{common}{slash}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_dir;

    #[test]
    fn resolves_home_relative_and_absolute() {
        let (cwd, home) = (Path::new("/data/a"), Path::new("/home/u"));
        assert_eq!(resolve("~", cwd, home), PathBuf::from("/home/u"));
        assert_eq!(resolve("~/x", cwd, home), PathBuf::from("/home/u/x"));
        assert_eq!(resolve("b/c", cwd, home), PathBuf::from("/data/a/b/c"));
        assert_eq!(resolve(" /etc ", cwd, home), PathBuf::from("/etc"));
    }

    #[test]
    fn tab_completes_like_a_shell() {
        let dir = test_dir("complete");
        fs::create_dir(dir.join("Projects")).unwrap();
        fs::create_dir(dir.join("Pictures")).unwrap();
        fs::create_dir(dir.join(".config")).unwrap();
        fs::write(dir.join("Pinned.txt"), "").unwrap();
        let home = Path::new("/nowhere");
        let base = format!("{}/", dir.display());
        // One folder matches: its name and a slash.
        assert_eq!(
            complete(&format!("{base}Pro"), &dir, home),
            Some(format!("{base}Projects/"))
        );
        // Several: the common start.
        assert_eq!(
            complete(&format!("{base}P"), &dir, home),
            Some(format!("{base}P"))
        );
        assert_eq!(
            complete(&format!("{base}Pi"), &dir, home),
            Some(format!("{base}Pi"))
        );
        // A file gets no slash; relative input works from cwd.
        assert_eq!(complete("Pin", &dir, home), Some("Pinned.txt".into()));
        // Hidden only when asked for.
        assert_eq!(complete(".co", &dir, home), Some(".config/".into()));
        assert_eq!(complete("x", &dir, home), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
