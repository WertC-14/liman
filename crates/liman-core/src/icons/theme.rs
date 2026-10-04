//! Finding icon files in a freedesktop icon theme
//! (https://specifications.freedesktop.org/icon-theme-spec/latest/).
//!
//! Simplified for our use: we only need SVG files, at any size, because we rasterize to 16×16
//! ourselves. So we look in `scalable` directories first, then fixed-size ones from large to small,
//! through the theme, its `Inherits=` chain and finally `hicolor`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Name of the user's icon theme. Order: `LIMAN_ICON_THEME`, GNOME settings, GTK 4/3 `settings.ini`,
/// KDE `kdeglobals`. `None` when nothing is configured (e.g. a headless server).
pub fn detect_theme_name(home: &Path) -> Option<String> {
    if let Some(name) = std::env::var("LIMAN_ICON_THEME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return Some(name);
    }
    if let Some(name) = gsettings() {
        return Some(name);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    for (file, key) in [
        ("gtk-4.0/settings.ini", "gtk-icon-theme-name"),
        ("gtk-3.0/settings.ini", "gtk-icon-theme-name"),
        ("kdeglobals", "Theme"),
    ] {
        if let Some(name) = fs::read_to_string(config.join(file))
            .ok()
            .and_then(|text| ini_value(&text, key))
        {
            return Some(name);
        }
    }
    None
}

fn gsettings() -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()?;
    let value = String::from_utf8(out.stdout).ok()?;
    let value = value.trim().trim_matches('\'');
    (out.status.success() && !value.is_empty()).then(|| value.to_string())
}

/// First `key=value` in an ini-like text (sections are ignored, good enough for these files).
fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
        .filter(|v| !v.is_empty())
}

/// One theme in the lookup chain, with its directories already sorted by preference.
struct Theme {
    roots: Vec<PathBuf>,
    dirs: Vec<String>,
}

pub struct IconTheme {
    chain: Vec<Theme>,
}

impl IconTheme {
    /// Loads `name` and everything it inherits from, ending with `hicolor`.
    pub fn load(name: Option<&str>, home: &Path) -> Self {
        let bases = base_dirs(home);
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut queue: Vec<String> = name.map(String::from).into_iter().collect();
        queue.push("hicolor".into());
        while !queue.is_empty() {
            let current = queue.remove(0);
            if !seen.insert(current.clone()) {
                continue;
            }
            let roots: Vec<PathBuf> = bases
                .iter()
                .map(|b| b.join(&current))
                .filter(|p| p.join("index.theme").is_file())
                .collect();
            let Some(index) = roots
                .first()
                .and_then(|r| fs::read_to_string(r.join("index.theme")).ok())
            else {
                continue;
            };
            let parents: Vec<String> = ini_value(&index, "Inherits")
                .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();
            // Parents before hicolor (hicolor is always last).
            let insert_at = queue
                .iter()
                .position(|q| q == "hicolor")
                .unwrap_or(queue.len());
            for (i, parent) in parents.into_iter().enumerate() {
                queue.insert(insert_at + i, parent);
            }
            chain.push(Theme {
                roots,
                dirs: sorted_dirs(&index),
            });
        }
        Self { chain }
    }

    /// Path of the first SVG found for any of `names` (in order), searching the whole chain per name.
    pub fn find(&self, names: &[&str]) -> Option<PathBuf> {
        names.iter().find_map(|name| {
            self.chain.iter().find_map(|theme| {
                theme.dirs.iter().find_map(|dir| {
                    theme.roots.iter().find_map(|root| {
                        let path = root.join(dir).join(format!("{name}.svg"));
                        path.is_file().then_some(path)
                    })
                })
            })
        })
    }

    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }
}

/// `$XDG_DATA_HOME/icons`, `~/.icons`, each `$XDG_DATA_DIRS/icons`.
fn base_dirs(home: &Path) -> Vec<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut dirs = vec![data_home.join("icons"), home.join(".icons")];
    dirs.extend(data_dirs.split(':').map(|d| Path::new(d).join("icons")));
    dirs
}

/// Directories from `Directories=` (and `ScaledDirectories=`), scalable first, then larger sizes first.
fn sorted_dirs(index: &str) -> Vec<String> {
    let mut dirs: Vec<(bool, u32, String)> = Vec::new();
    for key in ["Directories", "ScaledDirectories"] {
        let Some(list) = ini_value(index, key) else {
            continue;
        };
        for dir in list.split(',').map(str::trim).filter(|d| !d.is_empty()) {
            let section = section(index, dir);
            let scalable = ini_value(section, "Type").is_some_and(|t| t == "Scalable")
                || dir.contains("scalable");
            let size = ini_value(section, "Size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            dirs.push((scalable, size, dir.to_string()));
        }
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    dirs.dedup_by(|a, b| a.2 == b.2);
    dirs.into_iter().map(|(_, _, d)| d).collect()
}

/// Text of the `[name]` section of an ini file (empty if missing).
fn section<'a>(text: &'a str, name: &str) -> &'a str {
    let header = format!("[{name}]");
    let Some(start) = text.find(&header) else {
        return "";
    };
    let body = &text[start + header.len()..];
    let end = body.find("\n[").unwrap_or(body.len());
    &body[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn ini_values() {
        let text = "[Settings]\ngtk-icon-theme-name=kora\nother = x\n";
        assert_eq!(
            ini_value(text, "gtk-icon-theme-name").as_deref(),
            Some("kora")
        );
        assert_eq!(ini_value(text, "other").as_deref(), Some("x"));
        assert_eq!(ini_value(text, "missing"), None);
    }

    #[test]
    fn scalable_first_then_larger() {
        let index = "[Icon Theme]\nDirectories=16/places,scalable/places,64/places\n\
                     [16/places]\nSize=16\n[scalable/places]\nSize=64\nType=Scalable\n[64/places]\nSize=64\n";
        assert_eq!(
            sorted_dirs(index),
            ["scalable/places", "64/places", "16/places"]
        );
    }

    #[test]
    fn finds_icons_through_inheritance() {
        let home = std::env::temp_dir().join(format!("liman-icons-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let icons = home.join(".icons");
        write(
            &icons.join("child/index.theme"),
            "[Icon Theme]\nInherits=parent\nDirectories=places/scalable\n[places/scalable]\nType=Scalable\n",
        );
        write(&icons.join("child/places/scalable/folder.svg"), "<svg/>");
        write(
            &icons.join("parent/index.theme"),
            "[Icon Theme]\nDirectories=mimes/32\n[mimes/32]\nSize=32\n",
        );
        write(&icons.join("parent/mimes/32/application-pdf.svg"), "<svg/>");

        let theme = IconTheme::load(Some("child"), &home);
        assert_eq!(
            theme.find(&["folder"]),
            Some(icons.join("child/places/scalable/folder.svg"))
        );
        assert_eq!(
            theme.find(&["missing", "application-pdf"]),
            Some(icons.join("parent/mimes/32/application-pdf.svg"))
        );
        assert_eq!(theme.find(&["nothing"]), None);
        fs::remove_dir_all(&home).unwrap();
    }
}
