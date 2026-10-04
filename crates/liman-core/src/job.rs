//! A user action on files (copy, move, rename, trash) as one unit of work.
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
}

impl Done {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Copied(v) => v.is_empty(),
            Self::Moved(v) => v.is_empty(),
            Self::Renamed { .. } => false,
            Self::Trashed(v) => v.is_empty(),
        }
    }

    /// The path the user most likely wants selected afterwards (the new name, the first copy...).
    pub fn focus(&self) -> Option<&Path> {
        match self {
            Self::Copied(v) => v.first().map(PathBuf::as_path),
            Self::Moved(v) => v.first().map(|(_, to)| to.as_path()),
            Self::Renamed { to, .. } => Some(to),
            Self::Trashed(_) => None,
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
        }
    }
}

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
        }
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
