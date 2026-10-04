//! Clickable path bar: `⌂ Home › Projects › liman`.

use std::path::{Path, PathBuf};

use ratatui::style::Stylize;
use ratatui::text::{Line, Span};

use crate::theme::{DIM, FG};

const SEPARATOR: &str = " › ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub label: String,
    pub path: PathBuf,
}

/// Splits `path` into clickable segments. Paths under `home` start at "Home", others at "/".
pub fn segments(path: &Path, home: &Path) -> Vec<Segment> {
    let (mut current, first, rest) = match path.strip_prefix(home) {
        Ok(rest) => (home.to_path_buf(), "⌂ Home".to_string(), rest),
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

/// The path bar line; the last segment (current folder) is bright.
pub fn line(segments: &[Segment]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(SEPARATOR).fg(DIM));
        }
        let last = i + 1 == segments.len();
        let span = Span::raw(seg.label.clone());
        spans.push(if last {
            span.fg(FG).bold()
        } else {
            span.fg(DIM)
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
        let end = start + Span::raw(seg.label.as_str()).width();
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
        // " ⌂ Home › Projects"  -> "⌂ Home" at 1..7, separator 7..10, "Projects" at 10..18
        let segs = segments(Path::new("/home/u/Projects"), Path::new("/home/u"));
        assert_eq!(segment_at(&segs, 0, 0), None);
        assert_eq!(segment_at(&segs, 0, 1), Some(0));
        assert_eq!(segment_at(&segs, 0, 6), Some(0));
        assert_eq!(segment_at(&segs, 0, 8), None);
        assert_eq!(segment_at(&segs, 0, 10), Some(1));
        assert_eq!(segment_at(&segs, 0, 17), Some(1));
        assert_eq!(segment_at(&segs, 0, 18), None);
    }
}
