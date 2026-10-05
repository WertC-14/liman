//! What the preview panel shows for a path (F3). Blocking: build it on a worker thread.
//!
//! Text and code show their first lines, folders a summary of what is inside, images are scaled
//! down to pixels the UI draws as half blocks, PDFs and archives use `pdftotext`, `tar` and
//! `unzip` when they are installed. Anything else gets only the header (type, size, date).

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::FileType;
use crate::file_type::Tally;
use crate::i18n::{tr, trf};
use crate::sort::natural_cmp;

/// At most this many lines of text, folder names or archive members are kept.
pub const MAX_LINES: usize = 20_000;
/// Only the start of a text file is read.
const TEXT_BYTES: usize = 1024 * 1024;
/// A folder summary stops counting here (a huge folder must not stall the preview).
const FOLDER_LIMIT: usize = 10_000;

#[derive(Debug, Clone)]
pub struct Preview {
    pub path: PathBuf,
    pub file_type: FileType,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Unix permission bits (`0o644`), of the target for a link.
    pub mode: Option<u32>,
    /// Where a symbolic link points (as written in the link).
    pub link_target: Option<PathBuf>,
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Lines of a text file, PDF text or an archive listing. `source` names where they came from
    /// when it is not the file itself ("pdftotext", "tar").
    Text {
        lines: Vec<String>,
        more: bool,
        source: Option<&'static str>,
    },
    Folder {
        /// Children counted (stops at the limit, then `more` is set).
        total: usize,
        more: bool,
        /// How many children of each type, most common first.
        by_type: Vec<(FileType, usize)>,
        /// Child names (folders first), `true` for folders.
        names: Vec<(String, bool)>,
    },
    /// RGBA pixels, `width` × `height`, already scaled to fit the panel (two pixels per cell, one above the other).
    Image {
        width: u32,
        height: u32,
        original: (u32, u32),
        pixels: Vec<[u8; 4]>,
    },
    /// The start of a binary file as a hex dump, one line per 16 bytes (`xxd` style).
    Hex {
        lines: Vec<String>,
        /// The file is longer than the dump.
        more: bool,
    },
    /// Only the header is shown; the text says why (tool missing, unreadable...).
    Note(String),
}

/// Builds the preview of `path` for a panel of `cols` × `rows` cells.
pub fn build(path: &Path, cols: u16, rows: u16) -> Preview {
    let meta = fs::metadata(path).ok();
    let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
    let file_type = FileType::from_path(path, is_dir);
    let content = match &meta {
        None => Content::Note(tr("Cannot read this item").into()),
        Some(_) if is_dir => folder(path),
        Some(_) => file(path, file_type, cols, rows),
    };
    use std::os::unix::fs::PermissionsExt;
    Preview {
        path: path.to_path_buf(),
        file_type,
        size: meta.as_ref().filter(|m| !m.is_dir()).map_or(0, |m| m.len()),
        mode: meta.as_ref().map(|m| m.permissions().mode() & 0o7777),
        modified: meta.and_then(|m| m.modified().ok()),
        link_target: fs::read_link(path).ok(),
        content,
    }
}

fn file(path: &Path, file_type: FileType, cols: u16, rows: u16) -> Content {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match file_type {
        FileType::Image if ext != "svg" => image(path, cols, rows),
        FileType::Pdf => command_lines("pdftotext", &["-l", "3", "-layout"], path, &["-"])
            .unwrap_or_else(|| {
                Content::Note(tr("Install pdftotext (poppler) to see the text").into())
            }),
        FileType::Archive => archive(path, &ext),
        _ => text(path),
    }
}

