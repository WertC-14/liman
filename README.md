# liman

A terminal file manager with the feel of a GUI file manager (Nautilus / Dolphin), built with Rust and Ratatui.
Made for people who like GUI file managers but spend their day in SSH and tmux.

**Status:** early development.

- Three views: **Detailed** (one line per file), **Normal** and **Grid** (hollow type-colored boxes)
- Places sidebar with bookmarks and recent files, path bar, back / forward, mouse (click, Ctrl/Shift+click, drag and drop, right-click menu)
- Built-in terminal (F4) that follows the open folder; files a command prints (`find`, `fd`, `rg -l`...) show up above
- Git: status letters next to names, branch in the path bar, Ctrl+G panel with diff, stage, commit, push, pull
- Preview panel (F3): text, folders, images (half blocks), PDF and archive listings
- Tabs (Ctrl+T), each with its own folder and shell
- Command palette (Ctrl+P), 8 themes, English and Turkish, 256-color fallback for terminals without truecolor

Full guide (Turkish): [docs/KILAVUZ.md](docs/KILAVUZ.md).

## Install

Static Linux binary (x86_64, aarch64), no dependencies on the server:

```bash
curl -fsSL https://raw.githubusercontent.com/WertC-14/liman/main/install.sh | sh
```

It goes to `~/.local/bin/liman` (`LIMAN_INSTALL_DIR` changes that). From source:

```bash
cargo install --git https://github.com/WertC-14/liman liman
```

## Settings

`~/.config/liman/config`, one `key = value` per line:

| Key | Values |
|---|---|
| `theme` | liman, nord, gruvbox, catppuccin, tokyo-night, dracula, rose-pine, light (`t` picks one) |
| `lang` | `tr`, `en` (default: from the locale) |
| `colors` | `truecolor`, `256` (default: guessed from `COLORTERM` / `TERM`) |

Inside liman, `?` lists all keys.

## License

MIT
