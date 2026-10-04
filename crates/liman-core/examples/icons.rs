//! Shows which icon theme liman finds and draws a few icons with half blocks.
//!
//!     cargo run -p liman-core --example icons
//!     LIMAN_ICON_THEME=kora cargo run -p liman-core --example icons

use std::path::PathBuf;

use liman_core::icons::{ICON_SIZE, IconTheme, detect_theme_name, render_svg};

fn main() {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let name = detect_theme_name(&home);
    println!("theme: {}", name.as_deref().unwrap_or("(none)"));
    let theme = IconTheme::load(name.as_deref(), &home);

    for names in [
        &["folder-download", "folder"][..],
        &["folder"],
        &["application-pdf"],
        &["text-rust", "text-x-rust", "text-x-generic"],
        &["application-vnd.ms-excel", "x-office-spreadsheet"],
    ] {
        let Some(path) = theme.find(names) else {
            println!("{:?}: not found", names[0]);
            continue;
        };
        println!("{} -> {}", names[0], path.display());
        let Some(px) = std::fs::read(&path)
            .ok()
            .and_then(|svg| render_svg(&svg, ICON_SIZE))
        else {
            println!("  (could not render)");
            continue;
        };
        for y in (0..px.size).step_by(2) {
            let mut line = String::new();
            for x in 0..px.size {
                let [tr, tg, tb, ta] = px.get(x, y);
                let [br, bg, bb, ba] = px.get(x, y + 1);
                // Blend with a dark background, like the app does.
                let mix = |c: u8, a: u8| {
                    (u16::from(c) * u16::from(a) / 255 + 21 * (255 - u16::from(a)) / 255) as u8
                };
                line += &format!(
                    "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀",
                    mix(tr, ta),
                    mix(tg, ta),
                    mix(tb, ta),
                    mix(br, ba),
                    mix(bg, ba),
                    mix(bb, ba)
                );
            }
            println!("  {line}\x1b[0m");
        }
    }
}
