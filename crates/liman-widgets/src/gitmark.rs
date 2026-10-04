//! Git status next to file names: two letters like `git status --short` (staged in green,
//! unstaged in red), `?` untracked, `!` conflict, `•` a folder with changes inside.

use liman_core::FileType;
use liman_core::git::GitMark;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::theme;

/// Three cells: the two letters and a space. Clean files get three spaces so names stay aligned.
pub fn spans(mark: Option<GitMark>) -> Vec<Span<'static>> {
    let Some(mark) = mark else {
        return vec![Span::raw("   ")];
    };
    let staged = theme::type_color(FileType::Spreadsheet); // green in every theme
    let unstaged = theme::type_color(FileType::Pdf); // red
    let style = |c: char, color| {
        let shown = if c == '.' { ' ' } else { c };
        Span::styled(
            shown.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        )
    };
    match mark {
        GitMark::UNTRACKED => vec![
            style('?', theme::type_color(FileType::Archive)),
            Span::raw("  "),
        ],
        GitMark::CONFLICT => vec![style('!', unstaged), style('!', unstaged), Span::raw(" ")],
        GitMark::DIRTY_FOLDER => vec![Span::raw(" "), style('•', unstaged), Span::raw(" ")],
        m => vec![
            style(m.index, staged),
            style(m.worktree, unstaged),
            Span::raw(" "),
        ],
    }
}
