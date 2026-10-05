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

    /// Opens every folder from the root above `dir` down to its parent, so `dir` is visible.
    /// Returns the folders whose children still have to be read.
    pub fn reveal(&mut self, dir: &Path) -> Vec<PathBuf> {
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
        let mut missing = Vec::new();
        let mut opened = false;
        for ancestor in dir.ancestors().skip(1) {
            if !ancestor.starts_with(&root) {
                break;
            }
            opened |= self.expanded.insert(ancestor.to_path_buf());
            if !self.is_loaded(ancestor) {
                missing.push(ancestor.to_path_buf());
            }
        }
        if opened {
            self.rebuild();
        }
        missing.reverse(); // top down
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
    fn reveal_opens_the_way_down_and_asks_for_missing_children() {
        let mut tree = DirTree::new(vec![p("/home/u"), p("/")]);
        assert_eq!(names(&tree), ["u", "/"]);
        let missing = tree.reveal(Path::new("/home/u/Projects/liman"));
        assert_eq!(missing, [p("/home/u"), p("/home/u/Projects")]);
        tree.set_children(
            p("/home/u"),
            vec![p("/home/u/Documents"), p("/home/u/Projects")],
        );
        tree.set_children(p("/home/u/Projects"), vec![p("/home/u/Projects/liman")]);
        assert_eq!(
            names(&tree),
            ["u", "  Documents", "  Projects", "    liman", "/"]
        );
        // Already loaded: nothing to read again.
        assert!(tree.reveal(Path::new("/home/u/Projects/liman")).is_empty());
        tree.collapse(Path::new("/home/u"));
        assert_eq!(names(&tree), ["u", "/"]);
        assert!(!tree.expand(Path::new("/home/u")));
        assert_eq!(tree.rows().len(), 5);
        // Outside home: under `/`.
        assert_eq!(tree.reveal(Path::new("/etc")), [p("/")]);
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
