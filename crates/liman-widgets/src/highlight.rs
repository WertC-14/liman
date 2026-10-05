//! A small syntax colorizer for the preview: comments, strings, numbers, keywords and type-like
//! names for C-like, hash-comment and dash-comment languages, and Markdown structure. It is not
//! a parser; it only has to make a file easier to read at a glance. No dependency (ADR 0007 spirit).

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use liman_core::FileType;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// `//` and `/* */` comments: Rust, C, C++, Go, Java, JS/TS, Swift, Kotlin, C#, Zig, CSS...
    CLike,
    /// `#` comments: Python, shell, Ruby, TOML, YAML, Makefile, config files.
    Hash,
    /// `--` comments: SQL, Lua, Haskell.
    Dash,
    Markdown,
    Plain,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "rs" | "c" | "h" | "cpp" | "hpp" | "cc" | "go" | "java" | "kt" | "swift" | "js"
            | "mjs" | "ts" | "tsx" | "jsx" | "cs" | "zig" | "css" | "scss" | "php" | "json"
            | "dart" | "scala" => Self::CLike,
            "py" | "sh" | "bash" | "zsh" | "fish" | "rb" | "toml" | "yaml" | "yml" | "conf"
            | "cfg" | "ini" | "env" | "mk" | "r" | "pl" | "nix" => Self::Hash,
            "sql" | "lua" | "hs" => Self::Dash,
            "md" | "markdown" => Self::Markdown,
            _ => Self::Plain,
        }
    }
}

/// Carried from line to line: inside a `/* */` comment or a Markdown code fence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    block_comment: bool,
    fence: bool,
}

const KEYWORDS: &[&str] = &[
    // shared by many languages
    "if",
    "else",
    "for",
    "while",
    "loop",
    "do",
    "break",
    "continue",
    "return",
    "match",
    "switch",
    "case",
    "default",
    "in",
    "of",
    "as",
    "is",
    "not",
    "and",
    "or",
    "true",
    "false",
    "null",
    "nil",
    "None",
    "True",
    "False",
    "self",
    "Self",
    "this",
    "super",
    "new",
    "try",
    "catch",
    "except",
    "finally",
    "throw",
    "raise",
    "with",
    "yield",
    "await",
    "async",
    "import",
    "from",
    "export",
    "package",
    "use",
    "mod",
    "pub",
    "fn",
    "func",
    "function",
    "def",
    "class",
    "struct",
    "enum",
    "trait",
    "impl",
    "interface",
    "type",
    "let",
    "const",
    "var",
    "val",
    "static",
    "mut",
    "ref",
    "where",
    "public",
    "private",
    "protected",
    "void",
    "int",
    "bool",
    "char",
    "float",
    "double",
    "string",
    "lambda",
    "pass",
    "then",
    "fi",
    "done",
    "esac",
    "elif",
    "end",
    "local",
    "select",
    "insert",
    "update",
    "delete",
    "create",
    "table",
    "into",
    "values",
    "unsafe",
    "extern",
    "crate",
    "dyn",
    "move",
    "go",
    "defer",
    "chan",
    "map",
    "echo",
    "unless",
    "begin",
    "module",
    "require",
];

fn style_comment() -> Style {
    Style::new().fg(theme::dim()).add_modifier(Modifier::ITALIC)
}
fn style_string() -> Style {
    Style::new().fg(theme::type_color(FileType::Spreadsheet))
}
fn style_number() -> Style {
    Style::new().fg(theme::type_color(FileType::Archive))
}
fn style_keyword() -> Style {
    Style::new()
        .fg(theme::type_color(FileType::Code))
        .add_modifier(Modifier::BOLD)
}
fn style_type() -> Style {
    Style::new().fg(theme::type_color(FileType::Folder))
}
fn style_plain() -> Style {
    Style::new().fg(theme::fg())
}

/// The state after `lines` (used to start highlighting in the middle of a file).
pub fn state_after<'a>(lang: Lang, lines: impl Iterator<Item = &'a str>) -> State {
    let mut state = State::default();
    for line in lines {
        line_spans(lang, line, &mut state);
    }
    state
}