/// The first lines of a text file; a NUL byte in the start means it is binary.
fn text(path: &Path) -> Content {
    let mut buf = Vec::with_capacity(TEXT_BYTES);
    let read = fs::File::open(path).and_then(|f| f.take(TEXT_BYTES as u64).read_to_end(&mut buf));
    if let Err(e) = read {
        return Content::Note(trf("Cannot read: {}", &[&e]));
    }
    if buf.contains(&0) {
        return hex_dump(&buf);
    }
    let truncated = buf.len() == TEXT_BYTES;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(clean_line).collect();
    // The last line may be cut in the middle when we stopped reading.
    if truncated {
        lines.pop();
    }
    let more = truncated || lines.len() > MAX_LINES;
    lines.truncate(MAX_LINES);
    Content::Text {
        lines,
        more,
        source: None,
    }
}

/// Bytes of a binary file shown as hex.
const HEX_BYTES: usize = 4096;

/// `00000000  7f 45 4c 46 02 01 01 00  00 00 00 00 00 00 00 00  |.ELF............|`
fn hex_dump(bytes: &[u8]) -> Content {
    use std::fmt::Write as _;
    let shown = &bytes[..bytes.len().min(HEX_BYTES)];
    let lines = shown
        .chunks(16)
        .enumerate()
        .map(|(i, row)| {
            let mut line = format!("{:08x} ", i * 16);
            for (k, b) in row.iter().enumerate() {
                let gap = if k == 8 { "  " } else { " " };
                let _ = write!(line, "{gap}{b:02x}");
            }
            // Pad a short last row so the text column lines up.
            let missing = 16 - row.len();
            line.push_str(&" ".repeat(missing * 3 + usize::from(row.len() <= 8)));
            line.push_str("  |");
            line.extend(row.iter().map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    char::from(b)
                } else {
                    '.'
                }
            }));
            line.push('|');
            line
        })
        .collect();
    Content::Hex {
        lines,
        more: bytes.len() > HEX_BYTES,
    }
}

/// Tabs become spaces and control characters are dropped, so a line never moves the cursor.
fn clean_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn folder(path: &Path) -> Content {
    let Ok(read) = fs::read_dir(path) else {
        return Content::Note(tr("Cannot open this folder").into());
    };
    let mut names: Vec<(String, bool)> = Vec::new();
    let mut more = false;
    for item in read.flatten() {
        if names.len() == FOLDER_LIMIT {
            more = true;
            break;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // `file_type` comes from the directory entry (no extra stat); follow symlinks only when needed.
        let is_dir = match item.file_type() {
            Ok(t) if t.is_symlink() => item.path().is_dir(),
            Ok(t) => t.is_dir(),
            Err(_) => false,
        };
        names.push((name, is_dir));
    }
    let mut tally = Tally::default();
    for (name, is_dir) in &names {
        tally.add(FileType::from_path(Path::new(name), *is_dir));
    }
    let mut counts = tally.into_vec();
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| natural_cmp(&a.0, &b.0)));
    let total = names.len();
    names.truncate(MAX_LINES);
    Content::Folder {
        total,
        more,
        by_type: counts,
        names,
    }
}

/// Decodes and scales an image to fit `cols` × `rows * 2` pixels.
fn image(path: &Path, cols: u16, rows: u16) -> Content {
    let decoded = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())
        .and_then(|r| r.decode().map_err(|e| e.to_string()));
    let img = match decoded {
        Ok(img) => img,
        Err(e) => return Content::Note(trf("Cannot show this image: {}", &[&e])),
    };
    let original = (img.width(), img.height());
    let (max_w, max_h) = (u32::from(cols.max(1)), u32::from(rows.max(1)) * 2);
    // Never scale up: a 16 px icon stays 16 px.
    let small = if original.0 > max_w || original.1 > max_h {
        img.thumbnail(max_w, max_h)
    } else {
        img
    };
    let rgba = small.to_rgba8();
    Content::Image {
        width: rgba.width(),
        height: rgba.height(),
        original,
        pixels: rgba.pixels().map(|p| p.0).collect(),
    }
}

