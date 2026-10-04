//! Color depth. Themes are written in 24-bit RGB; terminals without truecolor (some tmux and SSH
//! setups, the Linux console) get the nearest of the 256 xterm colors instead.

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::buffer::Buffer;
use ratatui::style::Color;

static TRUECOLOR: AtomicBool = AtomicBool::new(true);

/// Whether frames go out in 24-bit color (set once at start-up).
pub fn truecolor() -> bool {
    TRUECOLOR.load(Ordering::Relaxed)
}

pub fn set_truecolor(on: bool) {
    TRUECOLOR.store(on, Ordering::Relaxed);
}

/// Guesses truecolor support from the environment. `COLORTERM` is the standard signal; some
/// terminals are known to support it even when the variable is lost (e.g. over SSH).
pub fn detect_truecolor(
    colorterm: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
) -> bool {
    if colorterm.is_some_and(|c| c == "truecolor" || c == "24bit") {
        return true;
    }
    let term = term.unwrap_or_default();
    if [
        "kitty",
        "alacritty",
        "wezterm",
        "foot",
        "ghostty",
        "-direct",
    ]
    .iter()
    .any(|t| term.contains(t))
    {
        return true;
    }
    term_program.is_some_and(|p| ["iTerm.app", "WezTerm", "vscode", "ghostty"].contains(&p))
}

/// The six levels of each channel in the xterm 6×6×6 color cube (indices 16–231).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Nearest 256-color index for an RGB color; other colors stay as they are.
pub fn to_256(color: Color) -> Color {
    let Color::Rgb(r, g, b) = color else {
        return color;
    };
    let level = |v: u8| {
        CUBE.iter()
            .enumerate()
            .min_by_key(|(_, c)| (i32::from(**c) - i32::from(v)).abs())
            .map_or(0, |(i, _)| i)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (CUBE[ri], CUBE[gi], CUBE[bi]);
    let cube_index = 16 + 36 * ri + 6 * gi + bi;

    // Gray ramp 232–255: 8, 18, ..., 238.
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gray_step = (avg.saturating_sub(3) / 10).min(23);
    let gray = (8 + 10 * gray_step) as u8;
    let gray_index = 232 + gray_step as usize;

    let dist = |(cr, cg, cb): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(cr, r) + d(cg, g) + d(cb, b)
    };
    let index = if dist((gray, gray, gray)) < dist(cube) {
        gray_index
    } else {
        cube_index
    };
    Color::Indexed(index as u8)
}

/// Rewrites every RGB color of a finished frame to the 256-color palette.
pub fn downsample(buf: &mut Buffer) {
    for cell in &mut buf.content {
        cell.fg = to_256(cell.fg);
        cell.bg = to_256(cell.bg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        assert!(detect_truecolor(
            Some("truecolor"),
            Some("tmux-256color"),
            None
        ));
        assert!(detect_truecolor(None, Some("xterm-kitty"), None));
        assert!(detect_truecolor(
            None,
            Some("xterm-256color"),
            Some("WezTerm")
        ));
        assert!(!detect_truecolor(None, Some("xterm-256color"), None));
        assert!(!detect_truecolor(None, Some("linux"), None));
    }

    #[test]
    fn nearest_256_colors() {
        assert_eq!(to_256(Color::Rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(to_256(Color::Rgb(255, 0, 0)), Color::Indexed(196));
        assert_eq!(to_256(Color::Rgb(255, 255, 255)), Color::Indexed(231));
        assert_eq!(to_256(Color::Rgb(30, 30, 34)), Color::Indexed(234)); // dark gray → ramp
        assert_eq!(to_256(Color::Reset), Color::Reset);
    }
}
