//! Color themes. One palette for everything, so the three views (ADR 0003), the panels and the
//! terminal stay consistent. `t` cycles the themes at run time; the choice is saved in the config.

use std::sync::atomic::{AtomicUsize, Ordering};

use liman_core::FileType;
use ratatui::style::Color;

pub struct Palette {
    pub name: &'static str,
    pub bg: Color,
    pub bar_bg: Color,
    pub selected_bg: Color,
    /// Rows marked with Space for a multi-item operation.
    pub marked_bg: Color,
    pub fg: Color,
    pub dim: Color,
    /// Frames of panels without focus.
    pub border: Color,
    /// One color per file type, in `FileType` order (Folder, Code, Config, Text, Pdf, Document,
    /// Spreadsheet, Presentation, Archive, Image, Video, Audio, Other).
    pub types: [Color; 13],
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

pub const THEMES: [Palette; 5] = [
    Palette {
        name: "liman",
        bg: rgb(21, 22, 28),
        bar_bg: rgb(32, 33, 41),
        selected_bg: rgb(52, 54, 66),
        marked_bg: rgb(34, 52, 84),
        fg: rgb(230, 230, 235),
        dim: rgb(150, 150, 160),
        border: rgb(70, 72, 86),
        // inspired by the kora icon theme (fm-research spike kutu.py)
        types: [
            rgb(64, 156, 230),
            rgb(150, 90, 200),
            rgb(110, 130, 170),
            rgb(130, 140, 160),
            rgb(220, 50, 50),
            rgb(60, 110, 200),
            rgb(40, 170, 90),
            rgb(235, 110, 40),
            rgb(240, 200, 70),
            rgb(40, 170, 170),
            rgb(200, 60, 140),
            rgb(240, 100, 160),
            rgb(100, 105, 115),
        ],
    },
    Palette {
        name: "nord",
        bg: rgb(46, 52, 64),
        bar_bg: rgb(59, 66, 82),
        selected_bg: rgb(67, 76, 94),
        marked_bg: rgb(76, 86, 106),
        fg: rgb(236, 239, 244),
        dim: rgb(160, 168, 184),
        border: rgb(76, 86, 106),
        types: [
            rgb(136, 192, 208),
            rgb(180, 142, 173),
            rgb(129, 161, 193),
            rgb(216, 222, 233),
            rgb(191, 97, 106),
            rgb(94, 129, 172),
            rgb(163, 190, 140),
            rgb(208, 135, 112),
            rgb(235, 203, 139),
            rgb(143, 188, 187),
            rgb(180, 142, 173),
            rgb(208, 135, 112),
            rgb(118, 128, 148),
        ],
    },
    Palette {
        name: "gruvbox",
        bg: rgb(40, 40, 40),
        bar_bg: rgb(50, 48, 47),
        selected_bg: rgb(80, 73, 69),
        marked_bg: rgb(69, 82, 84),
        fg: rgb(235, 219, 178),
        dim: rgb(168, 153, 132),
        border: rgb(102, 92, 84),
        types: [
            rgb(131, 165, 152),
            rgb(211, 134, 155),
            rgb(142, 192, 124),
            rgb(213, 196, 161),
            rgb(251, 73, 52),
            rgb(131, 165, 152),
            rgb(184, 187, 38),
            rgb(254, 128, 25),
            rgb(250, 189, 47),
            rgb(142, 192, 124),
            rgb(211, 134, 155),
            rgb(254, 128, 25),
            rgb(146, 131, 116),
        ],
    },
    Palette {
        name: "catppuccin",
        bg: rgb(30, 30, 46),
        bar_bg: rgb(24, 24, 37),
        selected_bg: rgb(49, 50, 68),
        marked_bg: rgb(54, 58, 89),
        fg: rgb(205, 214, 244),
        dim: rgb(147, 153, 178),
        border: rgb(69, 71, 90),
        types: [
            rgb(137, 180, 250),
            rgb(203, 166, 247),
            rgb(116, 199, 236),
            rgb(186, 194, 222),
            rgb(243, 139, 168),
            rgb(137, 220, 235),
            rgb(166, 227, 161),
            rgb(250, 179, 135),
            rgb(249, 226, 175),
            rgb(148, 226, 213),
            rgb(245, 194, 231),
            rgb(242, 205, 205),
            rgb(127, 132, 156),
        ],
    },
    Palette {
        name: "light",
        bg: rgb(250, 250, 251),
        bar_bg: rgb(236, 237, 241),
        selected_bg: rgb(212, 225, 245),
        marked_bg: rgb(222, 232, 214),
        fg: rgb(32, 33, 38),
        dim: rgb(110, 112, 122),
        border: rgb(196, 198, 206),
        types: [
            rgb(28, 113, 216),
            rgb(129, 61, 156),
            rgb(70, 90, 140),
            rgb(90, 96, 110),
            rgb(192, 28, 40),
            rgb(26, 95, 180),
            rgb(38, 162, 105),
            rgb(198, 70, 0),
            rgb(181, 131, 0),
            rgb(0, 128, 128),
            rgb(165, 29, 109),
            rgb(196, 50, 120),
            rgb(119, 118, 123),
        ],
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn palette() -> &'static Palette {
    &THEMES[CURRENT.load(Ordering::Relaxed) % THEMES.len()]
}

/// Switches to the theme called `name`; false if there is none.
pub fn set_by_name(name: &str) -> bool {
    match THEMES
        .iter()
        .position(|t| t.name.eq_ignore_ascii_case(name))
    {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// Switches to the next theme and returns its name.
pub fn cycle() -> &'static str {
    let next = (CURRENT.load(Ordering::Relaxed) + 1) % THEMES.len();
    CURRENT.store(next, Ordering::Relaxed);
    THEMES[next].name
}

pub fn bg() -> Color {
    palette().bg
}
pub fn bar_bg() -> Color {
    palette().bar_bg
}
pub fn selected_bg() -> Color {
    palette().selected_bg
}
pub fn marked_bg() -> Color {
    palette().marked_bg
}
pub fn fg() -> Color {
    palette().fg
}
pub fn dim() -> Color {
    palette().dim
}
pub fn border() -> Color {
    palette().border
}

/// Accent for focused frames and the current place: the folder color.
pub fn accent() -> Color {
    type_color(FileType::Folder)
}

pub fn type_color(file_type: FileType) -> Color {
    let i = match file_type {
        FileType::Folder => 0,
        FileType::Code => 1,
        FileType::Config => 2,
        FileType::Text => 3,
        FileType::Pdf => 4,
        FileType::Document => 5,
        FileType::Spreadsheet => 6,
        FileType::Presentation => 7,
        FileType::Archive => 8,
        FileType::Image => 9,
        FileType::Video => 10,
        FileType::Audio => 11,
        FileType::Other => 12,
    };
    palette().types[i]
}

/// Dark or light text, whichever reads better on `bg`.
pub fn text_on(bg: Color) -> Color {
    match bg {
        Color::Rgb(r, g, b) => {
            let luminance = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
            if luminance > 150.0 {
                Color::Rgb(20, 20, 24)
            } else {
                Color::Rgb(240, 240, 244)
            }
        }
        _ => fg(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yellow_gets_dark_text_red_gets_light_text() {
        let yellow = THEMES[0].types[8];
        let red = THEMES[0].types[4];
        assert_eq!(text_on(yellow), Color::Rgb(20, 20, 24));
        assert_eq!(text_on(red), Color::Rgb(240, 240, 244));
    }

    #[test]
    fn themes_have_unique_names() {
        let mut names: Vec<_> = THEMES.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), THEMES.len());
    }
}