fn archive(path: &Path, ext: &str) -> Content {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let tar_like = ext == "tar" || ext == "tgz" || name.contains(".tar.");
    let result = if ext == "zip" {
        command_lines("unzip", &["-Z1"], path, &[])
    } else if tar_like {
        command_lines("tar", &["-tf"], path, &[])
    } else {
        None
    };
    result.unwrap_or_else(|| Content::Note(tr("Archive (no listing tool for this format)").into()))
}

/// Runs `program args path tail` and keeps the first lines of its output.
/// `None` when the program is missing or fails without printing anything.
fn command_lines(
    program: &'static str,
    args: &[&str],
    path: &Path,
    tail: &[&str],
) -> Option<Content> {
    let mut child = Command::new(program)
        .args(args)
        .arg(path)
        .args(tail)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let mut lines = Vec::new();
    let mut more = false;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if lines.len() == MAX_LINES {
            more = true;
            break;
        }
        lines.push(clean_line(&line));
    }
    // Stop a long listing early; we have what we show.
    if more {
        let _ = child.kill();
    }
    let ok = child.wait().is_ok_and(|s| s.success()) || more;
    if !ok && lines.is_empty() {
        return None;
    }
    // pdftotext ends pages with form feeds and trailing blank lines.
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    Some(Content::Text {
        lines,
        more,
        source: Some(program),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("liman-preview-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn text_lines_with_tabs_and_controls_cleaned() {
        let dir = temp("text");
        let file = dir.join("a.rs");
        fs::write(&file, "fn main() {\n\tprintln!(\"\x1b[31mhi\");\n}\n").unwrap();
        let p = build(&file, 40, 10);
        assert_eq!(p.file_type, FileType::Code);
        let Content::Text { lines, more, .. } = p.content else {
            panic!("not text");
        };
        assert_eq!(lines, ["fn main() {", "    println!(\"[31mhi\");", "}"]);
        assert!(!more);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn binary_files_get_a_hex_dump() {
        let dir = temp("bin");
        let file = dir.join("x.bin");
        let mut bytes = b"\x7fELF".to_vec();
        bytes.extend(0u8..14);
        fs::write(&file, &bytes).unwrap();
        let preview = build(&file, 40, 10);
        let Content::Hex { lines, more } = preview.content else {
            panic!("expected a hex dump")
        };
        assert!(!more);
        assert_eq!(
            lines,
            [
                "00000000  7f 45 4c 46 00 01 02 03  04 05 06 07 08 09 0a 0b  |.ELF............|",
                "00000010  0c 0d                                             |..|",
            ]
        );
        assert_eq!(preview.mode.map(|m| m & 0o600), Some(0o600)); // we wrote it, we can read it
        assert_eq!(preview.link_target, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folder_summary_counts_types_and_lists_folders_first() {
        let dir = temp("folder");
        fs::create_dir(dir.join("sub")).unwrap();
        for name in ["b.txt", "a.txt", "c.png", ".hidden"] {
            fs::write(dir.join(name), "x").unwrap();
        }
        let Content::Folder {
            total,
            by_type,
            names,
            ..
        } = build(&dir, 40, 10).content
        else {
            panic!("not a folder");
        };
        assert_eq!(total, 4);
        assert_eq!(by_type[0], (FileType::Text, 2));
        let names: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["sub", "a.txt", "b.txt", "c.png"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn images_are_scaled_to_fit_two_pixels_per_row() {
        let dir = temp("img");
        let file = dir.join("p.png");
        image::RgbaImage::from_pixel(100, 50, image::Rgba([255, 0, 0, 255]))
            .save(&file)
            .unwrap();
        let Content::Image {
            width,
            height,
            original,
            pixels,
        } = build(&file, 20, 20).content
        else {
            panic!("not an image");
        };
        assert_eq!(original, (100, 50));
        assert_eq!((width, height), (20, 10));
        assert_eq!(pixels[0], [255, 0, 0, 255]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
