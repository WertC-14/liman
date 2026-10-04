//! A user action on files (copy, move, rename, trash, undo) as one unit of work.
//!
//! Running a [`Job`] gives a [`Done`] record of exactly what changed on disk. That record is what
//! undo (Ctrl+Z) reverses. If a job fails half-way, the record still lists the finished part,
//! so even a partial job can be undone.

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
}

impl Done {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Copied(v) => v.is_empty(),
            Self::Moved(v) => v.is_empty(),
            Self::Renamed { .. } => false,
            Self::Trashed(v) => v.is_empty(),
            Self::Undone(_) => false,
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
        }
    }

    /// After undoing `self`, the path that is back (old name, original place).
    fn restored_focus(&self) -> Option<&Path> {
        match self {
            Self::Copied(_) | Self::Undone(_) => None,
            Self::Moved(v) => v.first().map(|(from, _)| from.as_path()),
            Self::Renamed { from, .. } => Some(from),
            Self::Trashed(v) => v.first().map(|item| item.original.as_path()),
        }
    }

    /// Short description for the status bar: `Copied 3 items`, `Renamed “a” to “b”`.
    pub fn describe(&self) -> String {
        let count = |n: usize| crate::format::items(n);
        match self {
            Self::Copied(v) => format!("Copied {}", count(v.len())),
            Self::Moved(v) => format!("Moved {}", count(v.len())),
            Self::Renamed { from, to } => format!("Renamed “{}” to “{}”", name(from), name(to)),
            Self::Trashed(v) => format!("Moved {} to the trash", count(v.len())),
            Self::Undone(done) => format!("Undone: {}", done.describe()),
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
    /// Progress unit: bytes for copy/move, items for the rest.
    pub fn total(&self) -> u64 {
        match self {
            Self::Copy { sources, .. } | Self::Move { sources, .. } => {
                sources.iter().map(|p| ops::total_size(p)).sum()
            }
            Self::Rename { .. } => 1,
            Self::Trash { paths } => paths.len() as u64,
            Self::Undo(_) => 1,
        }
    }

    /// Runs the job. Blocking: call from a worker thread. `progress` gets the amount done so far.
    pub fn run(self, trash_dir: &Path, progress: &mut dyn FnMut(u64)) -> Outcome {
        match self {
            Self::Copy { sources, dest } => {
                let mut created = Vec::new();
                let mut base = 0;
                for src in &sources {
                    match ops::copy_into(src, &dest, &mut |b| progress(base + b)) {
                        Ok(path) => created.push(path),
                        Err(e) => return fail(Done::Copied(created), src, e),
                    }
                    base += ops::total_size(src);
                }
                ok(Done::Copied(created))
            }
            Self::Move { sources, dest } => {
                let mut moved = Vec::new();
                let mut base = 0;
                for src in &sources {
                    let size = ops::total_size(src);
                    match ops::move_into(src, &dest, &mut |b| progress(base + b)) {
                        Ok(to) if &to == src => {} // already there
                        Ok(to) => moved.push((src.clone(), to)),
                        Err(e) => return fail(Done::Moved(moved), src, e),
                    }
                    base += size;
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
                    error: Some(format!("Cannot rename “{}”: {e}", name(&path))),
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
            Self::Undo(done) => {
                let error = undo(&done, trash_dir)
                    .err()
                    .map(|e| format!("Cannot undo: {e}"));
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
        Done::Undone(_) => {}
    }
    Ok(())
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
