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

use crate::badge;
use crate::highlight;
use crate::theme;

pub struct PreviewView<'a> {
    preview: &'a Preview,
    scroll: usize,
    raw_markdown: bool,
}

impl<'a> PreviewView<'a> {
    pub fn new(preview: &'a Preview) -> Self {
        Self {
            preview,
            scroll: 0,
            raw_markdown: false,
        }
    }

    /// Markdown as source (highlighted, with line numbers) instead of formatted.
    pub fn raw_markdown(mut self, raw: bool) -> Self {
        self.raw_markdown = raw;
        self
    }

    /// First content line to show (text, folder names); images ignore it.
    pub fn scroll(mut self, scroll: usize) -> Self {
        self.scroll = scroll;
        self
    }
}

/// Rows the header takes: name, four lines of details, a rule.
pub const HEADER_ROWS: u16 = 6;

impl Widget for PreviewView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let p = self.preview;
        let body = Rect {
            y: area.y + HEADER_ROWS.min(area.height),
            height: area.height.saturating_sub(HEADER_ROWS),
            ..area
        };
        // The body first: a text tells which lines it showed, for the header.
        let mut position = None;
        if !body.is_empty() {
            match &p.content {
                Content::Text { lines, more, .. } => {
                    let lang = highlight::Lang::from_extension(
                        p.path.extension().and_then(|e| e.to_str()).unwrap_or(""),
                    );
                    let formatted = lang == highlight::Lang::Markdown && !self.raw_markdown;
                    let view = TextView {
                        lines,
                        more: *more,
                        scroll: self.scroll,
                        lang,
                        formatted,
                    };
                    position = Some(view.render(body, buf));
                }
                Content::Hex { lines, more } => {
                    position = Some(hex(lines, *more, self.scroll, body, buf));
                }
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
        header(p, position, area, buf);
    }
}

/// Badge and name, details (and which lines are on screen), a rule.
fn header(p: &Preview, position: Option<(usize, usize, usize)>, area: Rect, buf: &mut Buffer) {
    let color = theme::type_color(p.file_type);
    let name = p
        .path
        .file_name()
        .map_or_else(|| p.path.to_string_lossy(), |n| n.to_string_lossy());
    let extension = p.path.extension().and_then(|e| e.to_str());
    let badge = match p.file_type {
        FileType::Folder => "▸".to_string(),
        _ => extension.map_or_else(|| "·".into(), badge::extension_label),
    };
    let title = Line::from(vec![
        Span::raw(format!(" {badge} "))
            .bg(color)
            .fg(theme::text_on(color))
            .bold(),
        Span::raw(" "),
        Span::raw(name.into_owned()).fg(theme::fg()).bold(),
    ]);
    // Details as label / value rows (cardea's inspector), labels in one column.
    let size = match &p.content {
        Content::Folder { total, more, .. } if *more => trf("{}+ items", &[total]),
        Content::Folder { total, .. } => format::items(*total),
        _ => format::size(p.size),
    };
    let size = match &p.content {
        Content::Image { original, .. } => format!("{size} · {}×{}", original.0, original.1),
        Content::Text {
            source: Some(tool), ..
        } => format!("{size} · {}", trf("via {}", &[tool])),
        _ => size,
    };
    let modified = p
        .modified
        .map_or_else(|| "—".into(), |t| format::modified(t, format::now()));
    let mut perms = p.mode.map_or_else(|| "—".into(), format::permissions);
    if let Some(target) = &p.link_target {
        perms = format!("{perms}  → {}", target.display());
    }
    let details = [
        (
            tr("Type"),
            badge::type_label(extension, p.file_type == FileType::Folder),
        ),
        (tr("Size"), size),
        (tr("Modified"), modified),
        (tr("Permissions"), perms),
    ];
    let label_width = details
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    let rows = |y: u16| Rect::new(area.x, area.y + y, area.width, 1).intersection(area);
    Paragraph::new(title).render(rows(0), buf);
    for (i, (label, value)) in details.iter().enumerate() {
        let line = Line::from(vec![
            Span::raw(format!(" {label:<label_width$}  ")).fg(theme::dim()),
            Span::raw(value.as_str()).fg(theme::fg()),
        ]);
        Paragraph::new(line).render(rows(1 + i as u16), buf);
    }
    // The rule carries the position on its right: "──── 12–40 / 300 ".
    let mut rule = vec![Span::raw("─".repeat(usize::from(area.width))).fg(theme::border())];
    if let Some((first, last, total)) = position {
        let label = format!(" {first}–{last} / {total} ");
        let room = usize::from(area.width).saturating_sub(label.chars().count() + 1);
        rule = vec![
            Span::raw("─".repeat(room)).fg(theme::border()),
            Span::raw(label).fg(theme::dim()),
            Span::raw("─").fg(theme::border()),
        ];
    }
    Paragraph::new(Line::from(rule)).render(rows(HEADER_ROWS - 1), buf);
}

