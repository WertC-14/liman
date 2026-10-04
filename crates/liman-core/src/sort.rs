//! Sorting like GUI file managers: folders first, then case-insensitive "natural" order
//! (`file2` before `file10`).

use std::cmp::Ordering;

use crate::Entry;

pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
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
