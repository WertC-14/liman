//! Plain Unicode symbols (no Nerd Font needed, ADR 0004) for well-known folders.

use liman_core::SpecialDir;

pub fn special_dir(kind: SpecialDir) -> &'static str {
    match kind {
        SpecialDir::Home => "⌂",
        SpecialDir::Desktop => "▭",
        SpecialDir::Documents => "≡",
        SpecialDir::Downloads => "↓",
        SpecialDir::Music => "♪",
        SpecialDir::Pictures => "▣",
        SpecialDir::Videos => "▶",
        SpecialDir::Templates => "◇",
        SpecialDir::Public => "◎",
        SpecialDir::Trash => "✕",
        SpecialDir::Root => "/",
    }
}
