//! The preview panel (F3): a header with name, type, size and date, then the content.
//! Images are drawn with half blocks: each cell shows two pixels, the upper one as the
//! foreground of `▀` and the lower one as the background.

use liman_core::FileType;
use liman_core::format;
use liman_core::i18n::{tr, trf};
use liman_core::preview::{Content, Preview};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme;

pub struct PreviewView<'a> {
    preview: &'a Preview,
    scroll: usize,
}

impl<'a> PreviewView<'a> {
    pub fn new(preview: &'a Preview) -> Self {
        Self { preview, scroll: 0 }
    }

    /// First content line to show (text, folder names); images ignore it.
    pub fn scroll(mut self, scroll: usize) -> Self {
        self.scroll = scroll;
        self
    }
}

/// Rows the header takes: name, details, a blank line.
pub const HEADER_ROWS: u16 = 3;

impl Widget for PreviewView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let p = self.preview;
        let name = p
            .path
            .file_name()
            .map_or_else(|| p.path.to_string_lossy(), |n| n.to_string_lossy());
        let mut details = vec![type_label(p)];
        if p.file_type != FileType::Folder {
            details.push(format::size(p.size));
        }
        if let Some(time) = p.modified {
            details.push(format::modified(time, format::now()));
        }
        match &p.content {
            Content::Image { original, .. } => {
                details.push(format!("{}×{}", original.0, original.1))
            }
            Content::Text {
                source: Some(tool), ..
            } => details.push(trf("via {}", &[tool])),
            _ => {}
        }
        let header = vec![
            Line::from(
                Span::raw(name.into_owned())
                    .fg(theme::type_color(p.file_type))
                    .bold(),
            ),
            Line::from(details.join(" · ")).fg(theme::dim()),
        ];
        Paragraph::new(header).render(area, buf);

        let body = Rect {
            y: area.y + HEADER_ROWS.min(area.height),
            height: area.height.saturating_sub(HEADER_ROWS),
            ..area
        };
        if body.is_empty() {
            return;
        }
        match &p.content {
            Content::Text { lines, more, .. } => text(lines, *more, self.scroll, body, buf),
            Content::Folder {
                total,
                more,
                by_type,
                names,
            } => folder(*total, *more, by_type, names, self.scroll, body, buf),
            Content::Image {
                width,
                height,
                pixels,
                ..
            } => image(*width, *height, pixels, body, buf),
            Content::Note(note) => {
                Paragraph::new(note.as_str())
                    .style(Style::new().fg(theme::dim()).add_modifier(Modifier::ITALIC))
                    .render(body, buf);
            }
        }
    }
}

/// "PDF file", "Folder", "File".
fn type_label(p: &Preview) -> String {
    if p.file_type == FileType::Folder {
        return tr("Folder").into();
    }
    match p.path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() => trf(
            "{} file",
            &[&ext.chars().take(5).collect::<String>().to_uppercase()],
        ),
        _ => tr("File").into(),
    }
}

fn text(lines: &[String], more: bool, scroll: usize, area: Rect, buf: &mut Buffer) {
    let digits = lines.len().max(1).to_string().len();
    let start = scroll.min(lines.len().saturating_sub(1));
    let mut out: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(start)
        .take(usize::from(area.height))
        .map(|(i, line)| {
            Line::from(vec![
                Span::raw(format!("{:>digits$} ", i + 1)).fg(theme::border()),
                Span::raw(line.as_str()).fg(theme::fg()),
            ])
        })
        .collect();
    if more && out.len() < usize::from(area.height) {
        out.push(Line::from("…").fg(theme::dim()));
    }
    Paragraph::new(out).render(area, buf);
}

