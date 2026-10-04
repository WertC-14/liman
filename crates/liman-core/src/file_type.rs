//! File type categories. The UI picks a color per category; the badge label comes from the extension.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileType {
    Folder,
    Code,
    Config,
    Text,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Archive,
    Image,
    Video,
    Audio,
    Other,
}

impl FileType {
    /// Guesses the category from the extension only (no content sniffing, it is cheap and never blocks).
    pub fn from_path(path: &Path, is_dir: bool) -> Self {
        if is_dir {
            return Self::Folder;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return Self::Other;
        };
        Self::from_extension(&ext.to_ascii_lowercase())
    }

    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" | "py" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "c" | "h" | "cpp" | "hpp"
            | "cc" | "go" | "java" | "kt" | "swift" | "rb" | "php" | "lua" | "sh" | "bash"
            | "zsh" | "fish" | "html" | "css" | "scss" | "sql" | "zig" | "hs" | "ex" | "exs"
            | "cs" => Self::Code,
            "json" | "toml" | "yaml" | "yml" | "ini" | "xml" | "conf" | "cfg" | "lock" | "env" => {
                Self::Config
            }
            "md" | "markdown" | "txt" | "rst" | "org" | "log" | "tex" => Self::Text,
            "pdf" => Self::Pdf,
            "doc" | "docx" | "odt" | "rtf" | "epub" => Self::Document,
            "xls" | "xlsx" | "ods" | "csv" | "tsv" => Self::Spreadsheet,
            "ppt" | "pptx" | "odp" | "key" => Self::Presentation,
            "zip" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "deb" | "rpm"
            | "iso" => Self::Archive,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "tif" | "tiff"
            | "avif" | "heic" => Self::Image,
            "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => Self::Video,
            "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" => Self::Audio,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_from_extension() {
        let t = |p: &str| FileType::from_path(Path::new(p), false);
        assert_eq!(t("main.rs"), FileType::Code);
        assert_eq!(t("Notes.PDF"), FileType::Pdf);
        assert_eq!(t("deck.pptx"), FileType::Presentation);
        assert_eq!(t("plan.xls"), FileType::Spreadsheet);
        assert_eq!(t("backup.tar.gz"), FileType::Archive);
        assert_eq!(t("README.md"), FileType::Text);
        assert_eq!(t("Makefile"), FileType::Other);
        assert_eq!(t(".bashrc"), FileType::Other);
    }

    #[test]
    fn directories_are_folders_whatever_the_name() {
        assert_eq!(
            FileType::from_path(Path::new("photos.zip"), true),
            FileType::Folder
        );
    }
}
