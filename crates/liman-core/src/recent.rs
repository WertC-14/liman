//! Recently used files from GTK's list (`$XDG_DATA_HOME/recently-used.xbel`, written by Nautilus,
//! GNOME and most GTK apps). Read only: other programs own this file.

use std::fs;
use std::path::{Path, PathBuf};

/// At most this many recent files are shown.
pub const MAX_RECENT: usize = 200;

pub fn xbel_path(home: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("recently-used.xbel")
}

/// Existing local files from the list, newest first.
pub fn recent_files(home: &Path) -> Vec<PathBuf> {
    let text = fs::read_to_string(xbel_path(home)).unwrap_or_default();
    let mut items = parse_xbel(&text);
    items.sort_by(|a, b| b.1.cmp(&a.1)); // ISO dates sort as text
    items
        .into_iter()
        .map(|(path, _)| path)
        .filter(|p| p.symlink_metadata().is_ok())
        .take(MAX_RECENT)
        .collect()
}

/// (path, modified) for every `<bookmark href="file://…" … modified="…">`. A tiny scan instead of an
/// XML library: the file is generated and always uses this attribute layout.
fn parse_xbel(text: &str) -> Vec<(PathBuf, String)> {
    text.split("<bookmark ")
        .skip(1)
        .filter_map(|tag| {
            let tag = &tag[..tag.find('>')?];
            let href = attribute(tag, "href")?;
            let path = percent_decode(href.strip_prefix("file://")?);
            let modified = attribute(tag, "modified").unwrap_or_default().to_string();
            Some((PathBuf::from(path), modified))
        })
        .collect()
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

/// `%20` → space, `%C4%B0` → `İ` (UTF-8 bytes).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hrefs_and_dates() {
        let text = r#"<xbel>
  <bookmark href="file:///home/u/a%20b.md" added="x" modified="2026-07-07T10:58:11Z" visited="y">
  </bookmark>
  <bookmark href="file:///home/u/%C4%B0zle" modified="2026-07-09T11:27:11Z"></bookmark>
  <bookmark href="https://example.com" modified="2026-08-01T00:00:00Z"></bookmark>
</xbel>"#;
        let items = parse_xbel(text);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, PathBuf::from("/home/u/a b.md"));
        assert_eq!(items[1].0, PathBuf::from("/home/u/İzle"));
        assert_eq!(items[1].1, "2026-07-09T11:27:11Z");
    }
}