fn folder(
    total: usize,
    more: bool,
    by_type: &[(FileType, usize)],
    names: &[(String, bool)],
    scroll: usize,
    area: Rect,
    buf: &mut Buffer,
) {
    let count = if more {
        trf("{}+ items", &[&total])
    } else {
        format::items(total)
    };
    let mut summary = vec![Span::raw(count).fg(theme::fg()).bold()];
    for (t, n) in by_type.iter().take(4) {
        summary.push(Span::raw("  "));
        summary
            .push(Span::raw(format!("{n} {}", tr(type_plural(*t, *n)))).fg(theme::type_color(*t)));
    }
    let mut out = vec![Line::from(summary), Line::default()];
    let room = usize::from(area.height).saturating_sub(out.len());
    let start = scroll.min(names.len().saturating_sub(1));
    out.extend(names.iter().skip(start).take(room).map(|(name, is_dir)| {
        if *is_dir {
            Line::from(Span::raw(format!("{name}/")).fg(theme::accent()).bold())
        } else {
            let t = FileType::from_path(std::path::Path::new(name), false);
            Line::from(vec![
                Span::raw("· ").fg(theme::type_color(t)),
                Span::raw(name.as_str()).fg(theme::fg()),
            ])
        }
    }));
    Paragraph::new(out).render(area, buf);
}

fn type_plural(t: FileType, n: usize) -> &'static str {
    let one = n == 1;
    match t {
        FileType::Folder if one => "folder",
        FileType::Folder => "folders",
        FileType::Code => "code",
        FileType::Config => "config",
        FileType::Text => "text",
        FileType::Pdf => "PDF",
        FileType::Document if one => "document",
        FileType::Document => "documents",
        FileType::Spreadsheet if one => "sheet",
        FileType::Spreadsheet => "sheets",
        FileType::Presentation if one => "slide deck",
        FileType::Presentation => "slide decks",
        FileType::Archive if one => "archive",
        FileType::Archive => "archives",
        FileType::Image if one => "image",
        FileType::Image => "images",
        FileType::Video if one => "video",
        FileType::Video => "videos",
        FileType::Audio => "audio",
        FileType::Other => "other",
    }
}

/// Half-block image, centered horizontally; transparent pixels blend into the panel background.
fn image(width: u32, height: u32, pixels: &[[u8; 4]], area: Rect, buf: &mut Buffer) {
    let bg = match theme::bg() {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => [0, 0, 0],
    };
    let pixel = |x: u32, y: u32| -> Color {
        if y >= height {
            return Color::Rgb(bg[0], bg[1], bg[2]);
        }
        let [r, g, b, a] = pixels[(y * width + x) as usize];
        let mix = |c: u8, under: u8| {
            ((u16::from(c) * u16::from(a) + u16::from(under) * (255 - u16::from(a))) / 255) as u8
        };
        Color::Rgb(mix(r, bg[0]), mix(g, bg[1]), mix(b, bg[2]))
    };
    let cols = width.min(u32::from(area.width));
    let rows = height.div_ceil(2).min(u32::from(area.height));
    let left = area.x + ((u32::from(area.width) - cols) / 2) as u16;
    for row in 0..rows {
        for col in 0..cols {
            let cell = &mut buf[(left + col as u16, area.y + row as u16)];
            cell.set_char('▀')
                .set_fg(pixel(col, row * 2))
                .set_bg(pixel(col, row * 2 + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn preview(name: &str, file_type: FileType, content: Content) -> Preview {
        Preview {
            path: PathBuf::from(name),
            file_type,
            size: 1200,
            modified: None,
            content,
        }
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn text_has_header_and_line_numbers() {
        let p = preview(
            "main.rs",
            FileType::Code,
            Content::Text {
                lines: vec!["fn main() {}".into()],
                more: false,
                source: None,
            },
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 5));
        PreviewView::new(&p).render(buf.area, &mut buf);
        let rows = rows(&buf);
        assert!(rows[0].starts_with("main.rs"));
        assert!(rows[1].starts_with("RS file · 1.2 kB"));
        assert!(rows[3].starts_with("1 fn main() {}"));
    }

    #[test]
    fn image_cells_carry_two_pixels() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let p = preview(
            "a.png",
            FileType::Image,
            Content::Image {
                width: 2,
                height: 2,
                original: (2, 2),
                pixels: vec![red, red, blue, blue],
            },
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 4));
        PreviewView::new(&p).render(buf.area, &mut buf);
        let cell = &buf[(1, 3)]; // centered: (4 - 2) / 2 = 1
        assert_eq!(cell.symbol(), "▀");
        assert_eq!(cell.fg, Color::Rgb(255, 0, 0));
        assert_eq!(cell.bg, Color::Rgb(0, 0, 255));
    }
}
