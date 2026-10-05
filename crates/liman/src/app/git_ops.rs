//! Git from the user's side: stage / unstage / discard / commit / push / pull / switch branch,
//! and the git panel (`g`) with the changed files and a diff of the selected one.
//! Every command runs through `git` on a worker (ADR 0007).

use liman_core::i18n::{tr, trf};
use std::path::PathBuf;

use liman_core::git::GitMark;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Dialog};
use crate::worker;

/// The git panel: changed files, the selected one's diff.
pub struct GitPanel {
    pub selected: usize,
    /// Diff of the selected file (colored by the UI: `+` green, `-` red).
    pub diff: Vec<String>,
    /// Which file the diff belongs to.
    pub diff_for: Option<PathBuf>,
    pub scroll: usize,
    /// A diff is being built on a worker (one at a time; the newest selection wins).
    diff_busy: bool,
    /// Where `p` pushes: remote name and URL (read on a worker when the panel opens).
    pub remote: Option<(String, String)>,
    /// False until the remote lookup has answered.
    pub remote_known: bool,
}

impl App {
    fn git_root(&self) -> Option<PathBuf> {
        self.git.as_ref().map(|g| g.root.clone())
    }

    fn git_command(&mut self, args: Vec<String>, label: &str) {
        let Some(root) = self.git_root() else {
            self.message = Some(tr("Not inside a git repository").into());
            return;
        };
        self.message = Some(format!("{label}…"));
        worker::spawn_git_command(self.tx.clone(), root, args, label.to_string());
    }

    /// Paths the git action works on: the git panel's file, else marked / selected entries.
    fn git_targets(&self) -> Vec<PathBuf> {
        if let (Some(panel), Some(git)) = (&self.git_panel, &self.git) {
            return git
                .files
                .get(panel.selected)
                .map(|(p, _)| p.clone())
                .into_iter()
                .collect();
        }
        self.targets()
    }

    fn with_paths(base: &[&str], paths: &[PathBuf]) -> Vec<String> {
        let mut args: Vec<String> = base.iter().map(|s| (*s).to_string()).collect();
        args.push("--".into());
        args.extend(paths.iter().map(|p| p.display().to_string()));
        args
    }

    pub(super) fn git_stage(&mut self) {
        let paths = self.git_targets();
        if !paths.is_empty() {
            self.git_command(Self::with_paths(&["add", "-A"], &paths), tr("Staged"));
        }
    }

    pub(super) fn git_unstage(&mut self) {
        let paths = self.git_targets();
        if !paths.is_empty() {
            self.git_command(
                Self::with_paths(&["restore", "--staged"], &paths),
                tr("Unstaged"),
            );
        }
    }

    /// Space in the git panel: stage what is unstaged, unstage what is only staged.
    fn git_toggle_stage(&mut self) {
        let mark = self.git_panel_file().map(|(_, m)| m);
        match mark {
            Some(m) if m.is_unstaged() || m == GitMark::UNTRACKED => self.git_stage(),
            Some(_) => self.git_unstage(),
            None => {}
        }
    }

    pub(super) fn ask_git_discard(&mut self) {
        let paths = self.git_targets();
        if !paths.is_empty() {
            self.dialog = Some(Dialog::ConfirmDiscard { paths });
        }
    }

