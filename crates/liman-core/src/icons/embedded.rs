//! liman's own icon set (MIT, fm-research ADR 0005), compiled into the binary.
//! Used when the icon theme has nothing (e.g. a headless server). Files: `assets/icons/`.

use crate::{Entry, FileType, SpecialDir};

macro_rules! icon {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/",
            $name,
            ".svg"
        ))
    };
}

pub fn embedded_svg(entry: &Entry) -> &'static [u8] {
    if entry.is_dir {
        return match entry.special {
            Some(SpecialDir::Home) => icon!("folder-home"),
            Some(SpecialDir::Desktop) => icon!("folder-desktop"),
            Some(SpecialDir::Documents) => icon!("folder-documents"),
            Some(SpecialDir::Downloads) => icon!("folder-download"),
            Some(SpecialDir::Music) => icon!("folder-music"),
            Some(SpecialDir::Pictures) => icon!("folder-pictures"),
            Some(SpecialDir::Videos) => icon!("folder-videos"),
            Some(SpecialDir::Trash) => icon!("user-trash"),
            _ => icon!("folder"),
        };
    }
    match entry.file_type {
        FileType::Folder => icon!("folder"),
        FileType::Code => icon!("file-code"),
        FileType::Config => icon!("file-config"),
        FileType::Text => icon!("file-text"),
        FileType::Pdf => icon!("file-pdf"),
        FileType::Document => icon!("file-document"),
        FileType::Spreadsheet => icon!("file-spreadsheet"),
        FileType::Presentation => icon!("file-presentation"),
        FileType::Archive => icon!("file-archive"),
        FileType::Image => icon!("file-image"),
        FileType::Video => icon!("file-video"),
        FileType::Audio => icon!("file-audio"),
        FileType::Other => icon!("file-other"),
    }
}
