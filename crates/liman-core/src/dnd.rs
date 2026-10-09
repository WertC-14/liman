//! Drag and drop with other apps (fm-research ADR 0014). A terminal cannot start a system drag
//! itself; the app hands files to `ripdrag`, or puts them on the clipboard as files. Files dropped
//! on the terminal window arrive as pasted text (kitty, foot, ... paste their paths); this module
//! tells such a paste from ordinary text.

use std::path::{Path, PathBuf};

/// The files a paste names, when it is nothing but existing paths: one per line, or separated
/// by spaces with shell quoting (`'a b.txt'`, `a\ b.txt`), or as `file://` URIs. `None` for any
/// other text, so typing and pasting words still works.
pub fn dropped_paths(text: &str) -> Option<Vec<PathBuf>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // One path per line first (a name may contain spaces), then shell words.
    let lines: Vec<PathBuf> = text.lines().filter_map(path_of).collect();
    if !lines.is_empty() && lines.len() == text.lines().filter(|l| !l.trim().is_empty()).count() {
        return Some(lines);
    }
    let words = shell_words(text)?;
    let paths: Vec<PathBuf> = words.iter().filter_map(|w| path_of(w)).collect();
    (!paths.is_empty() && paths.len() == words.len()).then_some(paths)
}

/// An existing absolute path, or a `file://` URI of one.
fn path_of(item: &str) -> Option<PathBuf> {
    let item = item.trim();
    let path = match item.strip_prefix("file://") {
        // `file://host/path` keeps only the path; the host is this machine.
        Some(rest) => PathBuf::from(percent_decode(&rest[rest.find('/')?..])?),
        None => PathBuf::from(item),
    };
    (path.is_absolute() && path.exists()).then_some(path)
}

/// Splits like a shell: spaces separate, quotes and backslashes keep them. `None` when a quote
/// is not closed.
fn shell_words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => word.push(chars.next()?),
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next()?);
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `text/uri-list` for the clipboard ("copy as file"): one `file://` URI per line, CRLF ended.
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("{}\r\n", file_uri(p)))
        .collect()
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tells_dropped_files_from_text() {
        let dir = std::env::temp_dir().join(format!("liman-dnd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("ödev 1.pdf");
        let b = dir.join("b.txt");
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        let (sa, sb) = (a.display().to_string(), b.display().to_string());

        // kitty: shell-quoted, space separated.
        let quoted = format!("'{sa}' {sb}");
        assert_eq!(dropped_paths(&quoted), Some(vec![a.clone(), b.clone()]));
        // One per line, and as URIs (percent-encoded).
        assert_eq!(
            dropped_paths(&format!("{sa}\n{sb}\n")),
            Some(vec![a.clone(), b.clone()])
        );
        let uris = uri_list(&[a.clone(), b.clone()]);
        assert!(uris.contains("%C3%B6dev%201.pdf"));
        assert_eq!(dropped_paths(&uris), Some(vec![a.clone(), b.clone()]));
        // Ordinary text, a missing file, or a relative name are not drops.
        assert_eq!(dropped_paths("merhaba dünya"), None);
        assert_eq!(dropped_paths(&format!("{sb} /yok/böyle")), None);
        assert_eq!(dropped_paths("b.txt"), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
