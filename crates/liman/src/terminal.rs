//! The embedded shell for terminal mode (Dolphin's F4 panel, Midnight Commander's Ctrl+O).
//!
//! The user's shell runs in a pseudo-terminal (`portable-pty`). A reader thread forwards its output
//! to the main loop as [`AppEvent::TermOutput`]; `vt100` turns the bytes into a grid of cells that the
//! UI draws. Keys typed while the terminal has focus are encoded back into bytes and written to the pty.
//!
//! Folder sync works both ways: the shell's current directory is read from `/proc/<pid>/cwd`
//! after output; going to a folder in the file view sends `cd` to the shell when it is idle at its prompt.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::event::AppEvent;

/// Start-up output counts as finished after this much silence.
const QUIET: Duration = Duration::from_millis(300);
/// A prompt may span a few lines; the cursor this close to the top means the screen is clean.
const MAX_PROMPT_LINES: u16 = 2;

/// Lines kept above the screen (not shown yet, but programs like `less` expect it to exist).
const SCROLLBACK: usize = 1000;

pub struct Terminal {
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
    /// Clearing the start-up output (greeting, fastfetch, ...).
    greeting: Greeting,
    /// The user typed something: leave the screen alone from now on.
    user_typed: bool,
    /// Text to type at the first prompt (sent before the shell was ready, it would be lost).
    pending_input: Vec<u8>,
    /// Folder the file view wants the shell in, waiting for the previous `cd` to finish.
    pending_cd: Option<PathBuf>,
    /// Unfinished escape sequence at the end of the last chunk (queries can be split across reads).
    query_carry: Vec<u8>,
    /// The `cd` sent last and when; the next one waits until the shell is there (or 2 s passed).
    cd_in_flight: Option<(PathBuf, Instant)>,
    /// Output of the command line the user is running (from Enter until the prompt is back).
    capture: Option<Capture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Greeting {
    /// Nothing sent yet.
    Waiting,
    /// Ctrl+L sent; done once the prompt is at the top after a quiet moment.
    Sent,
    Done,
}

/// A `cd` the shell has not carried out after this long (no permission, ...) is given up on.
const CD_TIMEOUT: Duration = Duration::from_secs(2);

struct Capture {
    command: String,
    cwd: Option<PathBuf>,
    bytes: Vec<u8>,
}

/// A finished command line and what it printed (escape sequences still in).
pub struct CommandOutput {
    pub command: String,
    pub cwd: PathBuf,
    pub bytes: Vec<u8>,
}

/// Command output kept for results (enough for thousands of paths).
const MAX_CAPTURE: usize = 4 * 1024 * 1024;

impl Terminal {
    /// Starts the user's shell (`$SHELL`, else `/bin/sh`) in `cwd` with a `rows`×`cols` screen.
    pub fn spawn(
        cwd: &Path,
        rows: u16,
        cols: u16,
        tx: Sender<AppEvent>,
    ) -> anyhow_lite::Result<Self> {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        Self::spawn_shell(&shell, cwd, rows, cols, tx)
    }

    pub fn spawn_shell(
        shell: &str,
        cwd: &Path,
        rows: u16,
        cols: u16,
        tx: Sender<AppEvent>,
    ) -> anyhow_lite::Result<Self> {
        let (rows, cols) = (rows.max(2), cols.max(10));
        let pair = native_pty_system()
            .openpty(pty_size(rows, cols))
            .map_err(anyhow_lite::from)?;
        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd).map_err(anyhow_lite::from)?;
        drop(pair.slave); // the child holds its own copy; dropping ours lets EOF arrive when it exits

        let mut reader = pair.master.try_clone_reader().map_err(anyhow_lite::from)?;
        let writer = pair.master.take_writer().map_err(anyhow_lite::from)?;
        // The reader pings the quiet watcher after every chunk of output.
        let (ping, pings) = std::sync::mpsc::channel::<()>();
        let reader_tx = tx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = ping.send(());
                        if reader_tx
                            .send(AppEvent::TermOutput(buf[..n].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            let _ = reader_tx.send(AppEvent::TermExited);
        });
        // Quiet watcher: each burst of output followed by QUIET silence is reported once.
        // Used to clear the start-up greeting and to notice that a command has finished.
        // It sleeps in `recv` while the shell is idle and ends when the reader does.
        thread::spawn(move || {
            use std::sync::mpsc::RecvTimeoutError;
            while pings.recv().is_ok() {
                loop {
                    match pings.recv_timeout(QUIET) {
                        Ok(()) => continue, // still talking
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                if tx.send(AppEvent::TermQuiet).is_err() {
                    return;
                }
            }
        });

        Ok(Self {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK),
            master: pair.master,
            writer,
            child,
            size: (rows, cols),
            greeting: Greeting::Waiting,
            user_typed: false,
            pending_input: Vec::new(),
            pending_cd: None,
            cd_in_flight: None,
            query_carry: Vec::new(),
            capture: None,
        })
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        self.answer_queries(bytes);
        self.flush_cd();
        if let Some(capture) = &mut self.capture
            && capture.bytes.len() < MAX_CAPTURE
        {
            capture.bytes.extend_from_slice(bytes);
        }
    }

