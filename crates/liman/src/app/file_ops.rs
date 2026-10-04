//! File operations from the user's side: marks, clipboard, rename input, trash, and running jobs.
//! The work itself happens in `liman_core::job` on a worker thread.

use std::path::PathBuf;

use liman_core::job::{Done, Job, Outcome};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::App;
use crate::worker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMode {
    Copy,
    Cut,
}

/// Paths copied or cut with Ctrl+C / Ctrl+X, waiting for Ctrl+V.
pub struct Clipboard {
    pub mode: ClipMode,
    pub paths: Vec<PathBuf>,
}

/// A job running on the worker, shown in the status bar.
pub struct JobStatus {
    pub label: String,
    pub done: u64,
    pub total: u64,
}

/// F2: the new name being typed.
pub struct RenameInput {
    pub path: PathBuf,
    pub text: String,
}

impl App {
    /// Marked entries in display order, or the selected one when nothing is marked.
    pub(super) fn targets(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            return self
                .selected_entry()
                .map(|e| e.path.clone())
                .into_iter()
                .collect();
        }
        self.visible_entries()
            .into_iter()
            .filter(|e| self.marked.contains(&e.path))
            .map(|e| e.path.clone())
            .collect()
    }

    /// Space: mark or unmark the selected entry and move down, like GUI list selection with Ctrl+click.
    pub(super) fn toggle_mark(&mut self) {
        let Some(path) = self.selected_entry().map(|e| e.path.clone()) else {
            return;
        };
        if !self.marked.remove(&path) {
            self.marked.insert(path);
        }
        self.move_selection(1);
    }

    pub(super) fn mark_all(&mut self) {
        self.marked = self
            .visible_entries()
            .iter()
            .map(|e| e.path.clone())
            .collect();
    }

    pub(super) fn copy_to_clipboard(&mut self, mode: ClipMode) {
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let verb = match mode {
            ClipMode::Copy => "copy",
            ClipMode::Cut => "move",
        };
        self.message = Some(format!(
            "{} ready to {verb}: open a folder and press Ctrl+V",
            liman_core::format::items(paths.len())
        ));
        self.clipboard = Some(Clipboard { mode, paths });
        self.marked.clear();
    }

    pub(super) fn paste(&mut self) {
        let Some(clip) = &self.clipboard else {
            self.message = Some("Nothing to paste: use Ctrl+C or Ctrl+X first".into());
            return;
        };
        let (sources, dest) = (clip.paths.clone(), self.cwd.clone());
        let n = liman_core::format::items(sources.len());
        let (job, label) = match clip.mode {
            ClipMode::Copy => (Job::Copy { sources, dest }, format!("Copying {n}")),
            ClipMode::Cut => (Job::Move { sources, dest }, format!("Moving {n}")),
        };
        if self.start_job(job, label) && clip_is_cut(&self.clipboard) {
            self.clipboard = None; // cut items can be pasted once
        }
    }

    pub(super) fn trash_targets(&mut self) {
        if self.cwd.starts_with(&self.trash_dir) {
            self.message = Some("These items are already in the trash".into());
            return;
        }
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let label = format!(
            "Moving {} to the trash",
            liman_core::format::items(paths.len())
        );
        self.start_job(Job::Trash { paths }, label);
    }

    pub(super) fn begin_rename(&mut self) {
        if let Some(entry) = self.selected_entry() {
            self.rename = Some(RenameInput {
                path: entry.path.clone(),
                text: entry.name.clone(),
            });
        }
    }

    pub(super) fn on_rename_key(&mut self, key: KeyEvent) {
        let Some(input) = &mut self.rename else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.rename = None,
            KeyCode::Enter => {
                let RenameInput { path, text } = self.rename.take().expect("checked above");
                let label = format!("Renaming to “{text}”");
                self.start_job(
                    Job::Rename {
                        path,
                        new_name: text,
                    },
                    label,
                );
            }
            KeyCode::Backspace => {
                input.text.pop();
            }
            KeyCode::Char(c) => input.text.push(c),
            _ => {}
        }
    }

    /// Ctrl+Z: reverses the newest finished operation.
    pub(super) fn undo(&mut self) {
        let Some(done) = self.history.pop() else {
            self.message = Some("Nothing to undo".into());
            return;
        };
        let label = format!("Undoing: {}", done.describe());
        if !self.start_job(Job::Undo(done.clone()), label) {
            self.history.push(done); // a job is running; keep it for later
        }
    }

    /// Starts `job` on a worker unless one is already running. Returns whether it started.
    fn start_job(&mut self, job: Job, label: String) -> bool {
        if let Some(running) = &self.job {
            self.message = Some(format!("Please wait: {} is still running", running.label));
            return false;
        }
        self.job = Some(JobStatus {
            label,
            done: 0,
            total: job.total(),
        });
        worker::spawn_job(self.tx.clone(), job, self.trash_dir.clone());
        true
    }

    pub(super) fn on_job_progress(&mut self, done: u64) {
        if let Some(job) = &mut self.job {
            job.done = done;
            self.dirty = true;
        }
    }

    pub(super) fn on_job_finished(&mut self, outcome: Outcome) {
        self.job = None;
        self.message = Some(match &outcome.error {
            Some(err) if outcome.done.is_empty() => format!("Failed: {err}"),
            Some(err) => format!("{}, then failed: {err}", outcome.done.describe()),
            None => outcome.done.describe(),
        });
        // Select the result (new name, first copy) if it is in this folder, else keep the selection.
        let focus = outcome
            .done
            .focus()
            .filter(|p| p.parent() == Some(self.cwd.as_path()))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        self.select_after_load = focus.or_else(|| self.selected_entry().map(|e| e.name.clone()));
        let undoable = !outcome.done.is_empty() && !matches!(outcome.done, Done::Undone(_));
        if undoable {
            self.history.push(outcome.done);
        }
        self.load(self.cwd.clone());
    }
}

