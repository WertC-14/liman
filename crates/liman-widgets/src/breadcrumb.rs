//! Clickable path bar: `⌂ Home › Projects › liman`.

use std::path::{Path, PathBuf};

use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use crate::theme;

const SEPARATOR: &str = " › ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub label: String,
    pub path: PathBuf,
}

/// Splits `path` into clickable segments. Paths under `home` start at "Home", others at "/".
pub fn segments(path: &Path, home: &Path) -> Vec<Segment> {
    let (mut current, first, rest) = match path.strip_prefix(home) {
        Ok(rest) => (
            home.to_path_buf(),
            format!("⌂ {}", liman_core::i18n::tr("Home")),
            rest,
        ),
        Err(_) => (
            PathBuf::from("/"),
            "/".to_string(),
            path.strip_prefix("/").unwrap_or(path),
        ),
    };
    let mut out = vec![Segment {
        label: first,
        path: current.clone(),
    }];
    for part in rest.components() {
        current.push(part);
        out.push(Segment {
            label: part.as_os_str().to_string_lossy().into_owned(),
            path: current.clone(),
        });
    }
    out
}

/// The path bar line: each folder is a "chip" (`␣label␣` on a tinted background), the current one bright.
pub fn line(segments: &[Segment]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(SEPARATOR).fg(theme::dim()));
        }
        let last = i + 1 == segments.len();
        let chip = Span::raw(format!(" {} ", seg.label));
        spans.push(if last {
            chip.fg(theme::fg()).bg(theme::selected_bg()).bold()
        } else {
            chip.fg(theme::dim()).bg(theme::bar_bg())
        });
    }
    Line::from(spans)
}

/// Which segment is under `column`, for a line drawn starting at `x`.
pub fn segment_at(segments: &[Segment], x: u16, column: u16) -> Option<usize> {
    let mut start = usize::from(x) + 1; // leading space
    let column = usize::from(column);
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            start += Span::raw(SEPARATOR).width();
        }
        let end = start + Span::raw(seg.label.as_str()).width() + 2; // chip padding
        if (start..end).contains(&column) {
            return Some(i);
        }
        start = end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_under_home() {
        let segs = segments(Path::new("/home/u/Projects/liman"), Path::new("/home/u"));
        let labels: Vec<_> = segs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["⌂ Home", "Projects", "liman"]);
        assert_eq!(segs[1].path, PathBuf::from("/home/u/Projects"));
    }

    #[test]
    fn segments_outside_home() {
        let segs = segments(Path::new("/etc/ssh"), Path::new("/home/u"));
        let labels: Vec<_> = segs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["/", "etc", "ssh"]);
        assert_eq!(segments(Path::new("/"), Path::new("/home/u")).len(), 1);
    }

    #[test]
    fn segment_at_finds_the_label_under_the_mouse() {
        // " [ ⌂ Home ] › [ Projects ]" -> chip 1..9, separator 9..12, chip 12..22
        let segs = segments(Path::new("/home/u/Projects"), Path::new("/home/u"));
        assert_eq!(segment_at(&segs, 0, 0), None);
        assert_eq!(segment_at(&segs, 0, 1), Some(0));
        assert_eq!(segment_at(&segs, 0, 8), Some(0));
        assert_eq!(segment_at(&segs, 0, 10), None);
        assert_eq!(segment_at(&segs, 0, 12), Some(1));
        assert_eq!(segment_at(&segs, 0, 21), Some(1));
        assert_eq!(segment_at(&segs, 0, 22), None);
    }
}