    /// Answers the questions programs ask their terminal. Shells like fish 4 ask at start-up
    /// (device attributes, version, background color, capabilities) and wait for the answer before
    /// showing the prompt; without replies they hang or time out. vt100 only draws, so we reply here.
    fn answer_queries(&mut self, bytes: &[u8]) {
        let mut data = std::mem::take(&mut self.query_carry);
        data.extend_from_slice(bytes);
        let mut replies: Vec<Vec<u8>> = Vec::new();
        let mut i = 0;
        while let Some(pos) = data[i..].iter().position(|&b| b == 0x1b) {
            let start = i + pos;
            let Some((len, reply)) = parse_query(&data[start..], self) else {
                // Possibly cut off: keep it for the next chunk (bounded, real queries are short).
                if data.len() - start < 64 {
                    self.query_carry = data[start..].to_vec();
                }
                break;
            };
            replies.extend(reply);
            i = start + len.max(1);
        }
        for reply in replies {
            self.send(&reply);
        }
    }

    /// The shell went quiet: clear the start-up greeting and send a waiting `cd`.
    ///
    /// The panel is small, so the greeting (fastfetch, MOTD, ...) is dropped with Ctrl+L at the
    /// prompt, which clears the screen and redraws just the prompt (bash, zsh, fish). A greeting may
    /// start printing after a pause, so Ctrl+L is repeated until the prompt stays at the top.
    pub fn on_quiet(&mut self) {
        self.flush_cd();
        if self.greeting == Greeting::Done
            || self.user_typed
            || !self.is_idle()
            || self.capture.is_some()
        {
            return;
        }
        if self.greeting == Greeting::Sent && self.screen().cursor_position().0 <= MAX_PROMPT_LINES
        {
            self.greeting = Greeting::Done;
            self.flush_pending_input();
        } else {
            self.greeting = Greeting::Sent;
            self.send(b"\x0c");
        }
    }

    /// Matches the pty and the parser to the area the UI draws into.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(2), cols.max(10));
        if (rows, cols) == self.size {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(pty_size(rows, cols));
        self.parser.screen_mut().set_size(rows, cols);
    }

    pub fn send_key(&mut self, key: KeyEvent) {
        self.user_typed = true;
        if key.code == KeyCode::Enter && self.is_idle() {
            // A command line is about to run: remember it and collect what it prints.
            self.capture = Some(Capture {
                command: self.cursor_line(),
                cwd: self.cwd(),
                bytes: Vec::new(),
            });
        }
        if let Some(bytes) = key_bytes(key, self.screen().application_cursor()) {
            self.send(&bytes);
        }
    }

    /// The text of the line the cursor is on, without the prompt (see [`strip_prompt`]).
    fn cursor_line(&self) -> String {
        let (row, _) = self.screen().cursor_position();
        let (_, cols) = self.screen().size();
        strip_prompt(&self.screen().contents_between(row, 0, row, cols))
    }

    /// The command started with Enter has finished (the shell is back at its prompt and quiet).
    pub fn take_finished_command(&mut self) -> Option<CommandOutput> {
        if !self.is_idle() {
            return None;
        }
        let capture = self.capture.take()?;
        Some(CommandOutput {
            command: capture.command,
            cwd: capture.cwd.or_else(|| self.cwd())?,
            bytes: capture.bytes,
        })
    }

    /// Types `text` on the command line; waits for the first prompt if the shell is still starting.
    pub fn type_text(&mut self, text: &str) {
        self.pending_input.extend_from_slice(text.as_bytes());
        if self.greeting == Greeting::Done || self.user_typed {
            self.flush_pending_input();
        }
    }

    fn flush_pending_input(&mut self) {
        if !self.pending_input.is_empty() {
            self.user_typed = true; // from now on the screen belongs to the user
            let text = std::mem::take(&mut self.pending_input);
            self.send(&text);
        }
    }

