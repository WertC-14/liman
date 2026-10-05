//! A user action on files (copy, move, rename, trash, undo) as one unit of work.
//!
//! Running a [`Job`] gives a [`Done`] record of exactly what changed on disk. That record is what
//! undo (Ctrl+Z) reverses. If a job fails half-way, the record still lists the finished part,
//! so even a partial job can be undone.

use crate::i18n::trf;
use std::path::{Path, PathBuf};

use crate::ops;
use crate::trash::{self, TrashedItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Copy {
        sources: Vec<PathBuf>,
        dest: PathBuf,
    },
    Move {
        sources: Vec<PathBuf>,
        dest: PathBuf,
    },
    Rename {
        path: PathBuf,
        new_name: String,
    },
    Trash {
        paths: Vec<PathBuf>,
    },
    /// Reverses an earlier job.
    Undo(Done),
    /// Deletes for good (Shift+Del, after a confirmation). Cannot be undone.
    Delete {
        paths: Vec<PathBuf>,
    },
    /// A new empty folder at a free name like `New folder (2)` inside `parent`.
    CreateDir {
        parent: PathBuf,
        name: String,
    },
    /// Several jobs in order, undone as one (e.g. "replace" = trash the old files, then copy).
    Batch(Vec<Job>),
}

/// What a job did. Paths are absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// Newly created copies.
    Copied(Vec<PathBuf>),
    /// (from, to) pairs.
    Moved(Vec<(PathBuf, PathBuf)>),
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
    Trashed(Vec<TrashedItem>),
    /// An earlier job was reversed. Not undoable itself (no redo yet).
    Undone(Box<Done>),
    /// A folder that was created.
    Created(PathBuf),
    /// Number of items deleted for good. Not undoable.
    Deleted(usize),
    /// The parts of a batch, in the order they ran.
    Batch(Vec<Done>),
}

