//! Colors. One place, so the three views (ADR 0003) share the same type colors.

use liman_core::FileType;
use ratatui::style::Color;

pub const BG: Color = Color::Rgb(21, 22, 28);
pub const BAR_BG: Color = Color::Rgb(32, 33, 41);
pub const SELECTED_BG: Color = Color::Rgb(52, 54, 66);
pub const FG: Color = Color::Rgb(230, 230, 235);
pub const DIM: Color = Color::Rgb(150, 150, 160);

/// Type color, inspired by the kora icon theme the user runs in Nautilus (fm-research spike `kutu.py`).
pub fn type_color(file_type: FileType) -> Color {
    match file_type {
        FileType::Folder => Color::Rgb(64, 156, 230),
        FileType::Code => Color::Rgb(150, 90, 200),
        FileType::Config => Color::Rgb(110, 130, 170),
        FileType::Text => Color::Rgb(130, 140, 160),
        FileType::Pdf => Color::Rgb(220, 50, 50),
        FileType::Document => Color::Rgb(60, 110, 200),
        FileType::Spreadsheet => Color::Rgb(40, 170, 90),
        FileType::Presentation => Color::Rgb(235, 110, 40),
        FileType::Archive => Color::Rgb(240, 200, 70),
        FileType::Image => Color::Rgb(40, 170, 170),
        FileType::Video => Color::Rgb(200, 60, 140),
        FileType::Audio => Color::Rgb(240, 100, 160),
        FileType::Other => Color::Rgb(100, 105, 115),
    }
}

/// Dark or light text, whichever reads better on `bg`.
pub fn text_on(bg: Color) -> Color {
    match bg {
        Color::Rgb(r, g, b) => {
            let luminance = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
            if luminance > 150.0 {
                Color::Rgb(20, 20, 24)
            } else {
                FG
            }
        }
        _ => FG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yellow_gets_dark_text_red_gets_light_text() {
        assert_eq!(
            text_on(type_color(FileType::Archive)),
            Color::Rgb(20, 20, 24)
        );
        assert_eq!(text_on(type_color(FileType::Pdf)), FG);
    }
}
