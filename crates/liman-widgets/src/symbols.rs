//! Plain Unicode symbols (no Nerd Font needed, ADR 0004) for well-known folders.
//! Chosen from arrows and math operators: geometric shapes like ▭ ▣ are drawn two cells wide
//! by some fonts and eat the space after them.

use liman_core::SpecialDir;

pub fn special_dir(kind: SpecialDir) -> &'static str {
    if crate::icons::nerd() {
        return crate::icons::special_dir(kind);
    }
    match kind {
        SpecialDir::Home => "⌂",
        SpecialDir::Desktop => "⊞",
        SpecialDir::Documents => "≡",
        SpecialDir::Downloads => "↓",
        SpecialDir::Music => "♪",
        SpecialDir::Pictures => "⊡",
        SpecialDir::Videos => "▶",
        SpecialDir::Templates => "⋄",
        SpecialDir::Public => "⊚",
        SpecialDir::Trash => "✕",
        SpecialDir::Root => "/",
        SpecialDir::Bookmark => "★",
        SpecialDir::Recent => "↺",
    }
}
