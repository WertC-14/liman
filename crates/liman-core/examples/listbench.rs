//! Times `list_dir` and the child count pass on a folder:
//! `cargo run --release -p liman-core --example listbench -- DIR`
use liman_core::{ListOptions, list_dir, listing::count_children};
use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).expect("folder");
    let dir = std::path::Path::new(&dir);
    let t = Instant::now();
    let entries = list_dir(dir, ListOptions::default()).unwrap();
    let listed = t.elapsed();
    let t = Instant::now();
    let counted = entries
        .iter()
        .filter(|e| e.is_dir)
        .filter_map(|e| count_children(&e.path, ListOptions::default()))
        .count();
    println!(
        "{} entries listed in {:?}; {counted} folders counted in {:?}",
        entries.len(),
        listed,
        t.elapsed()
    );
}
