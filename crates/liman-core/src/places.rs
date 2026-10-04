//! Well-known places: the home folder, XDG user directories (Downloads, Music, ...), Trash and the root.
//! Used for the sidebar and for special folder symbols.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecialDir {
    Home,
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
    Templates,
    Public,
    Trash,
    Root,
    /// A folder the user bookmarked (Ctrl+D).
    Bookmark,
    /// Recently used files (not a folder: opens a list).
    Recent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    pub kind: SpecialDir,
}

/// Every known special directory with its path (used to mark entries in listings).
#[derive(Debug, Clone, Default)]
pub struct Places {
    pub home: PathBuf,
    special: Vec<(SpecialDir, PathBuf)>,
}

impl Places {
    /// Reads `$XDG_CONFIG_HOME/user-dirs.dirs` (or `~/.config/user-dirs.dirs`). Missing file: no XDG dirs.
    pub fn detect(home: &Path) -> Self {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let content = fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
        Self::from_user_dirs(&content, home)
    }

    pub fn from_user_dirs(content: &str, home: &Path) -> Self {
        let mut special = vec![(SpecialDir::Home, home.to_path_buf())];
        special.extend(parse_user_dirs(content, home));
        special.push((SpecialDir::Trash, trash_files(home)));
        special.push((SpecialDir::Root, PathBuf::from("/")));
        Self {
            home: home.to_path_buf(),
            special,
        }
    }

    pub fn kind_of(&self, path: &Path) -> Option<SpecialDir> {
        self.special
            .iter()
            .find(|(_, p)| p == path)
            .map(|(kind, _)| *kind)
    }

    /// Sidebar with the user's bookmarks: Home, Recent, XDG folders, bookmarks, Trash, Computer.
    pub fn sidebar_with(&self, bookmarks: &[PathBuf]) -> Vec<Place> {
        let mut places = self.sidebar();
        let after_home = places
            .iter()
            .position(|p| p.kind == SpecialDir::Home)
            .map_or(0, |i| i + 1);
        places.insert(
            after_home,
            Place {
                name: crate::i18n::tr("Recent").into(),
                path: PathBuf::from("recent:///"),
                kind: SpecialDir::Recent,
            },
        );
        let before_trash = places
            .iter()
            .position(|p| p.kind == SpecialDir::Trash)
            .unwrap_or(places.len());
        for (i, path) in bookmarks.iter().enumerate() {
            places.insert(
                before_trash + i,
                Place {
                    name: display_name(SpecialDir::Bookmark, path),
                    path: path.clone(),
                    kind: SpecialDir::Bookmark,
                },
            );
        }
        places
    }

    /// Sidebar entries in GUI order. Templates and Public are left out (Nautilus does the same);
    /// XDG dirs that do not exist or point to the home folder itself are skipped.
    pub fn sidebar(&self) -> Vec<Place> {
        use SpecialDir::*;
        let order = [
            Home, Desktop, Documents, Downloads, Music, Pictures, Videos, Trash, Root,
        ];
        order
            .iter()
            .filter_map(|kind| {
                let (_, path) = self.special.iter().find(|(k, _)| k == kind)?;
                let xdg = !matches!(kind, Home | Trash | Root);
                if xdg && (path == &self.home || !path.is_dir()) {
                    return None;
                }
                Some(Place {
                    name: display_name(*kind, path),
                    path: path.clone(),
                    kind: *kind,
                })
            })
            .collect()
    }
}

fn display_name(kind: SpecialDir, path: &Path) -> String {
    match kind {
        SpecialDir::Home => crate::i18n::tr("Home").into(),
        SpecialDir::Trash => crate::i18n::tr("Trash").into(),
        SpecialDir::Recent => crate::i18n::tr("Recent").into(),
        SpecialDir::Root => crate::i18n::tr("Computer").into(),
        _ => path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    }
}

/// Files in the freedesktop trash live in `$XDG_DATA_HOME/Trash/files`.
fn trash_files(home: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("Trash/files")
}

/// Parses lines like `XDG_DOWNLOAD_DIR="$HOME/Downloads"`. Unknown keys are ignored.
pub fn parse_user_dirs(content: &str, home: &Path) -> Vec<(SpecialDir, PathBuf)> {
    content
        .lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            let kind = match key {
                "XDG_DESKTOP_DIR" => SpecialDir::Desktop,
                "XDG_DOCUMENTS_DIR" => SpecialDir::Documents,
                "XDG_DOWNLOAD_DIR" => SpecialDir::Downloads,
                "XDG_MUSIC_DIR" => SpecialDir::Music,
                "XDG_PICTURES_DIR" => SpecialDir::Pictures,
                "XDG_VIDEOS_DIR" => SpecialDir::Videos,
                "XDG_TEMPLATES_DIR" => SpecialDir::Templates,
                "XDG_PUBLICSHARE_DIR" => SpecialDir::Public,
                _ => return None,
            };
            let value = value.trim().trim_matches('"');
            let path = match value.strip_prefix("$HOME") {
                Some(rest) => home.join(rest.trim_start_matches('/')),
                None if value.starts_with('/') => PathBuf::from(value),
                None => return None,
            };
            Some((kind, path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# comment
XDG_DESKTOP_DIR="$HOME/Desktop"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_PUBLICSHARE_DIR="$HOME/"
XDG_MUSIC_DIR="/mnt/music"
XDG_PROJECTS_DIR="$HOME/Projects"
"#;

    #[test]
    fn parses_known_keys_only() {
        let dirs = parse_user_dirs(SAMPLE, Path::new("/home/u"));
        assert_eq!(
            dirs,
            [
                (SpecialDir::Desktop, PathBuf::from("/home/u/Desktop")),
                (SpecialDir::Downloads, PathBuf::from("/home/u/Downloads")),
                (SpecialDir::Public, PathBuf::from("/home/u")),
                (SpecialDir::Music, PathBuf::from("/mnt/music")),
            ]
        );
    }

    #[test]
    fn kind_of_marks_special_paths() {
        let places = Places::from_user_dirs(SAMPLE, Path::new("/home/u"));
        assert_eq!(
            places.kind_of(Path::new("/home/u/Downloads")),
            Some(SpecialDir::Downloads)
        );
        assert_eq!(places.kind_of(Path::new("/home/u")), Some(SpecialDir::Home));
        assert_eq!(places.kind_of(Path::new("/home/u/Other")), None);
    }

    #[test]
    fn sidebar_keeps_home_trash_root_and_existing_xdg_dirs() {
        let home = std::env::temp_dir().join(format!("liman-places-{}", std::process::id()));
        fs::create_dir_all(home.join("Downloads")).unwrap();
        let places = Places::from_user_dirs(SAMPLE, &home);
        let names: Vec<_> = places.sidebar().into_iter().map(|p| p.name).collect();
        // Desktop does not exist, Music is outside and missing, Public is the home folder itself.
        assert_eq!(names, ["Home", "Downloads", "Trash", "Computer"]);
        fs::remove_dir_all(&home).unwrap();
    }
}