    /// After confirmation: tracked files go back to the last commit; untracked files go to the
    /// trash (not deleted, so Ctrl+Z can bring them back).
    pub(super) fn git_discard(&mut self, paths: Vec<PathBuf>) {
        let (untracked, tracked): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| {
            self.git.as_ref().and_then(|g| g.marks.get(p)).copied() == Some(GitMark::UNTRACKED)
        });
        if !tracked.is_empty() {
            let args = Self::with_paths(
                &["restore", "--source=HEAD", "--staged", "--worktree"],
                &tracked,
            );
            self.git_command(args, tr("Discarded changes"));
        }
        if !untracked.is_empty() {
            let label = trf(
                "Moving {} to the trash",
                &[&liman_core::format::items(untracked.len())],
            );
            self.start_job(liman_core::job::Job::Trash { paths: untracked }, label);
        }
    }

    pub(super) fn begin_git_commit(&mut self) {
        let staged = self
            .git
            .as_ref()
            .is_some_and(|g| g.files.iter().any(|(_, m)| m.is_staged()));
        if !staged {
            self.message =
                Some(tr("Nothing staged: stage files first (Space in the git panel)").into());
            return;
        }
        self.commit_input = Some(String::new());
    }

    pub(super) fn on_commit_key(&mut self, key: KeyEvent) {
        let Some(text) = &mut self.commit_input else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.commit_input = None,
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) if super::is_typing(key) => text.push(c),
            KeyCode::Enter => {
                let message = self.commit_input.take().unwrap_or_default();
                if !message.trim().is_empty() {
                    self.git_command(vec!["commit".into(), "-m".into(), message], tr("Committed"));
                }
            }
            _ => {}
        }
    }

    /// `p`: pushes the current branch's commits (not files: what is committed) to the remote.
    /// A branch without an upstream is pushed with `-u`, so later pushes know where to go.
    pub(super) fn git_push(&mut self) {
        let Some(git) = &self.git else {
            return;
        };
        if git.upstream.is_some() {
            return self.git_command(vec!["push".into()], tr("Pushed"));
        }
        // No upstream yet: find the remote and push with -u, both on the worker.
        let root = git.root.clone();
        let no_remote = tr("No remote yet: git remote add origin <URL> in the terminal");
        self.message = Some(format!("{}…", tr("Pushed")));
        worker::spawn_git_task(self.tx.clone(), tr("Pushed").into(), move || {
            let (remote, _) = liman_core::git::push_remote(&root, None).ok_or(no_remote)?;
            liman_core::git::run(&root, &["push", "-u", &remote, "HEAD"])
        });
    }

    pub(super) fn git_pull(&mut self) {
        self.git_command(vec!["pull".into(), "--ff-only".into()], tr("Pulled"));
    }

    /// `b` (git panel): local branches in a list; Enter switches.
    pub(super) fn open_branch_picker(&mut self) {
        if let Some(root) = self.git_root() {
            worker::spawn_git_branches(self.tx.clone(), root);
        }
    }

    pub(super) fn on_git_branches(&mut self, result: Result<Vec<String>, String>) {
        match result {
            Ok(branches) => {
                let current = self.git.as_ref().map(|g| g.branch.as_str());
                let selected = branches
                    .iter()
                    .position(|b| Some(b.as_str()) == current)
                    .unwrap_or(0);
                self.branch_picker = Some((branches, selected));
            }
            Err(e) => self.message = Some(first_line(&e)),
        }
        self.dirty = true;
    }

    pub(super) fn on_branch_key(&mut self, key: KeyEvent) {
        let Some((branches, selected)) = &mut self.branch_picker else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.branch_picker = None,
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(branches.len().saturating_sub(1)),
            KeyCode::Enter => {
                let branch = branches.get(*selected).cloned();
                self.branch_picker = None;
                if let Some(branch) = branch {
                    self.git_command(vec!["switch".into(), branch], tr("Switched branch"));
                }
            }
            _ => {}
        }
    }

    /// A git command finished: report, then read status and the folder again.
    pub(super) fn on_git_done(&mut self, label: String, result: Result<String, String>) {
        self.message = Some(match result {
            Ok(_) => label,
            Err(e) => format!("git: {}", first_line(&e)),
        });
        self.request_git();
        if self.results.is_none() {
            self.refresh();
        }
        if let Some(panel) = &mut self.git_panel {
            panel.diff_for = None; // reload the diff for the (maybe changed) file
        }
    }

    // ---- git panel ----

    pub(super) fn toggle_git_panel(&mut self) {
        if self.git_panel.is_some() {
            self.git_panel = None;
        } else if let Some(git) = &self.git {
            worker::spawn_git_remote(self.tx.clone(), git.root.clone(), git.upstream.clone());
            self.git_panel = Some(GitPanel {
                selected: 0,
                diff: Vec::new(),
                diff_for: None,
                scroll: 0,
                diff_busy: false,
                remote: None,
                remote_known: false,
            });
            self.request_git();
        } else {
            self.message = Some(tr("Not inside a git repository").into());
        }
    }

    fn git_panel_file(&self) -> Option<(PathBuf, GitMark)> {
        let panel = self.git_panel.as_ref()?;
        self.git.as_ref()?.files.get(panel.selected).cloned()
    }

    pub(super) fn on_git_panel_key(&mut self, key: KeyEvent) {
        let count = self.git.as_ref().map_or(0, |g| g.files.len());
        let Some(panel) = &mut self.git_panel else {
            return;
        };
        // Only Ctrl+G means something with a modifier; Ctrl+D must not discard.
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            if key.code == KeyCode::Char('g') {
                self.git_panel = None;
            }
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.git_panel = None,
            KeyCode::Up | KeyCode::Char('k') => {
                panel.selected = panel.selected.saturating_sub(1);
                panel.scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                panel.selected = (panel.selected + 1).min(count.saturating_sub(1));
                panel.scroll = 0;
            }
            KeyCode::PageDown | KeyCode::Char('J') => panel.scroll += 10,
            KeyCode::PageUp | KeyCode::Char('K') => panel.scroll = panel.scroll.saturating_sub(10),
            KeyCode::Char(' ') => self.git_toggle_stage(),
            KeyCode::Char('a') => {
                self.git_command(vec!["add".into(), "-A".into()], tr("Staged everything"));
            }
            KeyCode::Char('d') => self.ask_git_discard(),
            KeyCode::Char('c') => self.begin_git_commit(),
            KeyCode::Char('p') => self.git_push(),
            KeyCode::Char('P') => self.git_pull(),
            KeyCode::Char('b') => self.open_branch_picker(),
            KeyCode::Enter => {
                // Show the file in the view and close the panel.
                if let Some((path, _)) = self.git_panel_file()
                    && let Some(dir) = path.parent()
                {
                    self.select_after_load =
                        path.file_name().map(|n| n.to_string_lossy().into_owned());
                    self.git_panel = None;
                    self.load(dir.to_path_buf());
                }
            }
            _ => {}
        }
    }

    /// Called every frame the panel is open: asks a worker for the diff when the selected file
    /// changed. One diff at a time; when it arrives for a file no longer selected, the next frame
    /// asks for the current one.
    pub fn git_panel_diff(&mut self) {
        let Some((path, mark)) = self.git_panel_file() else {
            return;
        };
        let Some(root) = self.git_root() else {
            return;
        };
        let Some(panel) = &mut self.git_panel else {
            return;
        };
        if panel.diff_for.as_ref() == Some(&path) {
            return;
        }
        panel.diff_for = Some(path.clone());
        if !panel.diff_busy {
            panel.diff_busy = true;
            worker::spawn_git_diff(self.tx.clone(), root, path, mark);
        }
    }

    pub(super) fn on_git_diff(&mut self, path: PathBuf, lines: Vec<String>) {
        let Some(panel) = &mut self.git_panel else {
            return;
        };
        panel.diff_busy = false;
        if panel.diff_for.as_ref() == Some(&path) {
            panel.diff = lines;
        } else {
            panel.diff_for = None; // the selection moved on: ask again on the next frame
        }
        self.dirty = true;
    }

    pub(super) fn on_git_remote(
        &mut self,
        root: &std::path::Path,
        remote: Option<(String, String)>,
    ) {
        let same_repo = self.git.as_ref().is_some_and(|g| g.root == root);
        if let (Some(panel), true) = (&mut self.git_panel, same_repo) {
            panel.remote = remote;
            panel.remote_known = true;
            self.dirty = true;
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed")
        .to_string()
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Listing};
    use crate::event::AppEvent;
    use liman_core::Places;
    use liman_core::git::run;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use std::fs;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.handle(AppEvent::Input(Event::Key(KeyEvent::new(code, modifiers))));
    }

    fn pump(app: &mut App, rx: &Receiver<AppEvent>, done: impl Fn(&App) -> bool) {
        while !done(app) {
            let ev = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("worker event");
            app.handle(ev);
        }
    }

    #[test]
    fn stage_and_commit_from_the_git_panel() {
        if run(std::path::Path::new("/"), &["--version"]).is_err() {
            return; // no git here
        }
        let dir = std::env::temp_dir().join(format!("liman-gitui-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        run(&dir, &["config", "user.email", "t@example.com"]).unwrap();
        run(&dir, &["config", "user.name", "T"]).unwrap();
        fs::write(dir.join("a.txt"), "hello").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        pump(&mut app, &rx, |a| {
            matches!(a.listing, Listing::Ready(_))
                && a.git.as_ref().is_some_and(|g| !g.files.is_empty())
        });
        assert_eq!(app.git.as_ref().unwrap().branch, "main");

        press(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert!(app.git_panel.is_some());
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE); // stage a.txt
        pump(&mut app, &rx, |a| {
            a.git
                .as_ref()
                .is_some_and(|g| g.files.iter().any(|(_, m)| m.is_staged()))
        });
        press(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
        for c in "first".chars() {
            press(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        pump(&mut app, &rx, |a| {
            a.message.as_deref() == Some("Committed")
                && a.git.as_ref().is_some_and(|g| g.files.is_empty())
        });
        assert!(run(&dir, &["log", "--oneline"]).unwrap().contains("first"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
