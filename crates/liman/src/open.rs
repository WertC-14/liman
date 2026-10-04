//! Opening files: the desktop's default app when there is a graphical display,
//! `$VISUAL` / `$EDITOR` for text files otherwise (e.g. over SSH).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use liman_core::{Entry, FileType};

#[derive(Debug, PartialEq, Eq)]
pub enum OpenPlan {
    /// `xdg-open` in the background; the TUI keeps running.
    Desktop(PathBuf),
    /// A terminal editor; the TUI has to step aside while it runs.
    Editor { program: String, path: PathBuf },
    /// Nothing sensible to do; the message explains why.
    Unavailable(String),
}

/// Environment the decision depends on (separate so tests do not touch the real environment).
pub struct Env {
    pub has_display: bool,
    pub editor: Option<String>,
}

impl Env {
    pub fn current() -> Self {
        let set = |k| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        Self {
            has_display: set("DISPLAY") || set("WAYLAND_DISPLAY"),
            editor: ["VISUAL", "EDITOR"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())),
        }
    }
}

pub fn plan(entry: &Entry, env: &Env) -> OpenPlan {
    if env.has_display {
        return OpenPlan::Desktop(entry.path.clone());
    }
    let text_like = matches!(
        entry.file_type,
        FileType::Code | FileType::Config | FileType::Text | FileType::Other
    );
    if text_like {
        return OpenPlan::Editor {
            program: env.editor.clone().unwrap_or_else(|| "vi".into()),
            path: entry.path.clone(),
        };
    }
    OpenPlan::Unavailable(liman_core::i18n::trf(
        "No graphical display (SSH?): cannot open “{}” here",
        &[&entry.name],
    ))
}

/// Starts `xdg-open` detached from our terminal. A thread waits for it so no zombie is left behind.
pub fn open_desktop(path: &Path) -> std::io::Result<()> {
    let mut child = Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Runs the editor in the foreground (the caller must have released the terminal).
/// `program` may contain arguments, e.g. `code --wait`.
pub fn run_editor(program: &str, path: &Path) -> std::io::Result<()> {
    let mut parts = program.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let status = Command::new(bin).args(parts).arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("{bin} exited with {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        let path = PathBuf::from("/d").join(name);
        Entry {
            name: name.into(),
            file_type: FileType::from_path(&path, false),
            path,
            is_dir: false,
            is_symlink: false,
            special: None,
            size: 1,
            item_count: None,
            modified: None,
        }
    }

    #[test]
    fn desktop_when_there_is_a_display() {
        let env = Env {
            has_display: true,
            editor: None,
        };
        assert_eq!(
            plan(&entry("a.pdf"), &env),
            OpenPlan::Desktop("/d/a.pdf".into())
        );
    }

    #[test]
    fn editor_for_text_without_a_display() {
        let env = Env {
            has_display: false,
            editor: Some("nvim".into()),
        };
        assert_eq!(
            plan(&entry("main.rs"), &env),
            OpenPlan::Editor {
                program: "nvim".into(),
                path: "/d/main.rs".into()
            }
        );
        let no_editor = Env {
            has_display: false,
            editor: None,
        };
        assert!(matches!(
            plan(&entry("notes.md"), &no_editor),
            OpenPlan::Editor { program, .. } if program == "vi"
        ));
    }

    #[test]
    fn binary_files_without_a_display_are_refused() {
        let env = Env {
            has_display: false,
            editor: Some("nvim".into()),
        };
        assert!(matches!(
            plan(&entry("movie.mp4"), &env),
            OpenPlan::Unavailable(_)
        ));
    }
}
