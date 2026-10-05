//! Git, through the `git` command (ADR 0007: no library, the user's own git config and keys apply).
//! Blocking: run from worker threads. Without git or outside a repository everything returns
//! `None` / an error and the UI simply shows no git information.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Two-letter status like `git status --short`: index (staged) and worktree (unstaged).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitMark {
    pub index: char,
    pub worktree: char,
}

impl GitMark {
    pub const UNTRACKED: Self = Self {
        index: '?',
        worktree: '?',
    };
    pub const CONFLICT: Self = Self {
        index: '!',
        worktree: '!',
    };
    /// A folder with changes somewhere inside.
    pub const DIRTY_FOLDER: Self = Self {
        index: ' ',
        worktree: '•',
    };

    pub fn is_staged(self) -> bool {
        !matches!(self.index, ' ' | '?' | '!' | '.')
    }

    pub fn is_unstaged(self) -> bool {
        !matches!(self.worktree, ' ' | '.')
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub root: PathBuf,
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Absolute path → mark, for changed files and for every folder above them (inside the repo).
    pub marks: HashMap<PathBuf, GitMark>,
    /// Changed files only, in git's order (for the git panel).
    pub files: Vec<(PathBuf, GitMark)>,
}

fn git(dir: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0") // status must not take locks other git commands wait on
        .env("LC_ALL", "C")
        .output()
}

/// The repository's top folder, if `dir` is inside one.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    out.status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

pub fn status(dir: &Path) -> Option<GitStatus> {
    let root = repo_root(dir)?;
    let out = git(
        &root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )
    .ok()?;
    out.status
        .success()
        .then(|| parse_status(&out.stdout, &root))
}

/// Parses `git status --porcelain=v2 --branch -z`.
pub fn parse_status(raw: &[u8], root: &Path) -> GitStatus {
    let text = String::from_utf8_lossy(raw);
    let mut st = GitStatus {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let mut fields = text.split('\0');
    while let Some(record) = fields.next() {
        let mut parts = record.splitn(2, ' ');
        let (kind, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        match kind {
            "#" => {
                if let Some(head) = rest.strip_prefix("branch.head ") {
                    st.branch = head.to_string();
                } else if let Some(up) = rest.strip_prefix("branch.upstream ") {
                    st.upstream = Some(up.to_string());
                } else if let Some(ab) = rest.strip_prefix("branch.ab ") {
                    let mut it = ab.split(' ');
                    st.ahead = it
                        .next()
                        .and_then(|a| a.trim_start_matches('+').parse().ok())
                        .unwrap_or(0);
                    st.behind = it
                        .next()
                        .and_then(|b| b.trim_start_matches('-').parse().ok())
                        .unwrap_or(0);
                }
            }
            // ordinary: XY sub mH mI mW hH hI path ; renamed: ... Xscore path, then origPath field
            "1" | "2" | "u" => {
                let skip = match kind {
                    "1" => 7,
                    "2" => 8,
                    _ => 9,
                };
                let mut xy = rest.chars();
                let (index, worktree) = (xy.next().unwrap_or(' '), xy.next().unwrap_or(' '));
                let path = rest.splitn(skip + 1, ' ').nth(skip).unwrap_or("");
                if kind == "2" {
                    fields.next(); // the original path of a rename
                }
                let mark = if kind == "u" {
                    GitMark::CONFLICT
                } else {
                    GitMark { index, worktree }
                };
                st.add(path, mark);
            }
            "?" => st.add(rest, GitMark::UNTRACKED),
            _ => {}
        }
    }
    st
}

impl GitStatus {
    fn add(&mut self, rel: &str, mark: GitMark) {
        if rel.is_empty() {
            return;
        }
        let path = self.root.join(rel.trim_end_matches('/'));
        self.files.push((path.clone(), mark));
        // Folders above the file (up to the root) show that something changed inside.
        let mut dir = path.parent();
        while let Some(d) = dir {
            if !d.starts_with(&self.root) || d == self.root {
                break;
            }
            self.marks
                .entry(d.to_path_buf())
                .or_insert(GitMark::DIRTY_FOLDER);
            dir = d.parent();
        }
        self.marks.insert(path, mark);
    }

    /// `main ↑1 ↓2` for the path bar.
    pub fn summary(&self) -> String {
        use std::fmt::Write as _;
        let mut s = self.branch.clone();
        if self.ahead > 0 {
            let _ = write!(s, " ↑{}", self.ahead);
        }
        if self.behind > 0 {
            let _ = write!(s, " ↓{}", self.behind);
        }
        s
    }
}

/// The git panel's text for one changed file: the diff of its unstaged changes (or of the staged
/// ones when nothing else changed); for an untracked file its first lines, as additions.
pub fn file_diff(root: &Path, path: &Path, mark: GitMark) -> Vec<String> {
    let text = if mark == GitMark::UNTRACKED {
        untracked_lines(path)
    } else {
        let file = path.display().to_string();
        let mut args = vec!["diff", "--no-color"];
        if !mark.is_unstaged() {
            args.push("--cached");
        }
        args.extend(["--", file.as_str()]);
        run(root, &args).unwrap_or_else(|e| e)
    };
    text.lines().map(str::to_string).collect()
}

/// Only the start of an untracked file is read (it may be a huge data file).
const UNTRACKED_BYTES: u64 = 64 * 1024;
const UNTRACKED_LINES: usize = 400;

fn untracked_lines(path: &Path) -> String {
    use std::io::Read;
    let mut buf = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|f| f.take(UNTRACKED_BYTES).read_to_end(&mut buf));
    if read.is_err() || buf.contains(&0) {
        return crate::i18n::tr("(binary or unreadable)").into();
    }
    String::from_utf8_lossy(&buf)
        .lines()
        .take(UNTRACKED_LINES)
        .map(|l| format!("+{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The remote pushes go to: the upstream's remote, else `origin`, else the first one.
/// Returns its name and URL.
pub fn push_remote(dir: &Path, upstream: Option<&str>) -> Option<(String, String)> {
    let names = run(dir, &["remote"]).ok()?;
    let names: Vec<&str> = names.lines().filter(|l| !l.is_empty()).collect();
    let from_upstream = upstream.and_then(|u| u.split('/').next());
    let name = from_upstream
        .filter(|n| names.contains(n))
        .or_else(|| names.iter().copied().find(|n| *n == "origin"))
        .or_else(|| names.first().copied())?
        .to_string();
    let url = run(dir, &["remote", "get-url", &name])
        .ok()?
        .trim()
        .to_string();
    Some((name, url))
}

/// `git@github.com:user/repo.git` / `https://github.com/user/repo.git` → `github.com/user/repo`.
pub fn short_url(url: &str) -> String {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .unwrap_or(url);
    let url = url.split_once('@').map_or(url, |(_, rest)| rest);
    url.replacen(':', "/", 1)
}

/// Runs a git command in `dir` and returns its combined output, or the error text.
pub fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(dir, args).map_err(|e| format!("git: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text.trim().to_string())
    } else {
        Err(text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_are_shortened() {
        assert_eq!(
            short_url("git@github.com:WertC-14/liman.git"),
            "github.com/WertC-14/liman"
        );
        assert_eq!(short_url("https://github.com/a/b.git"), "github.com/a/b");
        assert_eq!(short_url("ssh://git@host:22/x/y"), "host/22/x/y");
    }
    use crate::ops::test_dir;
    use std::fs;

    #[test]
    fn untracked_files_show_only_their_start() {
        let dir = test_dir("git-untracked");
        let text = dir.join("notes.txt");
        let long: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        fs::write(&text, long).unwrap();
        let lines = file_diff(&dir, &text, GitMark::UNTRACKED);
        assert_eq!(lines.len(), UNTRACKED_LINES);
        assert_eq!(lines[0], "+line 0");
        let binary = dir.join("data.bin");
        fs::write(&binary, [1u8, 0, 2]).unwrap();
        assert_eq!(
            file_diff(&dir, &binary, GitMark::UNTRACKED),
            ["(binary or unreadable)"]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_branch_files_and_folders() {
        let raw = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 h1 h2 src/main.rs\0\
1 A. N... 000000 100644 100644 h1 h2 docs/new file.md\0\
2 R. N... 100644 100644 100644 h1 h2 R100 lib.rs\0old.rs\0\
u UU N... 1 2 3 4 h1 h2 h3 conflict.txt\0\
? notes/todo.txt\0";
        let st = parse_status(raw.as_bytes(), Path::new("/r"));
        assert_eq!(st.summary(), "main ↑2 ↓1");
        assert_eq!(
            st.marks[Path::new("/r/src/main.rs")],
            GitMark {
                index: '.',
                worktree: 'M'
            }
        );
        assert_eq!(st.marks[Path::new("/r/docs/new file.md")].index, 'A');
        assert_eq!(st.marks[Path::new("/r/lib.rs")].index, 'R');
        assert_eq!(st.marks[Path::new("/r/conflict.txt")], GitMark::CONFLICT);
        assert_eq!(st.marks[Path::new("/r/notes/todo.txt")], GitMark::UNTRACKED);
        assert_eq!(st.marks[Path::new("/r/src")], GitMark::DIRTY_FOLDER);
        assert!(!st.marks.contains_key(Path::new("/r")));
        assert_eq!(st.files.len(), 5);
    }

    #[test]
    fn real_repository_round_trip() {
        if git(Path::new("/"), &["--version"]).is_err() {
            return; // no git on this machine
        }
        let dir = test_dir("git");
        run(&dir, &["init", "-q", "-b", "main"]).unwrap();
        fs::write(dir.join("a.txt"), "x").unwrap();
        let st = status(&dir).expect("a repository");
        assert_eq!(st.branch, "main");
        assert_eq!(st.marks[&dir.join("a.txt")], GitMark::UNTRACKED);
        run(&dir, &["add", "a.txt"]).unwrap();
        assert!(status(&dir).unwrap().marks[&dir.join("a.txt")].is_staged());
        assert!(status(Path::new("/")).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