impl Done {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Copied(v) => v.is_empty(),
            Self::Moved(v) => v.is_empty(),
            Self::Renamed { .. } => false,
            Self::Trashed(v) => v.is_empty(),
            Self::Undone(_) => false,
            Self::Deleted(n) => *n == 0,
            Self::Created(_) => false,
            Self::Batch(parts) => parts.iter().all(Done::is_empty),
        }
    }

    /// Whether Ctrl+Z can reverse it.
    pub fn is_undoable(&self) -> bool {
        match self {
            Self::Undone(_) | Self::Deleted(_) => false,
            Self::Batch(parts) => parts.iter().all(Done::is_undoable),
            _ => !self.is_empty(),
        }
    }

    /// The path the user most likely wants selected afterwards (the new name, the first copy...).
    pub fn focus(&self) -> Option<&Path> {
        match self {
            Self::Copied(v) => v.first().map(PathBuf::as_path),
            Self::Moved(v) => v.first().map(|(_, to)| to.as_path()),
            Self::Renamed { to, .. } => Some(to),
            Self::Trashed(_) => None,
            Self::Undone(done) => done.restored_focus(),
            Self::Deleted(_) => None,
            Self::Created(path) => Some(path),
            Self::Batch(parts) => parts.iter().rev().find_map(Done::focus),
        }
    }

    /// After undoing `self`, the path that is back (old name, original place).
    fn restored_focus(&self) -> Option<&Path> {
        match self {
            Self::Copied(_) | Self::Undone(_) | Self::Deleted(_) | Self::Created(_) => None,
            Self::Batch(parts) => parts.iter().find_map(Done::restored_focus),
            Self::Moved(v) => v.first().map(|(from, _)| from.as_path()),
            Self::Renamed { from, .. } => Some(from),
            Self::Trashed(v) => v.first().map(|item| item.original.as_path()),
        }
    }

    /// Short description for the status bar: `Copied 3 items`, `Renamed “a” to “b”`.
    pub fn describe(&self) -> String {
        let count = |n: usize| crate::format::items(n);
        match self {
            Self::Copied(v) => trf("Copied {}", &[&count(v.len())]),
            Self::Moved(v) => trf("Moved {}", &[&count(v.len())]),
            Self::Renamed { from, to } => trf("Renamed “{}” to “{}”", &[&name(from), &name(to)]),
            Self::Trashed(v) => trf("Moved {} to the trash", &[&count(v.len())]),
            Self::Undone(done) => trf("Undone: {}", &[&done.describe()]),
            Self::Deleted(n) => trf("Deleted {} for good", &[&count(*n)]),
            Self::Created(path) => trf("Created “{}”", &[&name(path)]),
            Self::Batch(parts) => parts
                .iter()
                .filter(|d| !d.is_empty())
                .map(Done::describe)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

#[derive(Debug)]
pub struct Outcome {
    pub done: Done,
    /// First error; the job stops there.
    pub error: Option<String>,
}

impl Job {
    /// Progress unit: bytes for copy/move, items for the rest (a same-device move counts 1 per item).
    /// Walks the source trees of a copy: call it on a worker thread.
    pub fn total(&self) -> u64 {
        match self {
            Self::Copy { sources, .. } => sources.iter().map(|p| ops::total_size(p)).sum(),
            // A move inside one file system is a rename: one unit, no need to walk the tree.
            Self::Move { sources, dest } => sources
                .iter()
                .map(|p| {
                    if same_device(p, dest) {
                        1
                    } else {
                        ops::total_size(p)
                    }
                })
                .sum(),
            Self::Rename { .. } => 1,
            Self::Trash { paths } => paths.len() as u64,
            Self::Undo(_) => 1,
            Self::Delete { paths } => paths.len() as u64,
            Self::CreateDir { .. } => 1,
            Self::Batch(jobs) => jobs.iter().map(Job::total).sum(),
        }
    }

    /// Runs the job. Blocking: call from a worker thread. `progress` gets the amount done so far.
    pub fn run(self, trash_dir: &Path, progress: &mut dyn FnMut(u64)) -> Outcome {
        match self {
            Self::Copy { sources, dest } => {
                let mut created = Vec::new();
                let mut base = 0;
                for src in &sources {
                    // The bytes this source took, from its own progress (no second walk of the tree).
                    let mut copied = 0;
                    let result = ops::copy_into(src, &dest, &mut |b| {
                        copied = b;
                        progress(base + b);
                    });
                    match result {
                        Ok(path) => created.push(path),
                        Err(e) => return fail(Done::Copied(created), src, e),
                    }
                    base += copied;
                }
                ok(Done::Copied(created))
            }
            Self::Move { sources, dest } => {
                let mut moved = Vec::new();
                let mut base = 0;
                for src in &sources {
                    // Bytes copied when the move crossed file systems; 0 for a rename (one unit).
                    let mut copied = 0;
                    let result = ops::move_into(src, &dest, &mut |b| {
                        copied = b;
                        progress(base + b);
                    });
                    match result {
                        Ok(to) if &to == src => {} // already there
                        Ok(to) => moved.push((src.clone(), to)),
                        Err(e) => return fail(Done::Moved(moved), src, e),
                    }
                    base += copied.max(1);
                    progress(base);
                }
                ok(Done::Moved(moved))
            }
            Self::Rename { path, new_name } => match ops::rename(&path, &new_name) {
                Ok(to) => {
                    progress(1);
                    ok(Done::Renamed { from: path, to })
                }
                Err(e) => Outcome {
                    // Nothing changed; an empty Moved record undoes to nothing.
                    done: Done::Moved(Vec::new()),
                    error: Some(trf("Cannot rename “{}”: {}", &[&name(&path), &e])),
                },
            },
            Self::Trash { paths } => {
                let mut items = Vec::new();
                for (i, path) in paths.iter().enumerate() {
                    match trash::trash(path, trash_dir) {
                        Ok(item) => items.push(item),
                        Err(e) => return fail(Done::Trashed(items), path, e),
                    }
                    progress(i as u64 + 1);
                }
                ok(Done::Trashed(items))
            }
            Self::Delete { paths } => {
                for (i, path) in paths.iter().enumerate() {
                    if let Err(e) = ops::remove_all(path) {
                        return fail(Done::Deleted(i), path, e);
                    }
                    progress(i as u64 + 1);
                }
                ok(Done::Deleted(paths.len()))
            }
            Self::CreateDir { parent, name } => {
                let path = ops::free_name(&parent, &name);
                match std::fs::create_dir(&path) {
                    Ok(()) => ok(Done::Created(path)),
                    Err(e) => fail(Done::Batch(Vec::new()), &path, e),
                }
            }
            Self::Batch(jobs) => {
                let mut parts = Vec::new();
                let mut base = 0;
                for job in jobs {
                    let mut part = 0;
                    let out = job.run(trash_dir, &mut |d| {
                        part = d;
                        progress(base + d);
                    });
                    base += part;
                    parts.push(out.done);
                    if out.error.is_some() {
                        return Outcome {
                            done: Done::Batch(parts),
                            error: out.error,
                        };
                    }
                }
                ok(Done::Batch(parts))
            }
            Self::Undo(done) => {
                let error = undo(&done, trash_dir)
                    .err()
                    .map(|e| trf("Cannot undo: {}", &[&e]));
                progress(1);
                Outcome {
                    done: Done::Undone(Box::new(done)),
                    error,
                }
            }
        }
    }
}

/// Reverses `done`, newest change first. Copies go to the trash rather than being deleted,
/// so even undoing a copy loses nothing.
fn undo(done: &Done, trash_dir: &Path) -> std::io::Result<()> {
    match done {
        Done::Copied(paths) => {
            for path in paths.iter().rev() {
                trash::trash(path, trash_dir)?;
            }
        }
        Done::Moved(pairs) => {
            for (from, to) in pairs.iter().rev() {
                ops::move_path(to, from)?;
            }
        }
        Done::Renamed { from, to } => ops::move_path(to, from)?,
        Done::Trashed(items) => {
            for item in items.iter().rev() {
                trash::restore(item)?;
            }
        }
        Done::Undone(_) | Done::Deleted(_) => {}
        // Into the trash, not deleted: the user may already have put things inside.
        Done::Created(path) => {
            trash::trash(path, trash_dir)?;
        }
        Done::Batch(parts) => {
            for part in parts.iter().rev() {
                undo(part, trash_dir)?;
            }
        }
    }
    Ok(())
}

/// Whether `path` and the folder `dest` are on the same file system (a move is then a rename).
fn same_device(path: &Path, dest: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (path.symlink_metadata(), dest.metadata()) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

fn ok(done: Done) -> Outcome {
    Outcome { done, error: None }
}

fn fail(done: Done, path: &Path, e: std::io::Error) -> Outcome {
    Outcome {
        done,
        error: Some(format!("“{}”: {e}", name(path))),
    }
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::test_dir;
    use std::fs;

    #[test]
    fn copy_job_records_new_paths_and_reports_bytes() {
        let dir = test_dir("job-copy");
        fs::write(dir.join("a.txt"), "aaa").unwrap();
        fs::write(dir.join("b.txt"), "bb").unwrap();
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let job = Job::Copy {
            sources: vec![dir.join("a.txt"), dir.join("b.txt")],
            dest: dest.clone(),
        };
        assert_eq!(job.total(), 5);
        let mut last = 0;
        let out = job.run(&dir.join("Trash"), &mut |b| last = b);
        assert!(out.error.is_none());
        assert_eq!(last, 5);
        assert_eq!(
            out.done,
            Done::Copied(vec![dest.join("a.txt"), dest.join("b.txt")])
        );
        assert_eq!(out.done.describe(), "Copied 2 items");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_device_move_counts_items_and_progress_reaches_the_total() {
        let dir = test_dir("job-move");
        fs::create_dir(dir.join("big")).unwrap();
        fs::write(dir.join("big/data"), "0123456789").unwrap();
        fs::write(dir.join("c.txt"), "c").unwrap();
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let mov = Job::Move {
            sources: vec![dir.join("big"), dir.join("c.txt")],
            dest: dest.clone(),
        };
        // A rename per item, not the 11 bytes inside.
        assert_eq!(mov.total(), 2);
        // A batch reports the parts one after another and ends exactly at its total.
        fs::write(dir.join("d.txt"), "ddd").unwrap();
        let batch = Job::Batch(vec![
            mov,
            Job::Copy {
                sources: vec![dir.join("d.txt")],
                dest: dest.clone(),
            },
        ]);
        let total = batch.total();
        assert_eq!(total, 2 + 3);
        let mut last = 0;
        let out = batch.run(&dir.join("Trash"), &mut |d| last = d);
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(last, total);
        assert!(dest.join("big/data").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failing_job_keeps_the_finished_part() {
        let dir = test_dir("job-partial");
        fs::write(dir.join("a.txt"), "a").unwrap();
        let job = Job::Trash {
            paths: vec![dir.join("a.txt"), dir.join("missing.txt")],
        };
        let out = job.run(&dir.join("Trash"), &mut |_| {});
        assert!(out.error.unwrap().contains("missing.txt"));
        let Done::Trashed(items) = out.done else {
            panic!("expected a trash record")
        };
        assert_eq!(items.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs `job`, then undoes it, and returns the undo outcome.
    fn run_and_undo(job: Job, trash_dir: &Path) -> Outcome {
        let out = job.run(trash_dir, &mut |_| {});
        assert!(out.error.is_none(), "{:?}", out.error);
        Job::Undo(out.done).run(trash_dir, &mut |_| {})
    }

    #[test]
    fn undo_reverses_every_kind_of_job() {
        let dir = test_dir("job-undo");
        let trash_dir = dir.join("Trash");
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();

        let out = run_and_undo(
            Job::Rename {
                path: dir.join("a.txt"),
                new_name: "b.txt".into(),
            },
            &trash_dir,
        );
        assert!(out.error.is_none());
        assert!(dir.join("a.txt").exists() && !dir.join("b.txt").exists());
        assert_eq!(out.done.focus(), Some(dir.join("a.txt").as_path()));
        assert_eq!(out.done.describe(), "Undone: Renamed “a.txt” to “b.txt”");

        run_and_undo(
            Job::Move {
                sources: vec![dir.join("a.txt")],
                dest: dir.join("sub"),
            },
            &trash_dir,
        );
        assert!(dir.join("a.txt").exists() && !dir.join("sub/a.txt").exists());

        run_and_undo(
            Job::Trash {
                paths: vec![dir.join("a.txt")],
            },
            &trash_dir,
        );
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "a");

        run_and_undo(
            Job::Copy {
                sources: vec![dir.join("a.txt")],
                dest: dir.join("sub"),
            },
            &trash_dir,
        );
        assert!(!dir.join("sub/a.txt").exists());
        assert!(dir.join("a.txt").exists());
        assert!(trash_dir.join("files/a.txt").exists()); // the copy went to the trash
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replace_batch_undoes_as_one() {
        let dir = test_dir("job-replace");
        let trash_dir = dir.join("Trash");
        fs::create_dir_all(dir.join("dest")).unwrap();
        fs::write(dir.join("a.txt"), "new").unwrap();
        fs::write(dir.join("dest/a.txt"), "old").unwrap();
        let job = Job::Batch(vec![
            Job::Trash {
                paths: vec![dir.join("dest/a.txt")],
            },
            Job::Copy {
                sources: vec![dir.join("a.txt")],
                dest: dir.join("dest"),
            },
        ]);
        let out = job.run(&trash_dir, &mut |_| {});
        assert!(out.error.is_none());
        assert_eq!(fs::read_to_string(dir.join("dest/a.txt")).unwrap(), "new");
        assert!(out.done.is_undoable());
        assert_eq!(
            out.done.describe(),
            "Moved 1 item to the trash, Copied 1 item"
        );
        let undo = Job::Undo(out.done).run(&trash_dir, &mut |_| {});
        assert!(undo.error.is_none(), "{:?}", undo.error);
        assert_eq!(fs::read_to_string(dir.join("dest/a.txt")).unwrap(), "old");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn create_dir_picks_a_free_name_and_undo_trashes_it() {
        let dir = test_dir("job-mkdir");
        fs::create_dir(dir.join("New folder")).unwrap();
        let out = Job::CreateDir {
            parent: dir.clone(),
            name: "New folder".into(),
        }
        .run(&dir.join("Trash"), &mut |_| {});
        assert_eq!(out.done, Done::Created(dir.join("New folder (2)")));
        assert!(dir.join("New folder (2)").is_dir());
        Job::Undo(out.done).run(&dir.join("Trash"), &mut |_| {});
        assert!(!dir.join("New folder (2)").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn delete_is_for_good_and_not_undoable() {
        let dir = test_dir("job-delete");
        fs::create_dir_all(dir.join("f/g")).unwrap();
        let out = Job::Delete {
            paths: vec![dir.join("f")],
        }
        .run(&dir.join("Trash"), &mut |_| {});
        assert!(!dir.join("f").exists());
        assert_eq!(out.done, Done::Deleted(1));
        assert!(!out.done.is_undoable());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn undo_fails_safely_when_the_old_place_is_taken() {
        let dir = test_dir("job-undo-clash");
        fs::write(dir.join("a.txt"), "a").unwrap();
        let out = Job::Rename {
            path: dir.join("a.txt"),
            new_name: "b.txt".into(),
        }
        .run(&dir.join("Trash"), &mut |_| {});
        fs::write(dir.join("a.txt"), "new").unwrap(); // someone created a.txt again
        let undo = Job::Undo(out.done).run(&dir.join("Trash"), &mut |_| {});
        assert!(undo.error.is_some());
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "new");
        assert!(dir.join("b.txt").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_and_move_records() {
        let dir = test_dir("job-rename");
        fs::write(dir.join("a.txt"), "a").unwrap();
        let out = Job::Rename {
            path: dir.join("a.txt"),
            new_name: "b.txt".into(),
        }
        .run(&dir.join("Trash"), &mut |_| {});
        assert_eq!(
            out.done,
            Done::Renamed {
                from: dir.join("a.txt"),
                to: dir.join("b.txt")
            }
        );
        assert_eq!(out.done.focus(), Some(dir.join("b.txt").as_path()));

        fs::create_dir(dir.join("sub")).unwrap();
        let out = Job::Move {
            sources: vec![dir.join("b.txt")],
            dest: dir.join("sub"),
        }
        .run(&dir.join("Trash"), &mut |_| {});
        assert_eq!(
            out.done,
            Done::Moved(vec![(dir.join("b.txt"), dir.join("sub/b.txt"))])
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