    pub fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// The shell's current directory (Linux: `/proc/<pid>/cwd`).
    pub fn cwd(&self) -> Option<PathBuf> {
        let pid = self.child.process_id()?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    /// True when the shell itself is in the foreground (sitting at its prompt), not a program it started.
    pub fn is_idle(&self) -> bool {
        let shell = self.child.process_id().and_then(|p| i32::try_from(p).ok());
        shell.is_some() && self.master.process_group_leader() == shell
    }

    /// Makes the shell change to `dir`, one `cd` at a time: while one is on its way, newer requests
    /// replace each other and only the last is sent. Never typed while a program runs or while the
    /// user has text on the command line. Each `cd` is followed by Ctrl+L, so the panel shows just
    /// the prompt in the new folder. The leading space keeps it out of history (`ignorespace`).
    pub fn cd(&mut self, dir: &Path) {
        self.pending_cd = Some(dir.to_path_buf());
        self.flush_cd();
    }

    /// True while a `cd` from the file view is waiting or on its way; the shell's folder may be an
    /// intermediate one then and must not be followed.
    pub fn is_syncing(&self) -> bool {
        self.pending_cd.is_some() || self.cd_in_flight.is_some()
    }

    fn flush_cd(&mut self) {
        if let Some((dir, sent)) = &self.cd_in_flight {
            if self.cwd().as_ref() != Some(dir) && sent.elapsed() < CD_TIMEOUT {
                return; // still on its way
            }
            self.cd_in_flight = None;
        }
        let Some(dir) = self.pending_cd.take() else {
            return;
        };
        if self.cwd().as_ref() == Some(&dir) {
            return;
        }
        if !self.is_idle() || !self.typed_text().is_empty() {
            self.pending_cd = Some(dir); // try again at the next quiet moment
            return;
        }
        let quoted = dir.to_string_lossy().replace('\'', r"'\''");
        self.send(format!(" cd -- '{quoted}'\r\x0c").as_bytes());
        self.cd_in_flight = Some((dir, Instant::now()));
    }

    /// What the user typed on the command line so far (left of the cursor, so fish's grey
    /// autosuggestion to the right does not count).
    pub fn typed_text(&self) -> String {
        let (row, col) = self.screen().cursor_position();
        strip_prompt(&self.screen().contents_between(row, 0, row, col))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// One escape sequence at the start of `seq`: its length and the reply it needs (if any).
/// `None` when the sequence is not complete yet.
fn parse_query(seq: &[u8], term: &Terminal) -> Option<(usize, Option<Vec<u8>>)> {
    let next = *seq.get(1)?;
    match next {
        // CSI: ESC [ params final
        b'[' => {
            let end = seq[2..].iter().position(|b| (0x40..=0x7e).contains(b))? + 2;
            let body = &seq[2..end];
            let reply = match (body, seq[end]) {
                (b"" | b"0", b'c') => Some(b"\x1b[?62;22c".to_vec()), // DA1: VT220 with color
                (b">0" | b">", b'q') => Some(b"\x1bP>|liman\x1b\\".to_vec()), // XTVERSION
                (b"5", b'n') => Some(b"\x1b[0n".to_vec()),            // status: OK
                (b"6", b'n') => {
                    let (row, col) = term.screen().cursor_position();
                    Some(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes())
                }
                _ => None, // e.g. ESC[?u (kitty keyboard): no reply means "not supported"
            };
            Some((end + 1, reply))
        }
        // OSC: ESC ] ... (BEL | ESC \)
        b']' => {
            let (end, term_len) = find_string_end(&seq[2..])?;
            let reply = match &seq[2..2 + end] {
                b"10;?" => Some(osc_color(10, liman_widgets::theme::fg())),
                b"11;?" => Some(osc_color(11, liman_widgets::theme::bg())),
                _ => None,
            };
            Some((2 + end + term_len, reply))
        }
        // DCS: ESC P ... ESC \  (XTGETTCAP "+q": answer "not available")
        b'P' => {
            let (end, term_len) = find_string_end(&seq[2..])?;
            let reply = seq[2..]
                .starts_with(b"+q")
                .then(|| b"\x1bP0+r\x1b\\".to_vec());
            Some((2 + end + term_len, reply))
        }
        _ => Some((2, None)),
    }
}

/// End of an OSC/DCS string: index of BEL or ESC \, and the terminator's length.
fn find_string_end(data: &[u8]) -> Option<(usize, usize)> {
    data.iter().enumerate().find_map(|(i, &b)| match b {
        0x07 => Some((i, 1)),
        0x1b if data.get(i + 1) == Some(&b'\\') => Some((i, 2)),
        _ => None,
    })
}

fn osc_color(code: u8, color: ratatui::style::Color) -> Vec<u8> {
    let (r, g, b) = match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    format!("\x1b]{code};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x1b\\").into_bytes()
}

/// A command line without its prompt: everything after the last prompt symbol (`❯ $ # > %`).
pub fn strip_prompt(line: &str) -> String {
    let line = line.trim();
    let after = line.rfind(['❯', '$', '#', '>', '%']).map_or(line, |i| {
        &line[i + line[i..].chars().next().map_or(1, char::len_utf8)..]
    });
    after.trim().to_string()
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Bytes a terminal sends for `key` (xterm conventions). `app_cursor`: the program asked for
/// "application cursor keys" (vim, less), which use `ESC O A` instead of `ESC [ A`.
pub fn key_bytes(key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let arrow = |c: char| {
        let prefix = if app_cursor { "\x1bO" } else { "\x1b[" };
        format!("{prefix}{c}").into_bytes()
    };
    let mut bytes = match key.code {
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => {
            vec![(c.to_ascii_lowercase() as u8) & 0x1f]
        }
        KeyCode::Char(' ') if ctrl => vec![0],
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => arrow('A'),
        KeyCode::Down => arrow('B'),
        KeyCode::Right => arrow('C'),
        KeyCode::Left => arrow('D'),
        KeyCode::Home => arrow('H'),
        KeyCode::End => arrow('F'),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(n @ 1..=4) => format!("\x1bO{}", (b'P' + n - 1) as char).into_bytes(),
        KeyCode::F(n) => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => return None,
            };
            format!("\x1b[{code}~").into_bytes()
        }
        _ => return None,
    };
    if alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// Tiny error type so callers get a readable message without pulling in `anyhow` ourselves.
pub mod anyhow_lite {
    pub type Result<T> = std::result::Result<T, String>;

    pub fn from<E: std::fmt::Display>(e: E) -> String {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn prompts_are_stripped_from_command_lines() {
        assert_eq!(strip_prompt("❯ find . -name x"), "find . -name x");
        assert_eq!(strip_prompt("user@host:~$ ls -1"), "ls -1");
        assert_eq!(strip_prompt("❯"), "");
        assert_eq!(strip_prompt("  fd rs  "), "fd rs");
    }

    #[test]
    fn start_up_queries_get_answers() {
        let (tx, _rx) = mpsc::channel();
        let term = Terminal::spawn_shell("/bin/sh", &std::env::temp_dir(), 10, 60, tx).unwrap();
        let reply = |seq: &[u8]| parse_query(seq, &term).map(|(_, r)| r);
        assert_eq!(reply(b"\x1b[0c"), Some(Some(b"\x1b[?62;22c".to_vec())));
        assert_eq!(reply(b"\x1b[?u"), Some(None));
        assert_eq!(
            reply(b"\x1b[>0q"),
            Some(Some(b"\x1bP>|liman\x1b\\".to_vec()))
        );
        assert!(
            reply(b"\x1b]11;?\x07")
                .unwrap()
                .unwrap()
                .starts_with(b"\x1b]11;rgb:")
        );
        assert_eq!(
            reply(b"\x1bP+q696e646e\x1b\\"),
            Some(Some(b"\x1bP0+r\x1b\\".to_vec()))
        );
        assert_eq!(reply(b"\x1b[6n"), Some(Some(b"\x1b[1;1R".to_vec())));
        assert_eq!(reply(b"\x1b]11;"), None); // incomplete: wait for more
    }

    #[test]
    fn keys_are_encoded_like_xterm() {
        let none = KeyModifiers::NONE;
        assert_eq!(
            key_bytes(key(KeyCode::Char('a'), none), false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes(key(KeyCode::Char('ş'), none), false),
            Some("ş".as_bytes().to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::Enter, none), false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::Up, none), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::Up, none), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::Char('b'), KeyModifiers::ALT), false),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::F(1), none), false),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_bytes(key(KeyCode::F(5), none), false),
            Some(b"\x1b[15~".to_vec())
        );
    }

    /// Feeds output into the terminal until `done` holds or the timeout passes.
    fn pump(
        term: &mut Terminal,
        rx: &mpsc::Receiver<AppEvent>,
        done: impl Fn(&Terminal) -> bool,
    ) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(AppEvent::TermOutput(bytes)) = rx.recv_timeout(Duration::from_millis(50)) {
                term.process(&bytes);
            }
            if done(term) {
                return true;
            }
        }
        false
    }

    #[test]
    fn runs_a_shell_and_follows_its_directory() {
        // Plain sh for predictable output, whatever the user's shell is.
        let (tx, rx) = mpsc::channel();
        let mut term = Terminal::spawn_shell("/bin/sh", &std::env::temp_dir(), 10, 60, tx).unwrap();
        term.send(b"echo liman-$((40+2))\r");
        assert!(pump(&mut term, &rx, |t| t
            .screen()
            .contents()
            .contains("liman-42")));

        assert!(pump(&mut term, &rx, |t| t.is_idle()));
        term.cd(Path::new("/"));
        assert!(pump(&mut term, &rx, |t| t.cwd().as_deref()
            == Some(Path::new("/"))));
    }
}
