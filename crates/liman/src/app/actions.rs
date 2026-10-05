//! Every user action in one list. The command palette (Ctrl+P), the right-click menu and the key
//! overview use it, so a new action shows up everywhere at once.

use liman_core::i18n::{tr, trf};
use std::io::Write;

use liman_core::job::Job;

use super::{App, Listing, TermMode, View};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    PathsToTerminal,
    OpenTerminalHere,
    Copy,
    Cut,
    Paste,
    Rename,
    NewFolder,
    CopyPath,
    Bookmark,
    Trash,
    DeleteForGood,
    MarkAll,
    Undo,
    Search,
    Filter,
    ToggleHidden,
    SortNext,
    SortReverse,
    SmallLarge,
    ZoomIn,
    ZoomOut,
    Back,
    Forward,
    Up,
    Home,
    Recent,
    NewTab,
    CloseTab,
    NextTab,
    Preview,
    Read,
    TerminalPanel,
    TerminalFullScreen,
    Theme,
    Keys,
    Quit,
    GitPanel,
    GitStage,
    GitUnstage,
    GitDiscard,
    GitCommit,
    GitPush,
    GitPull,
    GitBranch,
}

impl Action {
    pub const ALL: [Action; 45] = [
        Self::GitPanel,
        Self::GitStage,
        Self::GitUnstage,
        Self::GitDiscard,
        Self::GitCommit,
        Self::GitPush,
        Self::GitPull,
        Self::GitBranch,
        Self::Open,
        Self::PathsToTerminal,
        Self::OpenTerminalHere,
        Self::Copy,
        Self::Cut,
        Self::Paste,
        Self::Rename,
        Self::NewFolder,
        Self::CopyPath,
        Self::Bookmark,
        Self::Trash,
        Self::DeleteForGood,
        Self::MarkAll,
        Self::Undo,
        Self::Search,
        Self::Filter,
        Self::ToggleHidden,
        Self::SortNext,
        Self::SortReverse,
        Self::SmallLarge,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::Back,
        Self::Forward,
        Self::Up,
        Self::Home,
        Self::Recent,
        Self::NewTab,
        Self::CloseTab,
        Self::NextTab,
        Self::Preview,
        Self::Read,
        Self::TerminalPanel,
        Self::TerminalFullScreen,
        Self::Theme,
        Self::Keys,
        Self::Quit,
    ];

    /// Right-click on an entry.
    pub const ON_ENTRY: [Action; 13] = [
        Self::Open,
        Self::PathsToTerminal,
        Self::Copy,
        Self::Cut,
        Self::Rename,
        Self::CopyPath,
        Self::Bookmark,
        Self::Trash,
        Self::DeleteForGood,
        Self::Paste,
        Self::GitStage,
        Self::GitUnstage,
        Self::GitDiscard,
    ];

    /// Right-click on empty space in the list.
    pub const ON_FOLDER: [Action; 7] = [
        Self::Paste,
        Self::NewFolder,
        Self::OpenTerminalHere,
        Self::ToggleHidden,
        Self::SortNext,
        Self::SmallLarge,
        Self::Bookmark,
    ];

