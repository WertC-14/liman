//! Files named in a shell command's output (`find`, `fd`, `grep -l`, `rg`), shown in the file view
//! as a "results" listing. Lines that are not existing paths are ignored.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::listing::entry_for;
use crate::{Entry, ListOptions};

/// At most this many results are shown (a `find /` can print millions of lines).
pub const MAX_RESULTS: usize = 5000;

/// Removes terminal escape sequences (colors, cursor moves, titles) and carriage returns.
pub fn strip_ansi(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // CSI: ESC [ params final-byte(@..~)
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] ... (BEL | ESC \)
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {} // two-byte sequences like ESC =
            },
            '\r' => {}
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Existing paths named in `text`, in order, without duplicates. Relative paths are taken from `base`
/// (the shell's folder). A line counts if it is a path, or starts with `path:` (grep / ripgrep output).
pub fn paths_from_output(text: &str, base: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let candidate = resolve(line, base).or_else(|| {
            let (prefix, _) = line.split_once(':')?;
            resolve(prefix, base).filter(|p| p.is_file())
        });
        if let Some(path) = candidate
            && seen.insert(path.clone())
        {
            out.push(path);
            if out.len() == MAX_RESULTS {
                break;
            }
        }
    }
    out
}

fn resolve(text: &str, base: &Path) -> Option<PathBuf> {
    let text = text.strip_prefix("./").unwrap_or(text);
    if text.is_empty() || text == "." {
        return None;
    }
    let path = if Path::new(text).is_absolute() {
        PathBuf::from(text)
    } else {
        base.join(text)
    };
    path.symlink_metadata().is_ok().then_some(path)
}

/// Entries for `paths`, named relative to `base` where possible (`src/main.rs`), in the given order.
pub fn entries_for(paths: &[PathBuf], base: &Path) -> Vec<Entry> {
    paths
        .iter()
        .map(|path| {
            let name = path
                .strip_prefix(base)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.display().to_string());
            entry_for(path.clone(), name, ListOptions { show_hidden: true })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_dir;
    use std::fs;

    #[test]
    fn strips_colors_titles_and_carriage_returns() {
        let raw = b"\x1b]0;fish\x07\x1b[1;34m./src\x1b[0m\r\n./a.txt\r\n\x1b[?2004h";
        assert_eq!(strip_ansi(raw), "./src\n./a.txt\n");
    }

    #[test]
    fn keeps_existing_paths_from_find_and_grep_output() {
        let dir = test_dir("results");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("a.txt"), "x").unwrap();
        let output = format!(
            "❯ find . -name '*.rs'\n./src/main.rs\nfind: ‘./secret’: Permission denied\n\
             a.txt\n{}/a.txt\nsrc/main.rs:1:fn main() {{}}\nnot a path\n❯ ",
            dir.display()
        );
        let paths = paths_from_output(&output, &dir);
        assert_eq!(paths, [dir.join("src/main.rs"), dir.join("a.txt")]);

        let entries = entries_for(&paths, &dir);
        assert_eq!(entries[0].name, "src/main.rs");
        assert_eq!(entries[1].size, 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
