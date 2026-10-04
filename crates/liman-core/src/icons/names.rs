//! Which icon names (freedesktop icon naming spec) to look for, most specific first.

use crate::{Entry, FileType, SpecialDir};

pub fn icon_names(entry: &Entry) -> Vec<&'static str> {
    let mut names = Vec::new();
    if entry.is_dir {
        if let Some(kind) = entry.special {
            names.extend_from_slice(special(kind));
        }
        names.push("folder");
        return names;
    }
    if let Some(ext) = entry.extension() {
        names.extend_from_slice(by_extension(&ext.to_ascii_lowercase()));
    }
    names.extend_from_slice(by_type(entry.file_type));
    names.push("text-x-generic");
    names
}

fn special(kind: SpecialDir) -> &'static [&'static str] {
    match kind {
        SpecialDir::Home => &["user-home", "folder-home"],
        SpecialDir::Desktop => &["user-desktop", "folder-desktop"],
        SpecialDir::Documents => &["folder-documents"],
        SpecialDir::Downloads => &["folder-download", "folder-downloads"],
        SpecialDir::Music => &["folder-music"],
        SpecialDir::Pictures => &["folder-pictures", "folder-images"],
        SpecialDir::Videos => &["folder-videos", "folder-video"],
        SpecialDir::Templates => &["folder-templates"],
        SpecialDir::Public => &["folder-publicshare", "folder-public"],
        SpecialDir::Trash => &["user-trash"],
        SpecialDir::Root => &["drive-harddisk"],
    }
}

/// MIME-type icon names for common extensions (a small table instead of full shared-mime-info).
fn by_extension(ext: &str) -> &'static [&'static str] {
    match ext {
        "pdf" => &["application-pdf"],
        "rs" => &["text-rust", "text-x-rust"],
        "py" => &["text-x-python"],
        "js" | "mjs" => &["application-javascript", "text-javascript"],
        "ts" | "tsx" => &["application-x-typescript", "text-x-typescript"],
        "c" => &["text-x-csrc"],
        "h" => &["text-x-chdr"],
        "cpp" | "cc" | "hpp" => &["text-x-c++src"],
        "go" => &["text-x-go"],
        "java" => &["text-x-java"],
        "sh" | "bash" | "zsh" | "fish" => &["application-x-shellscript", "text-x-script"],
        "html" => &["text-html"],
        "css" => &["text-css"],
        "json" => &["application-json"],
        "toml" => &["application-toml", "text-x-toml"],
        "yaml" | "yml" => &["application-x-yaml", "text-x-yaml"],
        "xml" => &["application-xml", "text-xml"],
        "md" | "markdown" => &["text-markdown", "text-x-markdown"],
        "txt" => &["text-plain"],
        "doc" => &["application-msword"],
        "docx" => &["application-vnd.openxmlformats-officedocument.wordprocessingml.document"],
        "odt" => &["application-vnd.oasis.opendocument.text"],
        "xls" => &["application-vnd.ms-excel"],
        "xlsx" => &["application-vnd.openxmlformats-officedocument.spreadsheetml.sheet"],
        "ods" => &["application-vnd.oasis.opendocument.spreadsheet"],
        "csv" => &["text-csv"],
        "ppt" => &["application-vnd.ms-powerpoint"],
        "pptx" => &["application-vnd.openxmlformats-officedocument.presentationml.presentation"],
        "odp" => &["application-vnd.oasis.opendocument.presentation"],
        "zip" => &["application-zip"],
        "tar" => &["application-x-tar"],
        "gz" | "tgz" | "xz" | "zst" | "bz2" => &["application-x-compressed-tar"],
        "7z" => &["application-x-7z-compressed"],
        "png" => &["image-png"],
        "jpg" | "jpeg" => &["image-jpeg"],
        "gif" => &["image-gif"],
        "svg" => &["image-svg+xml"],
        "mp4" => &["video-mp4"],
        "mkv" => &["video-x-matroska"],
        "webm" => &["video-webm"],
        "mp3" => &["audio-mpeg", "audio-x-mpeg"],
        "flac" => &["audio-flac", "audio-x-flac"],
        "ogg" | "opus" => &["audio-ogg", "audio-x-vorbis+ogg"],
        _ => &[],
    }
}

/// Generic icons per category, used when the extension has no specific icon.
fn by_type(file_type: FileType) -> &'static [&'static str] {
    match file_type {
        FileType::Folder => &["folder"],
        FileType::Code => &["text-x-script"],
        FileType::Config | FileType::Text | FileType::Other => &[],
        FileType::Pdf => &["application-pdf"],
        FileType::Document => &["x-office-document"],
        FileType::Spreadsheet => &["x-office-spreadsheet"],
        FileType::Presentation => &["x-office-presentation"],
        FileType::Archive => &["package-x-generic", "application-x-archive"],
        FileType::Image => &["image-x-generic"],
        FileType::Video => &["video-x-generic"],
        FileType::Audio => &["audio-x-generic"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, special: Option<SpecialDir>) -> Entry {
        let path = PathBuf::from(name);
        Entry {
            name: name.into(),
            file_type: FileType::from_path(&path, is_dir),
            path,
            is_dir,
            is_symlink: false,
            special,
            size: 0,
            item_count: None,
            modified: None,
        }
    }

    #[test]
    fn specific_names_come_first() {
        assert_eq!(
            icon_names(&entry("Downloads", true, Some(SpecialDir::Downloads))),
            ["folder-download", "folder-downloads", "folder"]
        );
        assert_eq!(icon_names(&entry("x", true, None)), ["folder"]);
        assert_eq!(
            icon_names(&entry("main.rs", false, None)),
            [
                "text-rust",
                "text-x-rust",
                "text-x-script",
                "text-x-generic"
            ]
        );
        assert_eq!(
            icon_names(&entry("Makefile", false, None)),
            ["text-x-generic"]
        );
    }
}
