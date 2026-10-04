//! File operations from the user's side: marks, clipboard, rename input, trash, and running jobs.
//! The work itself happens in `liman_core::job` on a worker thread.

use std::path::PathBuf;

use liman_core::job::{Job, Outcome};
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

/// How to paste over names that already exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Keep the old file, the pasted one becomes `name (2)`.
    KeepBoth,
    /// Move the old file to the trash, paste the new one.
    Replace,
    /// Paste only what does not exist yet.
    Skip,
}

/// A question shown in the middle of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    Conflict { existing: Vec<PathBuf> },
    ConfirmDelete { paths: Vec<PathBuf> },
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

    /// Ctrl+V. If names already exist here, asks first (keep both / replace / skip).
    pub(super) fn paste(&mut self) {
        let Some(clip) = &self.clipboard else {
            self.message = Some("Nothing to paste: use Ctrl+C or Ctrl+X first".into());
            return;
        };
        let dest = self.cwd.clone();
        let conflicts: Vec<PathBuf> = clip
            .paths
            .iter()
            .filter(|src| src.parent() != Some(dest.as_path())) // pasting into the same folder: copies get "(2)"
            .filter_map(|src| src.file_name().map(|n| dest.join(n)))
            .filter(|target| target.symlink_metadata().is_ok())
            .collect();
        if conflicts.is_empty() {
            self.paste_with(Conflict::KeepBoth);
        } else {
            self.dialog = Some(Dialog::Conflict {
                existing: conflicts,
            });
        }
    }

    /// Runs the paste with the chosen answer to name conflicts.
    pub(super) fn paste_with(&mut self, answer: Conflict) {
        let Some(clip) = &self.clipboard else {
            return;
        };
        let dest = self.cwd.clone();
        let existing = |src: &PathBuf| {
            src.file_name()
                .map(|n| dest.join(n))
                .filter(|t| src.parent() != Some(dest.as_path()) && t.symlink_metadata().is_ok())
        };
        let mut sources = clip.paths.clone();
        let mut replaced = Vec::new();
        match answer {
            Conflict::KeepBoth => {}
            Conflict::Skip => sources.retain(|s| existing(s).is_none()),
            Conflict::Replace => replaced = sources.iter().filter_map(existing).collect(),
        }
        if sources.is_empty() {
            self.message = Some("Nothing to paste: every item already exists here".into());
            return;
        }
        let n = liman_core::format::items(sources.len());
        let (paste, label) = match clip.mode {
            ClipMode::Copy => (Job::Copy { sources, dest }, format!("Copying {n}")),
            ClipMode::Cut => (Job::Move { sources, dest }, format!("Moving {n}")),
        };
        // "Replace" moves the old files to the trash first, so Ctrl+Z brings them back.
        let job = if replaced.is_empty() {
            paste
        } else {
            Job::Batch(vec![Job::Trash { paths: replaced }, paste])
        };
        if self.start_job(job, label) && clip_is_cut(&self.clipboard) {
            self.clipboard = None; // cut items can be pasted once
        }
    }

    /// Drag and drop: move (or copy with Ctrl) `sources` into `dest`. Name conflicts keep both.
    pub(super) fn drop_onto(&mut self, sources: Vec<PathBuf>, dest: PathBuf, copy: bool) {
        let n = liman_core::format::items(sources.len());
        let name = dest
            .file_name()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (job, label) = if copy {
            (
                Job::Copy { sources, dest },
                format!("Copying {n} to “{name}”"),
            )
        } else {
            (
                Job::Move { sources, dest },
                format!("Moving {n} to “{name}”"),
            )
        };
        if self.start_job(job, label) {
            self.marked.clear();
        }
    }

    /// Shift+Del: asks before deleting for good.
    pub(super) fn ask_delete(&mut self) {
        let paths = self.targets();
        if !paths.is_empty() {
            self.dialog = Some(Dialog::ConfirmDelete { paths });
        }
    }

    pub(super) fn on_dialog_key(&mut self, key: KeyEvent) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match (dialog, key.code) {
            (Dialog::Conflict { .. }, KeyCode::Enter | KeyCode::Char('b')) => {
                self.paste_with(Conflict::KeepBoth)
            }
            (Dialog::Conflict { .. }, KeyCode::Char('r')) => self.paste_with(Conflict::Replace),
            (Dialog::Conflict { .. }, KeyCode::Char('s')) => self.paste_with(Conflict::Skip),
            (Dialog::ConfirmDelete { paths }, KeyCode::Char('y')) => {
                let label = format!(
                    "Deleting {} for good",
                    liman_core::format::items(paths.len())
                );
                self.start_job(Job::Delete { paths }, label);
            }
            (_, KeyCode::Esc | KeyCode::Char('n' | 'q')) => {}
            (dialog, _) => self.dialog = Some(dialog), // other keys: keep asking
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
            KeyCode::Char(c) if super::is_typing(key) => input.text.push(c),
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
    pub(super) fn start_job(&mut self, job: Job, label: String) -> bool {
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
        if outcome.done.is_undoable() {
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
    fn command_output_with_paths_becomes_a_results_view() {
        let (dir, mut app, rx) = setup("results");
        app.on_command_finished(crate::terminal::CommandOutput {
            command: "find . -name '*.txt'".into(),
            cwd: dir.clone(),
            bytes: b"\x1b[32m./a.txt\x1b[0m\r\n./b.txt\r\nnot a file\r\n".to_vec(),
        });
        pump(&mut app, &rx, loaded);
        let results = app.results.clone().expect("results view");
        assert_eq!(results.count, 2);
        assert_eq!(app.visible_entries()[0].name, "a.txt");

        press(&mut app, KeyCode::Backspace); // back to the folder
        pump(&mut app, &rx, loaded);
        assert!(app.results.is_none());
        assert_eq!(app.entry_count(), Some(3)); // a.txt, b.txt, dest/

        app.on_command_finished(crate::terminal::CommandOutput {
            command: "echo hi".into(),
            cwd: dir.clone(),
            bytes: b"hi\r\n".to_vec(),
        });
        assert!(app.results.is_none() && matches!(app.listing, Listing::Ready(_)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paste_over_existing_names_asks_and_replace_is_undoable() {
        let (dir, mut app, rx) = setup("conflict");
        fs::write(dir.join("dest/a.txt"), "old").unwrap();
        select(&mut app, "a.txt");
        ctrl(&mut app, 'c');
        app.load(dir.join("dest"));
        pump(&mut app, &rx, loaded);
        ctrl(&mut app, 'v');
        assert!(matches!(app.dialog, Some(Dialog::Conflict { .. })));
        press(&mut app, KeyCode::Char('r'));
        pump(&mut app, &rx, loaded);
        assert_eq!(fs::read_to_string(dir.join("dest/a.txt")).unwrap(), "a");
        ctrl(&mut app, 'z');
        pump(&mut app, &rx, loaded);
        assert_eq!(fs::read_to_string(dir.join("dest/a.txt")).unwrap(), "old");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn shift_delete_asks_and_deletes_for_good() {
        let (dir, mut app, rx) = setup("shiftdel");
        select(&mut app, "a.txt");
        app.handle(key(KeyCode::Delete, KeyModifiers::SHIFT));
        press(&mut app, KeyCode::Char('x')); // other keys keep the question open
        assert!(matches!(app.dialog, Some(Dialog::ConfirmDelete { .. })));
        press(&mut app, KeyCode::Char('y'));
        pump(&mut app, &rx, loaded);
        assert!(!dir.join("a.txt").exists());
        assert!(!dir.join(".trash/files/a.txt").exists());
        assert!(app.history.is_empty()); // not undoable
        fs::remove_dir_all(&dir).unwrap();
    }

    fn mouse(
        kind: ratatui::crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
    ) -> AppEvent {
        AppEvent::Input(Event::Mouse(ratatui::crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }))
    }

    #[test]
    fn ctrl_and_shift_clicks_mark_and_dragging_moves_into_a_folder() {
        use ratatui::crossterm::event::{MouseButton, MouseEventKind};
        let (dir, mut app, rx) = setup("drag");
        app.list_area = ratatui::layout::Rect::new(0, 0, 80, 20); // rows start at y = 2: dest, a.txt, b.txt
        app.drawn_view = crate::app::View::Detailed;
        let none = KeyModifiers::NONE;
        app.handle(mouse(MouseEventKind::Down(MouseButton::Left), 10, 3, none)); // a.txt
        app.handle(mouse(MouseEventKind::Up(MouseButton::Left), 10, 3, none));
        app.handle(mouse(
            MouseEventKind::Down(MouseButton::Left),
            10,
            4,
            KeyModifiers::SHIFT,
        )); // to b.txt
        assert_eq!(app.marked.len(), 2);
        app.handle(mouse(
            MouseEventKind::Down(MouseButton::Left),
            10,
            4,
            KeyModifiers::CONTROL,
        )); // unmark b
        assert_eq!(app.marked.len(), 1);
        // drag a.txt (marked) onto dest/
        app.handle(mouse(MouseEventKind::Down(MouseButton::Left), 10, 3, none));
        app.handle(mouse(MouseEventKind::Drag(MouseButton::Left), 12, 2, none));
        app.handle(mouse(MouseEventKind::Up(MouseButton::Left), 12, 2, none));
        pump(&mut app, &rx, loaded);
        assert!(dir.join("dest/a.txt").exists() && !dir.join("a.txt").exists());
        assert!(dir.join("b.txt").exists());
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