fn clip_is_cut(clipboard: &Option<Clipboard>) -> bool {
    clipboard.as_ref().is_some_and(|c| c.mode == ClipMode::Cut)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Listing;
    use crate::event::AppEvent;
    use liman_core::Places;
    use ratatui::crossterm::event::{Event, KeyEventKind, KeyEventState, KeyModifiers};
    use std::fs;
    use std::path::Path;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> AppEvent {
        AppEvent::Input(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }))
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle(key(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle(key(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    /// Feeds worker events to the app until `done` holds (listing loaded, job finished...).
    fn pump(app: &mut App, rx: &Receiver<AppEvent>, done: impl Fn(&App) -> bool) {
        while !done(app) {
            let ev = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("worker event");
            app.handle(ev);
        }
    }

    fn loaded(app: &App) -> bool {
        matches!(app.listing, Listing::Ready(_)) && app.job.is_none()
    }

    /// Temp home with a.txt, b.txt and dest/; the app shows it with a private trash.
    fn setup(tag: &str) -> (PathBuf, App, Receiver<AppEvent>) {
        let dir = std::env::temp_dir().join(format!("liman-app-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("dest")).unwrap();
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("b.txt"), "b").unwrap();
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(dir.clone(), Places::from_user_dirs("", &dir), tx);
        app.trash_dir = dir.join(".trash");
        pump(&mut app, &rx, loaded);
        (dir, app, rx)
    }

    fn select(app: &mut App, name: &str) {
        let row = app
            .visible_entries()
            .iter()
            .position(|e| e.name == name)
            .unwrap();
        app.table.select(Some(row));
    }

    #[test]
    fn copy_and_paste_into_another_folder() {
        let (dir, mut app, rx) = setup("paste");
        select(&mut app, "a.txt");
        ctrl(&mut app, 'c');
        assert!(app.running, "Ctrl+C copies, it does not quit");
        app.load(dir.join("dest"));
        pump(&mut app, &rx, loaded);
        ctrl(&mut app, 'v');
        pump(&mut app, &rx, loaded);
        assert!(dir.join("dest/a.txt").exists());
        assert!(dir.join("a.txt").exists());
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.selected_entry().unwrap().name, "a.txt");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn marked_items_are_cut_and_moved_once() {
        let (dir, mut app, rx) = setup("cut");
        select(&mut app, "a.txt");
        press(&mut app, KeyCode::Char(' ')); // mark a.txt, cursor moves to b.txt
        press(&mut app, KeyCode::Char(' ')); // mark b.txt
        assert_eq!(app.marked.len(), 2);
        ctrl(&mut app, 'x');
        assert!(app.marked.is_empty());
        app.load(dir.join("dest"));
        pump(&mut app, &rx, loaded);
        ctrl(&mut app, 'v');
        pump(&mut app, &rx, loaded);
        assert!(dir.join("dest/a.txt").exists() && dir.join("dest/b.txt").exists());
        assert!(!dir.join("a.txt").exists());
        assert!(app.clipboard.is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn delete_moves_to_trash_and_f2_renames() {
        let (dir, mut app, rx) = setup("trash");
        select(&mut app, "a.txt");
        press(&mut app, KeyCode::Delete);
        pump(&mut app, &rx, loaded);
        assert!(!dir.join("a.txt").exists());
        assert!(dir.join(".trash/files/a.txt").exists());

        select(&mut app, "b.txt");
        press(&mut app, KeyCode::F(2));
        for _ in 0..5 {
            press(&mut app, KeyCode::Backspace); // "b.txt" -> ""
        }
        for c in "c.md".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        pump(&mut app, &rx, loaded);
        assert!(dir.join("c.md").exists());
        assert_eq!(app.selected_entry().unwrap().name, "c.md");
        assert_eq!(app.history.len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_to_an_existing_name_fails_without_changes() {
        let (dir, mut app, rx) = setup("rename-clash");
        select(&mut app, "a.txt");
        press(&mut app, KeyCode::F(2));
        app.rename.as_mut().unwrap().text = "b.txt".into();
        press(&mut app, KeyCode::Enter);
        pump(&mut app, &rx, loaded);
        assert!(app.message.as_deref().unwrap().starts_with("Failed"));
        assert!(dir.join("a.txt").exists());
        assert!(app.history.is_empty());
        let _ = Path::new("");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ctrl_z_undoes_trash_then_rename() {
        let (dir, mut app, rx) = setup("undo");
        select(&mut app, "a.txt");
        press(&mut app, KeyCode::F(2));
        app.rename.as_mut().unwrap().text = "renamed.txt".into();
        press(&mut app, KeyCode::Enter);
        pump(&mut app, &rx, loaded);
        select(&mut app, "b.txt");
        press(&mut app, KeyCode::Delete);
        pump(&mut app, &rx, loaded);
        assert!(!dir.join("b.txt").exists());

        ctrl(&mut app, 'z');
        pump(&mut app, &rx, loaded);
        assert!(dir.join("b.txt").exists());
        assert_eq!(app.selected_entry().unwrap().name, "b.txt");
        assert!(app.message.as_deref().unwrap().starts_with("Undone"));

        ctrl(&mut app, 'z');
        pump(&mut app, &rx, loaded);
        assert!(dir.join("a.txt").exists() && !dir.join("renamed.txt").exists());
        assert!(app.history.is_empty());

        ctrl(&mut app, 'z');
        assert_eq!(app.message.as_deref(), Some("Nothing to undo"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paste_without_clipboard_explains() {
        let (dir, mut app, _rx) = setup("empty-clip");
        ctrl(&mut app, 'v');
        assert!(app.message.as_deref().unwrap().contains("Ctrl+C"));
        assert!(app.job.is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