/// A text preview: highlighted source with line numbers, or formatted Markdown.
struct TextView<'a> {
    lines: &'a [String],
    more: bool,
    scroll: usize,
    lang: highlight::Lang,
    /// Markdown formatted (no markers, no line numbers) instead of as source.
    formatted: bool,
}

impl TextView<'_> {
    /// Draws from line `scroll`, wrapped. Returns (first line, last line, total) shown.
    fn render(&self, area: Rect, buf: &mut Buffer) -> (usize, usize, usize) {
        let lines = self.lines;
        let digits = lines.len().max(1).to_string().len();
        let gutter = if self.formatted { 1 } else { digits + 3 }; // "123 │ "
        let width = usize::from(area.width).saturating_sub(gutter).max(1);
        let start = self.scroll.min(lines.len().saturating_sub(1));
        // Comments and code fences that opened above the first line on screen.
        let above = lines[..start].iter().map(String::as_str);
        let mut state = highlight::state_after(self.lang, above);
        let mut in_code = state.in_fence();
        let height = usize::from(area.height);
        let mut out: Vec<Line> = Vec::with_capacity(height);
        let mut last = start;
        for (i, line) in lines.iter().enumerate().skip(start) {
            if out.len() >= height {
                break;
            }
            let spans = if self.formatted {
                crate::markdown::line(line, &mut in_code, width)
            } else {
                highlight::line_spans(self.lang, line, &mut state)
            };
            for (k, part) in wrap(spans, width).into_iter().enumerate() {
                if out.len() >= height {
                    break;
                }
                let number = match (self.formatted, k) {
                    (true, _) => " ".to_string(),
                    (false, 0) => format!("{:>digits$} │ ", i + 1),
                    (false, _) => format!("{:>digits$} │ ", ""),
                };
                let mut row = vec![Span::raw(number).fg(theme::border())];
                row.extend(part);
                out.push(Line::from(row));
            }
            last = i + 1;
        }
        if self.more && last == lines.len() && out.len() < height {
            out.push(Line::from("…").fg(theme::dim()));
        }
        Paragraph::new(out).render(area, buf);
        (start + 1, last, lines.len())
    }
}

/// A hex dump: offsets dim, bytes in the normal color, the text column in the accent color.
fn hex(
    lines: &[String],
    more: bool,
    scroll: usize,
    area: Rect,
    buf: &mut Buffer,
) -> (usize, usize, usize) {
    let start = scroll.min(lines.len().saturating_sub(1));
    let height = usize::from(area.height);
    let mut out: Vec<Line> = lines
        .iter()
        .skip(start)
        .take(height)
        .map(|l| {
            let (offset, rest) = l.split_at(l.len().min(8));
            let (bytes, text) = rest.split_at(rest.find("  |").unwrap_or(rest.len()));
            Line::from(vec![
                Span::raw(offset.to_string()).fg(theme::border()),
                Span::raw(bytes.to_string()).fg(theme::fg()),
                Span::raw(text.to_string()).fg(theme::accent()),
            ])
        })
        .collect();
    let last = (start + out.len()).min(lines.len());
    if more && last == lines.len() && out.len() < height {
        out.push(Line::from("…").fg(theme::dim()));
    }
    Paragraph::new(out).render(area, buf);
    (start + 1, last, lines.len())
}

