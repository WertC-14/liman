//! Nerd Font icons (`icons = nerd` in the config). Off by default: a terminal without a Nerd
//! Font shows empty boxes, and liman must look right over SSH (CLAUDE.md, ADR 0004). When off,
//! the plain Unicode symbols of [`crate::symbols`] and the extension badges are used.
//!
//! Glyphs are Nerd Fonts v3 code points (Font Awesome `f…`, Devicons / Seti `e…`), written as
//! escapes so the source stays readable without the font.

use std::sync::atomic::{AtomicBool, Ordering};

use liman_core::{Entry, FileType, SpecialDir};

static NERD: AtomicBool = AtomicBool::new(false);

/// Whether Nerd Font icons are drawn (set once at start-up from the config).
pub fn nerd() -> bool {
    NERD.load(Ordering::Relaxed)
}

pub fn set_nerd(on: bool) {
    NERD.store(on, Ordering::Relaxed);
}

/// A well-known folder.
pub fn special_dir(kind: SpecialDir) -> &'static str {
    match kind {
        SpecialDir::Home => "\u{f015}",
        SpecialDir::Desktop => "\u{f108}",
        SpecialDir::Documents => "\u{f0f6}",
        SpecialDir::Downloads => "\u{f019}",
        SpecialDir::Music => "\u{f001}",
        SpecialDir::Pictures => "\u{f03e}",
        SpecialDir::Videos => "\u{f03d}",
        SpecialDir::Templates => "\u{f0c5}",
        SpecialDir::Public => "\u{f0ac}",
        SpecialDir::Trash => "\u{f1f8}",
        SpecialDir::Root => "\u{f0a0}",
        SpecialDir::Bookmark => "\u{f02e}",
        SpecialDir::Recent => "\u{f017}",
    }
}

/// A folder (open: its branch is expanded).
pub fn folder(open: bool) -> &'static str {
    if open { "\u{f07c}" } else { "\u{f07b}" }
}

/// The icon of an entry: well-known folders and file names first, then the extension, then
/// the file type.
pub fn entry(entry: &Entry) -> &'static str {
    if let Some(kind) = entry.special {
        return special_dir(kind);
    }
    if entry.is_dir {
        return folder(false);
    }
    match entry.name.as_str() {
        "Cargo.toml" | "Cargo.lock" => return "\u{e7a8}",
        "Makefile" | "makefile" | "justfile" => return "\u{f489}",
        "Dockerfile" => return "\u{f308}",
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "\u{e702}",
        _ => {}
    }
    let ext = entry
        .extension()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "rs" => "\u{e7a8}",
        "py" => "\u{e73c}",
        "js" | "mjs" | "jsx" => "\u{e74e}",
        "ts" | "tsx" => "\u{e628}",
        "go" => "\u{e627}",
        "c" | "h" => "\u{e61e}",
        "cpp" | "cc" | "hpp" => "\u{e61d}",
        "java" | "kt" => "\u{e738}",
        "rb" => "\u{e739}",
        "php" => "\u{e73d}",
        "lua" => "\u{e620}",
        "html" => "\u{e736}",
        "css" | "scss" => "\u{e749}",
        "sh" | "bash" | "zsh" | "fish" => "\u{f489}",
        "md" | "markdown" => "\u{e73e}",
        "json" => "\u{e60b}",
        "toml" | "yaml" | "yml" | "ini" | "conf" | "cfg" | "env" => "\u{e615}",
        "lock" => "\u{f023}",
        _ => match entry.file_type {
            FileType::Text => "\u{f0f6}",
            FileType::Pdf => "\u{f1c1}",
            FileType::Document => "\u{f1c2}",
            FileType::Spreadsheet => "\u{f1c3}",
            FileType::Presentation => "\u{f1c4}",
            FileType::Archive => "\u{f1c6}",
            FileType::Image => "\u{f1c5}",
            FileType::Video => "\u{f1c8}",
            FileType::Audio => "\u{f1c7}",
            FileType::Code | FileType::Config => "\u{f121}",
            FileType::Folder | FileType::Other => "\u{f15b}",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(name: &str) -> Entry {
        let path = PathBuf::from(name);
        Entry {
            name: name.into(),
            file_type: FileType::from_path(&path, false),
            path,
            is_dir: false,
            is_symlink: false,
            size: 0,
            item_count: None,
            contents: None,
            modified: None,
            special: None,
        }
    }

    #[test]
    fn names_then_extensions_then_types() {
        assert_eq!(entry(&file("Cargo.toml")), "\u{e7a8}"); // name before extension
        assert_eq!(entry(&file("main.RS")), "\u{e7a8}");
        assert_eq!(entry(&file("report.pdf")), "\u{f1c1}"); // by type
        assert_eq!(entry(&file("noext")), "\u{f15b}");
    }
}
