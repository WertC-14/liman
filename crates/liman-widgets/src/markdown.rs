//! Markdown shown formatted in the preview, not as source (like cardea's reader): headings
//! without `#`, bullets, task boxes, quotes, rules, framed code blocks, and inline `code`,
//! **bold**, *italic* and [links](…) without their markers. Line by line: only a code fence
//! carries over to the next line, so scrolling can start anywhere (see `highlight::state_after`).

use liman_core::FileType;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::theme;

/// One source line as styled spans (not wrapped yet). `in_code`: inside a fenced code block,
/// updated by fence lines. `width` is only used for horizontal rules.
pub fn line(text: &str, in_code: &mut bool, width: usize) -> Vec<Span<'static>> {
    let trimmed = text.trim_start();
    let frame = Style::new().fg(theme::border());
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        *in_code = !*in_code;
        return if *in_code {
            let lang = trimmed.trim_start_matches(['`', '~']).trim();
            vec![
                Span::styled("┌─ ", frame),
                Span::styled(lang.to_string(), Style::new().fg(theme::dim())),
            ]
        } else {
            vec![Span::styled("└─", frame)]
        };
    }
    if *in_code {
        return vec![
            Span::styled("│ ", frame),
            Span::styled(text.to_string(), code_style()),
        ];
    }
    if is_rule(trimmed) {
        return vec![Span::styled("─".repeat(width.clamp(3, 60)), frame)];
    }
    if let Some((level, title)) = heading(trimmed) {
        let color = match level {
            1 => theme::accent(),
            2 => theme::type_color(FileType::Code),
            _ => theme::fg(),
        };
        let mut style = Style::new().fg(color).add_modifier(Modifier::BOLD);
        if level == 1 {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        return vec![Span::styled(title.to_string(), style)];
    }
    if let Some(quote) = trimmed.strip_prefix('>') {
        let mut out = vec![Span::styled("▎ ", Style::new().fg(theme::accent()))];
        out.extend(
            inline(quote.trim_start())
                .into_iter()
                .map(|s| s.patch_style(Style::new().add_modifier(Modifier::ITALIC))),
        );
        return out;
    }
    let indent = &text[..text.len() - trimmed.len()];
    let bullet = Style::new().fg(theme::type_color(FileType::Archive));
    let (marker, rest) = if let Some(rest) = trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("* [ ] "))
    {
        ("[ ] ".to_string(), rest)
    } else if let Some(rest) = trimmed
        .strip_prefix("- [x] ")
        .or_else(|| trimmed.strip_prefix("- [X] "))
        .or_else(|| trimmed.strip_prefix("* [x] "))
    {
        ("[✓] ".to_string(), rest)
    } else if let Some(rest) = ["- ", "* ", "+ "]
        .iter()
        .find_map(|m| trimmed.strip_prefix(m))
    {
        ("• ".to_string(), rest)
    } else {
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        match trimmed[digits..].strip_prefix(". ") {
            Some(rest) if digits > 0 => (format!("{}. ", &trimmed[..digits]), rest),
            _ => (String::new(), trimmed),
        }
    };
    let mut out = Vec::new();
    if !indent.is_empty() {
        out.push(Span::raw(indent.to_string()));
    }
    if !marker.is_empty() {
        out.push(Span::styled(marker, bullet));
    }
    out.extend(inline(rest));
    out
}

fn code_style() -> Style {
    Style::new().fg(theme::type_color(FileType::Spreadsheet))
}

fn is_rule(line: &str) -> bool {
    let line = line.trim_end();
    line.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|c| line.chars().all(|x| x.to_string() == *c || x == ' '))
}

/// `## Title` → (2, "Title").
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    let title = line[level..].strip_prefix(' ')?;
    (1..=6)
        .contains(&level)
        .then(|| (level, title.trim_end_matches(['#', ' '])))
}

/// Inline markup without its markers: `code`, **bold**, *italic* / _italic_, [text](url),
/// ![alt](image).
fn inline(text: &str) -> Vec<Span<'static>> {
    let plain = Style::new().fg(theme::fg());
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut rest = text;
    let flush = |buf: &mut String, out: &mut Vec<Span<'static>>| {
        if !buf.is_empty() {
            out.push(Span::styled(std::mem::take(buf), plain));
        }
    };
    while let Some(c) = rest.chars().next() {
        let after_space = buf.is_empty() || buf.ends_with(' ');
        // (marker length, closing marker, style) for the markup starting here.
        let span: Option<(usize, &str, Style)> = if rest.starts_with("**") {
            Some((2, "**", plain.add_modifier(Modifier::BOLD)))
        } else if c == '`' {
            Some((1, "`", code_style()))
        } else if (c == '*' || c == '_') && after_space {
            Some((
                1,
                if c == '*' { "*" } else { "_" },
                plain.add_modifier(Modifier::ITALIC),
            ))
        } else {
            None
        };
        if let Some((open, close, style)) = span
            && let Some(end) = rest[open..].find(close).filter(|&e| e > 0)
        {
            flush(&mut buf, &mut out);
            out.push(Span::styled(rest[open..open + end].to_string(), style));
            rest = &rest[open + end + close.len()..];
            continue;
        }
        // [text](url) and ![alt](src): the text only.
        let image = rest.starts_with("![");
        if (c == '[' || image)
            && let Some(close) = rest.find("](")
            && let Some(paren) = rest[close..].find(')')
        {
            let label = &rest[if image { 2 } else { 1 }..close];
            flush(&mut buf, &mut out);
            if image {
                out.push(Span::styled(
                    format!("[{}: {label}]", liman_core::i18n::tr("image")),
                    Style::new().fg(theme::dim()),
                ));
            } else {
                out.push(Span::styled(
                    label.to_string(),
                    Style::new()
                        .fg(theme::accent())
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            rest = &rest[close + paren + 1..];
            continue;
        }
        buf.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut buf, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn markers_disappear() {
        let mut code = false;
        let mut show = |l: &str| text(&line(l, &mut code, 20));
        assert_eq!(show("# Title #"), "Title");
        assert_eq!(
            show("Some **bold** and `code` and *it*."),
            "Some bold and code and it."
        );
        assert_eq!(show("- see [docs](a.md)"), "• see docs");
        assert_eq!(show("  - [x] done"), "  [✓] done");
        assert_eq!(show("2. second"), "2. second");
        assert_eq!(show("> quoted"), "▎ quoted");
        assert_eq!(show("---"), "─".repeat(20));
        assert_eq!(show("snake_case_name stays"), "snake_case_name stays");
        assert_eq!(show("![logo](logo.png)"), "[image: logo]");
    }

    #[test]
    fn code_blocks_are_framed_and_kept_as_is() {
        let mut code = false;
        assert_eq!(text(&line("```rust", &mut code, 20)), "┌─ rust");
        assert!(code);
        assert_eq!(
            text(&line("# not a heading", &mut code, 20)),
            "│ # not a heading"
        );
        assert_eq!(text(&line("```", &mut code, 20)), "└─");
        assert!(!code);
    }
}
