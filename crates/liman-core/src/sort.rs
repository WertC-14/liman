//! Sorting like GUI file managers: folders first, then case-insensitive "natural" order
//! (`file2` before `file10`).

use std::cmp::Ordering;

use crate::Entry;

pub fn sort_entries(entries: &mut [Entry]) {
    sort_with(entries, SortOrder::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
    Type,
}

impl SortKey {
    pub const ALL: [SortKey; 4] = [Self::Name, Self::Size, Self::Modified, Self::Type];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Type => "type",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SortOrder {
    pub key: SortKey,
    pub descending: bool,
}

impl SortOrder {
    /// For the config file: `name`, `size-desc`, ...
    pub fn to_config(self) -> String {
        format!(
            "{}{}",
            self.key.name(),
            if self.descending { "-desc" } else { "" }
        )
    }

    pub fn from_config(text: &str) -> Option<Self> {
        let (key, descending) = match text.strip_suffix("-desc") {
            Some(k) => (k, true),
            None => (text, false),
        };
        let key = SortKey::ALL.into_iter().find(|k| k.name() == key)?;
        Some(Self { key, descending })
    }
}

/// Folders always first (like GUI file managers), then by `order`; ties by natural name order.
pub fn sort_with(entries: &mut [Entry], order: SortOrder) {
    entries.sort_by(|a, b| {
        let by_key = match order.key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Size => size_of(a).cmp(&size_of(b)),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Type => a
                .extension()
                .map(str::to_lowercase)
                .cmp(&b.extension().map(str::to_lowercase)),
        };
        let by_key = if order.descending {
            by_key.reverse()
        } else {
            by_key
        };
        b.is_dir
            .cmp(&a.is_dir)
            .then(by_key)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
}

/// Folders compare by their number of items, files by bytes.
fn size_of(e: &Entry) -> u64 {
    if e.is_dir {
        e.item_count.unwrap_or(0) as u64
    } else {
        e.size
    }
}

/// Compares runs of digits by numeric value and everything else case-insensitively.
/// Falls back to a plain comparison so that the order is total (`a` vs `A`, `01` vs `1`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_digits(&mut ai);
                let nb = take_digits(&mut bi);
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        s.push(c);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64, age: u64) -> Entry {
        let path = std::path::PathBuf::from(name);
        Entry {
            name: name.into(),
            file_type: crate::FileType::from_path(&path, is_dir),
            path,
            is_dir,
            is_symlink: false,
            special: None,
            size,
            item_count: None,
            modified: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(age)),
        }
    }

    #[test]
    fn sort_orders_keep_folders_first() {
        let mut v = vec![
            entry("b.txt", false, 10, 3),
            entry("a.pdf", false, 99, 1),
            entry("dir", true, 0, 2),
            entry("c.rs", false, 50, 2),
        ];
        let names = |v: &[Entry]| v.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        sort_with(
            &mut v,
            SortOrder {
                key: SortKey::Size,
                descending: true,
            },
        );
        assert_eq!(names(&v), ["dir", "a.pdf", "c.rs", "b.txt"]);
        sort_with(
            &mut v,
            SortOrder {
                key: SortKey::Modified,
                descending: false,
            },
        );
        assert_eq!(names(&v), ["dir", "a.pdf", "c.rs", "b.txt"]);
        sort_with(
            &mut v,
            SortOrder {
                key: SortKey::Type,
                descending: false,
            },
        );
        assert_eq!(names(&v), ["dir", "a.pdf", "c.rs", "b.txt"]);
        sort_with(&mut v, SortOrder::default());
        assert_eq!(names(&v), ["dir", "a.pdf", "b.txt", "c.rs"]);
    }

    #[test]
    fn sort_order_config_round_trip() {
        let o = SortOrder {
            key: SortKey::Size,
            descending: true,
        };
        assert_eq!(o.to_config(), "size-desc");
        assert_eq!(SortOrder::from_config("size-desc"), Some(o));
        assert_eq!(
            SortOrder::from_config("modified").unwrap().key,
            SortKey::Modified
        );
        assert_eq!(SortOrder::from_config("nope"), None);
    }

    #[test]
    fn numbers_compare_by_value() {
        let mut v = vec!["file10", "file2", "file1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["file1", "file2", "file10"]);
    }

    #[test]
    fn case_insensitive() {
        let mut v = vec!["banana", "Apple", "cherry"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Apple", "banana", "cherry"]);
    }

    #[test]
    fn order_is_total() {
        assert_ne!(natural_cmp("a", "A"), Ordering::Equal);
        assert_ne!(natural_cmp("01", "1"), Ordering::Equal);
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
    }
}
