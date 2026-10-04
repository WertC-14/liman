//! The entries a list or grid shows: the listing plus the display order (filter and sort
//! applied by the app). A view over borrowed data, so a frame builds no per-entry vector.

use liman_core::Entry;

#[derive(Clone, Copy)]
pub struct Rows<'a> {
    entries: &'a [Entry],
    /// Indices into `entries` in display order; `None` = all entries as they are.
    order: Option<&'a [usize]>,
}

impl<'a> Rows<'a> {
    pub fn all(entries: &'a [Entry]) -> Self {
        Self {
            entries,
            order: None,
        }
    }

    pub fn ordered(entries: &'a [Entry], order: &'a [usize]) -> Self {
        Self {
            entries,
            order: Some(order),
        }
    }

    pub fn len(&self) -> usize {
        self.order.map_or(self.entries.len(), <[usize]>::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, row: usize) -> Option<&'a Entry> {
        match self.order {
            Some(order) => order.get(row).and_then(|&i| self.entries.get(i)),
            None => self.entries.get(row),
        }
    }

    pub fn iter(self) -> impl Iterator<Item = &'a Entry> {
        (0..self.len()).filter_map(move |row| self.get(row))
    }
}