    /// The label in the interface language (palette, menu, help).
    pub fn label(self) -> &'static str {
        tr(self.label_en())
    }

    pub const fn label_en(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::PathsToTerminal => "Put path in terminal",
            Self::OpenTerminalHere => "Open terminal here",
            Self::Copy => "Copy",
            Self::Cut => "Cut",
            Self::Paste => "Paste",
            Self::Rename => "Rename",
            Self::NewFolder => "New folder",
            Self::CopyPath => "Copy path to clipboard",
            Self::Bookmark => "Bookmark (toggle)",
            Self::Trash => "Move to trash",
            Self::DeleteForGood => "Delete for good",
            Self::MarkAll => "Mark all",
            Self::Undo => "Undo",
            Self::Search => "Search in subfolders",
            Self::Filter => "Filter this folder",
            Self::ToggleHidden => "Show / hide hidden files",
            Self::SortNext => "Sort by next column",
            Self::SortReverse => "Reverse sort order",
            Self::SmallLarge => "Small list / large view",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Up => "Parent folder",
            Self::Home => "Home",
            Self::Recent => "Recent files",
            Self::NewTab => "New tab",
            Self::CloseTab => "Close tab",
            Self::NextTab => "Next tab",
            Self::Preview => "Preview panel",
            Self::Read => "Read file",
            Self::TerminalPanel => "Terminal panel",
            Self::TerminalFullScreen => "Terminal full screen",
            Self::Theme => "Theme…",
            Self::Keys => "All keys",
            Self::Quit => "Quit",
            Self::GitPanel => "Git: panel (changes, diff)",
            Self::GitStage => "Git: stage",
            Self::GitUnstage => "Git: unstage",
            Self::GitDiscard => "Git: discard changes",
            Self::GitCommit => "Git: commit",
            Self::GitPush => "Git: push",
            Self::GitPull => "Git: pull",
            Self::GitBranch => "Git: switch branch",
        }
    }

    pub const fn keys(self) -> &'static str {
        match self {
            Self::Open => "Enter",
            Self::PathsToTerminal => "Alt+Enter",
            Self::OpenTerminalHere => "F4",
            Self::Copy => "Ctrl+C",
            Self::Cut => "Ctrl+X",
            Self::Paste => "Ctrl+V",
            Self::Rename => "F2",
            Self::NewFolder => "Ctrl+N",
            Self::CopyPath => "Alt+C",
            Self::Bookmark => "Ctrl+D",
            Self::Trash => "Del",
            Self::DeleteForGood => "Shift+Del",
            Self::MarkAll => "Ctrl+A",
            Self::Undo => "Ctrl+Z",
            Self::Search => "Ctrl+F",
            Self::Filter => "/",
            Self::ToggleHidden => "Ctrl+H",
            Self::SortNext => "s",
            Self::SortReverse => "S",
            Self::SmallLarge => "v",
            Self::ZoomIn => "+",
            Self::ZoomOut => "-",
            Self::Back => "Alt+←",
            Self::Forward => "Alt+→",
            Self::Up => "Bksp",
            Self::Home => "~",
            Self::Recent => "",
            Self::NewTab => "Ctrl+T",
            Self::CloseTab => "Ctrl+W",
            Self::NextTab => "Ctrl+PgDn",
            Self::Preview => "F3",
            Self::Read => "r",
            Self::TerminalPanel => "F4",
            Self::TerminalFullScreen => "Ctrl+O",
            Self::Theme => "t",
            Self::Keys => "?",
            Self::Quit => "q",
            Self::GitPanel => "Ctrl+G",
            Self::GitStage | Self::GitUnstage => "Ctrl+G Space",
            Self::GitDiscard => "Ctrl+G d",
            Self::GitCommit => "Ctrl+G c",
            Self::GitPush => "Ctrl+G p",
            Self::GitPull => "Ctrl+G P",
            Self::GitBranch => "Ctrl+G b",
        }
    }
}

/// Actions whose label contains every word of `query` (case-insensitive), in list order.
pub fn matching(query: &str) -> Vec<Action> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    Action::ALL
        .into_iter()
        .filter(|a| {
            // Both languages match: "kopyala" and "copy" find the same action.
            let label = format!("{} {}", a.label(), a.label_en()).to_lowercase();
            words.iter().all(|w| label.contains(w))
        })
        .collect()
}

impl App {
    pub(super) fn run_action(&mut self, action: Action) {
        use liman_core::sort::SortOrder;
        match action {
            Action::Open => self.activate_selected(),
            Action::PathsToTerminal => self.paths_to_terminal(),
            Action::OpenTerminalHere => {
                if self.term_mode == TermMode::Hidden {
                    self.toggle_panel();
                } else {
                    self.focus = super::Focus::Terminal;
                }
            }
            Action::Copy => self.copy_to_clipboard(super::ClipMode::Copy),
            Action::Cut => self.copy_to_clipboard(super::ClipMode::Cut),
            Action::Paste => self.paste(),
            Action::Rename => self.begin_rename(),
            Action::NewFolder => self.new_folder(),
            Action::CopyPath => self.copy_paths_osc52(),
            Action::Bookmark => self.toggle_bookmark(),
            Action::Trash => self.trash_targets(),
            Action::DeleteForGood => self.ask_delete(),
            Action::MarkAll => self.mark_all(),
            Action::Undo => self.undo(),
            Action::Search => self.search_input = Some(String::new()),
            Action::Filter => self.filter_editing = true,
            Action::ToggleHidden => self.toggle_hidden(),
            Action::SortNext => {
                let order = SortOrder {
                    key: self.sort.key.next(),
                    descending: false,
                };
                self.set_sort(order);
            }
            Action::SortReverse => {
                let order = SortOrder {
                    descending: !self.sort.descending,
                    ..self.sort
                };
                self.set_sort(order);
            }
            Action::SmallLarge => self.toggle_compact(),
            Action::ZoomIn => self.zoom(1),
            Action::ZoomOut => self.zoom(-1),
            Action::Back => self.go_back(),
            Action::Forward => self.go_forward(),
            Action::Up => self.go_up(),
            Action::Home => self.load(self.places.home.clone()),
            Action::Recent => self.show_recent(),
            Action::NewTab => self.new_tab(),
            Action::CloseTab => self.close_tab(),
            Action::NextTab => self.cycle_tab(1),
            Action::Preview => self.toggle_preview(),
            Action::Read => self.open_reader(),
            Action::TerminalPanel => self.toggle_panel(),
            Action::TerminalFullScreen => self.toggle_fullscreen(),
            Action::Theme => self.open_theme_picker(),
            Action::Keys => self.help_open = true,
            Action::Quit => self.running = false,
            Action::GitPanel => self.toggle_git_panel(),
            Action::GitStage => self.git_stage(),
            Action::GitUnstage => self.git_unstage(),
            Action::GitDiscard => self.ask_git_discard(),
            Action::GitCommit => self.begin_git_commit(),
            Action::GitPush => self.git_push(),
            Action::GitPull => self.git_pull(),
            Action::GitBranch => self.open_branch_picker(),
        }
    }

