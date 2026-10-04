//! Times sorting 20 000 entries by name: `cargo run --release -p liman-core --example sortbench`
use liman_core::sort::{SortOrder, sort_with};
use liman_core::{Entry, FileType};
use std::time::Instant;

fn main() {
    let mut entries: Vec<Entry> = (0..20_000u32)
        .map(|i| {
            let name = format!(
                "{}Photo_{}-IMG{:05}.jpg",
                ["", "a", "Z"][(i % 3) as usize],
                i.wrapping_mul(7919) % 1000,
                i
            );
            Entry {
                path: name.clone().into(),
                name,
                is_dir: i % 50 == 0,
                is_symlink: false,
                size: u64::from(i),
                item_count: None,
                modified: None,
                file_type: FileType::Image,
                special: None,
            }
        })
        .collect();
    for key in liman_core::sort::SortKey::ALL {
        let t = Instant::now();
        sort_with(
            &mut entries,
            SortOrder {
                key,
                descending: false,
            },
        );
        println!("{:>8}: {:?}", key.name(), t.elapsed());
    }
}
