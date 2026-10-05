//! The folder tree of the sidebar (cardea's "Directories"): roots (home, `/`), folders that are
//! expanded, and the children read for them. The rows to draw are the tree flattened in order.
//! Reading children is blocking ([`subfolders`]): the app does it on a worker.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::sort::natural_cmp;

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Debug, Default)]
pub struct DirTree {
    roots: Vec<PathBuf>,
    /// Subfolders of the folders read so far, sorted.
    children: HashMap<PathBuf, Vec<PathBuf>>,
    expanded: HashSet<PathBuf>,
    /// The folder the tree was last focused on.
    focused: Option<PathBuf>,
    /// The flattened rows, rebuilt after every change.
    rows: Vec<TreeRow>,
}

impl DirTree {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        let mut tree = Self {
            roots,
            ..Self::default()
        };
        tree.rebuild();
        tree
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn is_loaded(&self, dir: &Path) -> bool {
        self.children.contains_key(dir)
    }

    /// The subfolders of `dir` arrived (or changed).
    pub fn set_children(&mut self, dir: PathBuf, children: Vec<PathBuf>) {
        if self.children.get(&dir) != Some(&children) {
            self.children.insert(dir, children);
            self.rebuild();
        }
    }

    /// Opens `dir`; returns true when its children still have to be read.
    pub fn expand(&mut self, dir: &Path) -> bool {
        self.expanded.insert(dir.to_path_buf());
        self.rebuild();
        !self.is_loaded(dir)
    }

    pub fn collapse(&mut self, dir: &Path) {
        if self.expanded.remove(dir) {
            self.rebuild();
        }
    }

    /// Shows where `dir` is: the folders from its root down to `dir` itself are open, every
    /// other branch is closed (so the tree never piles up branches opened along the way).
    /// Returns the folders whose children still have to be read, top down.
    ///
    /// Focusing the same folder again (a reload of it) changes nothing, so branches opened by
    /// hand stay open until the user moves.
    pub fn focus(&mut self, dir: &Path) -> Vec<PathBuf> {
        if self.focused.as_deref() == Some(dir) {
            return Vec::new();
        }
        self.focused = Some(dir.to_path_buf());
        // The deepest root that contains `dir` (home before `/`).
        let Some(root) = self
            .roots
            .iter()
            .filter(|r| dir.starts_with(r))
            .max_by_key(|r| r.components().count())
            .cloned()
        else {
            return Vec::new();
        };
        let path: HashSet<PathBuf> = dir
            .ancestors()
            .take_while(|a| a.starts_with(&root))
            .map(Path::to_path_buf)
            .collect();
        let mut missing: Vec<PathBuf> = path
            .iter()
            .filter(|d| !self.is_loaded(d))
            .cloned()
            .collect();
        missing.sort_by_key(|d| d.components().count());
        if self.expanded != path {
            self.expanded = path;
            self.rebuild();
        }
        missing
    }

    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        for root in &self.roots {
            self.push(root, 0, &mut rows);
        }
        self.rows = rows;
    }

    fn push(&self, dir: &Path, depth: usize, rows: &mut Vec<TreeRow>) {
        let expanded = self.expanded.contains(dir);
        rows.push(TreeRow {
            path: dir.to_path_buf(),
            name: dir.file_name().map_or_else(
                || dir.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            depth,
            expanded,
        });
        if expanded && let Some(children) = self.children.get(dir) {
            for child in children {
                self.push(child, depth + 1, rows);
            }
        }
    }
}

/// The folders inside `dir` (links to folders too), in natural order. Hidden ones only with
/// `show_hidden`.
pub fn subfolders(dir: &Path, show_hidden: bool) -> Vec<PathBuf> {
    let Ok(items) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = items
        .flatten()
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                return None;
            }
            let is_dir = match item.file_type() {
                Ok(t) if t.is_symlink() => item.path().is_dir(),
                Ok(t) => t.is_dir(),
                Err(_) => false,
            };
            is_dir.then(|| (name, item.path()))
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    out.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn names(tree: &DirTree) -> Vec<String> {
        tree.rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    #[test]
    fn focus_opens_only_the_way_to_the_folder() {
        let mut tree = DirTree::new(vec![p("/home/u"), p("/")]);
        assert_eq!(names(&tree), ["u", "/"]);
        // The way down and the folder itself are open; their children are asked for.
        let missing = tree.focus(Path::new("/home/u/Projects"));
        assert_eq!(missing, [p("/home/u"), p("/home/u/Projects")]);
        tree.set_children(
            p("/home/u"),
            vec![p("/home/u/Documents"), p("/home/u/Projects")],
        );
        tree.set_children(p("/home/u/Projects"), vec![p("/home/u/Projects/liman")]);
        tree.set_children(p("/home/u/Documents"), vec![p("/home/u/Documents/notes")]);
        assert_eq!(
            names(&tree),
            ["u", "  Documents", "  Projects", "    liman", "/"]
        );
        // A branch opened by hand stays open until the next move.
        assert!(!tree.expand(Path::new("/home/u/Documents")));
        assert_eq!(names(&tree)[2], "    notes");
        // A reload of the same folder keeps the hand-opened branch.
        assert!(tree.focus(Path::new("/home/u/Projects")).is_empty());
        assert_eq!(names(&tree)[2], "    notes");
        // Back home: everything below it closes (the user's request, LOG 2026-10-05).
        assert!(tree.focus(Path::new("/home/u")).is_empty());
        assert_eq!(names(&tree), ["u", "  Documents", "  Projects", "/"]);
        tree.collapse(Path::new("/home/u"));
        assert_eq!(names(&tree), ["u", "/"]);
        // Outside home: under `/`, and home closes.
        assert_eq!(tree.focus(Path::new("/etc")), [p("/"), p("/etc")]);
    }

    #[test]
    fn subfolders_are_folders_only_in_natural_order() {
        let dir = crate::ops::test_dir("tree");
        for d in ["b10", "b2", ".hidden", "a"] {
            fs::create_dir(dir.join(d)).unwrap();
        }
        fs::write(dir.join("file.txt"), "").unwrap();
        let names: Vec<_> = subfolders(&dir, false)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a", "b2", "b10"]);
        assert_eq!(subfolders(&dir, true).len(), 4);
        fs::remove_dir_all(&dir).unwrap();
    }
}