    /// Ctrl+N: a new folder here; it is selected and its name opens for editing.
    pub(super) fn new_folder(&mut self) {
        if self.results.is_some() || !matches!(self.listing, Listing::Ready(_)) {
            return;
        }
        let job = Job::CreateDir {
            parent: self.cwd.clone(),
            name: tr("New folder").into(),
        };
        if self.start_job(job, tr("Creating a folder").into()) {
            self.rename_after_load = true;
        }
    }

    /// Alt+C: the marked (or selected) paths to the clipboard with OSC 52, which the terminal
    /// handles — so it also works over SSH, into the clipboard of the machine you sit at.
    pub(super) fn copy_paths_osc52(&mut self) {
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let text = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
        let _ = out.flush();
        self.message = Some(match paths.as_slice() {
            [one] => trf("Copied path {}", &[&one.display()]),
            many => trf("Copied {} paths", &[&many.len()]),
        });
    }

    /// Whether `action` makes sense right now (greyed out in menus otherwise).
    pub fn action_available(&self, action: Action) -> bool {
        let has_entry = self.selected_entry().is_some();
        match action {
            Action::Open
            | Action::Copy
            | Action::Cut
            | Action::Rename
            | Action::Trash
            | Action::DeleteForGood
            | Action::CopyPath
            | Action::PathsToTerminal => has_entry,
            Action::Paste => self.clipboard.is_some(),
            Action::Undo => !self.history.is_empty(),
            Action::Back => !self.back_stack.is_empty(),
            Action::Forward => !self.forward_stack.is_empty(),
            Action::NewFolder => self.results.is_none(),
            Action::GitStage | Action::GitUnstage | Action::GitDiscard => {
                self.git.is_some() && has_entry
            }
            Action::GitPanel
            | Action::GitCommit
            | Action::GitPush
            | Action::GitPull
            | Action::GitBranch => self.git.is_some(),
            Action::ZoomIn => {
                self.view != View::Grid
                    || self.drawn_grid_level + 1 < liman_widgets::grid::BOX_SIZES.len()
            }
            _ => true,
        }
    }
}

/// Standard base64 (RFC 4648) for OSC 52.
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What the palette or a right-click menu is showing.
pub struct Menu {
    pub items: Vec<Action>,
    pub selected: usize,
    /// Palette: the search text; right-click menu: `None`.
    pub query: Option<String>,
    /// Right-click menu: top-left corner on screen.
    pub at: (u16, u16),
    /// First item shown (long palette lists scroll). Written by the UI.
    pub offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_turkish_label() {
        for action in Action::ALL {
            assert!(
                liman_core::i18n::turkish(action.label_en()).is_some(),
                "{:?}",
                action
            );
        }
    }

    #[test]
    fn palette_finds_actions_in_both_languages() {
        // label_en is always searched; the Turkish label only when Turkish is on (global),
        // so check the English path here and the table above.
        assert_eq!(matching("trash")[0], Action::Trash);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("/home/şş".as_bytes()), "L2hvbWUvxZ/Fnw==");
    }

    #[test]
    fn palette_matches_every_word() {
        assert_eq!(matching("hidden"), [Action::ToggleHidden]);
        assert_eq!(matching("term full"), [Action::TerminalFullScreen]);
        assert_eq!(matching("").len(), Action::ALL.len());
    }
}