/// Splits styled spans into rows of at most `width` cells (an empty line stays one row).
fn wrap(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let mut rows = vec![Vec::new()];
    let mut used = 0;
    for span in spans {
        let mut piece = String::new();
        for c in span.content.chars() {
            // Display width (wide characters like emoji take two cells), measured by ratatui.
            let w = Span::raw(&*c.encode_utf8(&mut [0; 4])).width();
            if used + w > width && used > 0 {
                if !piece.is_empty() {
                    rows.last_mut()
                        .expect("one row")
                        .push(Span::styled(std::mem::take(&mut piece), span.style));
                }
                rows.push(Vec::new());
                used = 0;
            }
            piece.push(c);
            used += w;
        }
        if !piece.is_empty() {
            rows.last_mut()
                .expect("one row")
                .push(Span::styled(piece, span.style));
        }
    }
    rows
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
    let mut out = vec![
        Line::from(Span::raw(count).fg(theme::fg()).bold()),
        Line::default(),
    ];
    // One bar per type: how much of the folder it is.
    const BAR: usize = 16;
    let counted: usize = by_type.iter().map(|(_, n)| n).sum::<usize>().max(1);
    for (t, n) in by_type.iter().take(5) {
        let filled = (n * BAR).div_ceil(counted).min(BAR);
        out.push(Line::from(vec![
            Span::raw("█".repeat(filled)).fg(theme::type_color(*t)),
            Span::raw("░".repeat(BAR - filled)).fg(theme::border()),
            Span::raw(format!("  {n} {}", tr(type_plural(*t, *n)))).fg(theme::type_color(*t)),
        ]));
    }
    out.push(Line::default());
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

    #[test]
    fn wide_characters_wrap_by_cells() {
        let rows = wrap(vec![Span::raw("ab✅cd")], 4);
        let text: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(text, ["ab✅", "cd"]);
    }
    use std::path::PathBuf;

    fn preview(name: &str, file_type: FileType, content: Content) -> Preview {
        Preview {
            path: PathBuf::from(name),
            file_type,
            size: 1200,
            modified: None,
            mode: Some(0o644),
            link_target: None,
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
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
        PreviewView::new(&p).render(buf.area, &mut buf);
        let rows = rows(&buf);
        assert!(rows[0].starts_with(" RS  main.rs"));
        assert!(
            rows[1].starts_with(" Type         RS file"),
            "{:?}",
            rows[1]
        );
        assert!(rows[2].starts_with(" Size         1.2 kB"));
        assert!(rows[4].starts_with(" Permissions  rw-r--r-- (644)"));
        assert!(rows[6].starts_with("1 │ fn main() {}"));
    }

    #[test]
    fn markdown_is_formatted_unless_raw() {
        let p = preview(
            "notes.md",
            FileType::Text,
            Content::Text {
                lines: vec!["# Plan".into(), "- **soon**".into()],
                more: false,
                source: None,
            },
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
        PreviewView::new(&p).render(buf.area, &mut buf);
        let formatted = rows(&buf);
        assert!(formatted[6].starts_with(" Plan"), "{:?}", formatted[6]);
        assert!(formatted[7].starts_with(" • soon"));
        PreviewView::new(&p)
            .raw_markdown(true)
            .render(buf.area, &mut buf);
        assert!(rows(&buf)[6].starts_with("1 │ # Plan"));
    }

    #[test]
    fn binary_files_show_a_hex_dump() {
        let p = preview(
            "x.bin",
            FileType::Other,
            Content::Hex {
                lines: vec!["00000000  7f 45  |.E|".into()],
                more: false,
            },
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
        PreviewView::new(&p).render(buf.area, &mut buf);
        assert!(rows(&buf)[6].starts_with("00000000  7f 45  |.E|"));
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
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 8));
        PreviewView::new(&p).render(buf.area, &mut buf);
        let cell = &buf[(1, 6)]; // under the 6 header rows; centered: (4 - 2) / 2 = 1
        assert_eq!(cell.symbol(), "▀");
        assert_eq!(cell.fg, Color::Rgb(255, 0, 0));
        assert_eq!(cell.bg, Color::Rgb(0, 0, 255));
    }
}