/// One line as styled spans; `state` is updated for the next line.
pub fn line_spans(lang: Lang, line: &str, state: &mut State) -> Vec<Span<'static>> {
    match lang {
        Lang::Plain => vec![Span::styled(line.to_string(), style_plain())],
        Lang::Markdown => markdown(line, state),
        _ => code(lang, line, state),
    }
}

fn code(lang: Lang, line: &str, state: &mut State) -> Vec<Span<'static>> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<Span<'static>>| {
        if !plain.is_empty() {
            out.push(Span::styled(std::mem::take(plain), style_plain()));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        // Inside a block comment: until */.
        if state.block_comment {
            let start = i;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1).copied() == Some('/')) {
                i += 1;
            }
            if i < chars.len() {
                i += 2;
                state.block_comment = false;
            }
            out.push(Span::styled(
                chars[start..i.min(chars.len())].iter().collect::<String>(),
                style_comment(),
            ));
            continue;
        }
        let c = chars[i];
        let line_comment = match lang {
            Lang::CLike => c == '/' && chars.get(i + 1).copied() == Some('/'),
            Lang::Hash => c == '#',
            Lang::Dash => c == '-' && chars.get(i + 1).copied() == Some('-'),
            _ => false,
        };
        if line_comment {
            flush(&mut plain, &mut out);
            out.push(Span::styled(
                chars[i..].iter().collect::<String>(),
                style_comment(),
            ));
            break;
        }
        if lang == Lang::CLike && c == '/' && chars.get(i + 1).copied() == Some('*') {
            flush(&mut plain, &mut out);
            state.block_comment = true;
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            // A Rust lifetime ('a) is not a string: only take ' when it closes soon.
            let close = chars[i + 1..]
                .iter()
                .position(|&x| x == c)
                .map(|p| i + 1 + p);
            let is_string = match (c, close) {
                ('\'', Some(end)) => lang != Lang::CLike || end - i <= 3 || chars[i + 1] == '\\',
                (_, Some(_)) => true,
                (_, None) => c == '"',
            };
            if is_string {
                flush(&mut plain, &mut out);
                let mut end = i + 1;
                while end < chars.len() && chars[end] != c {
                    end += if chars[end] == '\\' { 2 } else { 1 };
                }
                let end = (end + 1).min(chars.len());
                out.push(Span::styled(
                    chars[i..end].iter().collect::<String>(),
                    style_string(),
                ));
                i = end;
                continue;
            }
        }
        if c.is_ascii_digit()
            && !chars[..i]
                .last()
                .is_some_and(|p| p.is_alphanumeric() || *p == '_')
        {
            flush(&mut plain, &mut out);
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            out.push(Span::styled(
                chars[start..i].iter().collect::<String>(),
                style_number(),
            ));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let style = if KEYWORDS.contains(&word.as_str()) {
                Some(style_keyword())
            } else if word.chars().next().is_some_and(char::is_uppercase) && word.len() > 1 {
                Some(style_type())
            } else {
                None
            };
            match style {
                Some(style) => {
                    flush(&mut plain, &mut out);
                    out.push(Span::styled(word, style));
                }
                None => plain.push_str(&word),
            }
            continue;
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, &mut out);
    out
}

fn markdown(line: &str, state: &mut State) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        state.fence = !state.fence;
        return vec![Span::styled(line.to_string(), style_comment())];
    }
    if state.fence {
        return vec![Span::styled(line.to_string(), style_string())];
    }
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|&c| c == '#').count();
        let color = if level <= 1 {
            theme::accent()
        } else {
            theme::type_color(FileType::Code)
        };
        return vec![Span::styled(
            line.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        )];
    }
    if trimmed.starts_with('>') {
        return vec![Span::styled(line.to_string(), style_comment())];
    }
    if trimmed.starts_with('|') {
        return vec![Span::styled(line.to_string(), Style::new().fg(theme::fg()))];
    }
    let mut out = Vec::new();
    let indent = line.len() - trimmed.len();
    let mut body = trimmed;
    let bullet_len = if body.starts_with("- [x] ") || body.starts_with("- [ ] ") {
        6
    } else if body.starts_with("- ") || body.starts_with("* ") || body.starts_with("+ ") {
        2
    } else {
        let digits = body.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && body[digits..].starts_with(". ") {
            digits + 2
        } else {
            0
        }
    };
    if bullet_len > 0 {
        out.push(Span::raw(line[..indent].to_string()));
        out.push(Span::styled(body[..bullet_len].to_string(), style_number()));
        body = &body[bullet_len..];
    } else if indent > 0 {
        out.push(Span::raw(line[..indent].to_string()));
    }
    out.extend(inline_markdown(body));
    out
}

/// `code`, **bold** and [links](…) inside a Markdown line.
fn inline_markdown(text: &str) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let marker = match c {
            '`' => Some(("`", style_string())),
            '*' if rest.starts_with("**") => Some((
                "**",
                Style::new().fg(theme::fg()).add_modifier(Modifier::BOLD),
            )),
            '[' => Some((
                "]",
                Style::new()
                    .fg(theme::accent())
                    .add_modifier(Modifier::UNDERLINED),
            )),
            _ => None,
        };
        if let Some((close, style)) = marker {
            let open_len = if close == "]" { 1 } else { close.len() };
            if let Some(end) = rest[open_len..].find(close) {
                if !plain.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut plain), style_plain()));
                }
                let mut stop = open_len + end + close.len();
                // A link keeps its (target) in dim text.
                if close == "]"
                    && rest[stop..].starts_with('(')
                    && let Some(paren) = rest[stop..].find(')')
                {
                    {
                        out.push(Span::styled(rest[..stop].to_string(), style));
                        out.push(Span::styled(
                            rest[stop..stop + paren + 1].to_string(),
                            style_comment(),
                        ));
                        stop += paren + 1;
                        rest = &rest[stop..];
                        continue;
                    }
                }
                out.push(Span::styled(rest[..stop].to_string(), style));
                rest = &rest[stop..];
                continue;
            }
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if !plain.is_empty() {
        out.push(Span::styled(plain, style_plain()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn code_keeps_the_text_and_marks_tokens() {
        let mut state = State::default();
        let line = r#"let x = "a // b"; // note 42"#;
        let spans = line_spans(Lang::CLike, line, &mut state);
        assert_eq!(text_of(&spans), line);
        let string = spans.iter().find(|s| s.content == "\"a // b\"").unwrap();
        assert_eq!(string.style, style_string());
        assert_eq!(spans.last().unwrap().content, "// note 42");
        assert_eq!(spans[0].content, "let");
    }

    #[test]
    fn block_comments_span_lines() {
        let mut state = State::default();
        line_spans(Lang::CLike, "a /* start", &mut state);
        assert!(state.block_comment);
        let spans = line_spans(Lang::CLike, "still */ b", &mut state);
        assert!(!state.block_comment);
        assert_eq!(spans[0].content, "still */");
        assert_eq!(text_of(&spans), "still */ b");
    }

    #[test]
    fn rust_lifetimes_are_not_strings() {
        let mut state = State::default();
        let line = "fn f<'a>(x: &'a str) -> char { 'z' }";
        let spans = line_spans(Lang::CLike, line, &mut state);
        assert_eq!(text_of(&spans), line);
        assert!(spans.iter().any(|s| s.content == "'z'"));
    }

    #[test]
    fn markdown_structure() {
        let mut state = State::default();
        assert_eq!(
            text_of(&line_spans(Lang::Markdown, "## Title", &mut state)),
            "## Title"
        );
        let spans = line_spans(Lang::Markdown, "- see [docs](a.md) and `code`", &mut state);
        assert_eq!(text_of(&spans), "- see [docs](a.md) and `code`");
        assert!(spans.iter().any(|s| s.content == "[docs]"));
        line_spans(Lang::Markdown, "```rust", &mut state);
        assert!(state.fence);
    }
}
