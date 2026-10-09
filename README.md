# liman

A terminal file manager with the feel of a GUI file manager (Nautilus / Dolphin), built with Rust and Ratatui.
Made for people who like GUI file managers but spend their day in SSH and tmux.

**Status:** early development.

![liman: tabs, path bar with git branch, Places, the file list, preview and the built-in terminal](docs/img/02-parcalar.png)

- Three views: **Detailed** (one line per file), **Normal** and **Grid** (hollow type-colored boxes)
- Places: Quick Access (your own pinned files and folders, recent files) and a folder tree that shows where you are
- Path bar you can type into (Ctrl+L, Tab completes), back / forward, mouse (click, Ctrl/Shift+click, drag and drop, right-click menu)
- Search as you type in this folder and below (Ctrl+F)
- Built-in terminal (F4) that follows the open folder; files a command prints (`find`, `fd`, `rg -l`...) show up above
- Git: status letters next to names, branch in the path bar, Ctrl+G panel with diff, stage, commit, push, pull
- Preview panel (F3): file details, syntax colors, formatted Markdown, hex dumps, PDF and archive listings, and real
  images on Kitty / Sixel / iTerm2 terminals (half blocks elsewhere)
- Tabs (Ctrl+T), each with its own folder and shell
- Files to other apps: Ctrl+C copies them for other apps too (Ctrl+V in a browser or chat); Ctrl+V pastes files copied
  in another file manager; dragging an entry to the window edge copies it; files dropped on the window are copied here
- Command palette (Ctrl+P), 8 themes, optional Nerd Font icons, English and Turkish, 256-color fallback
- Safe by default: trashing asks first and can be undone; home and the well-known folders cannot be removed

Full guide (Turkish): [docs/KILAVUZ.md](docs/KILAVUZ.md).

## Screenshots

The interface speaks English or Turkish (these shots are in Turkish).

| | |
|---|---|
| ![Grid view](docs/img/01-acilis.png) **Grid**: hollow boxes, folders in the color of what they hold | ![Detailed view](docs/img/04-ayrintili.png) **Detailed**: type, name, size, date |
| ![Code preview](docs/img/08-onizleme-kod.png) **Preview (F3)**: syntax colors, line numbers | ![Image preview](docs/img/17-onizleme-resim.png) **Images** drawn with terminal cells |
| ![Git panel](docs/img/14-git-panel.png) **Git (Ctrl+G)**: diff, stage, commit, push target | ![Git letters](docs/img/13-git-liste.png) **Git status** next to every file |
| ![find results](docs/img/11-find-sonuc.png) **Terminal (F4)**: files a command prints show up above | ![Tabs](docs/img/12-sekmeler.png) **Tabs**, each with its own folder and shell |
| ![Reader](docs/img/09-okuyucu.png) **Reader**: Markdown full screen | ![Multi-select](docs/img/05-secim.png) **Selection**: Shift / Ctrl, like a GUI |
| ![Command palette](docs/img/15-palet.png) **Command palette (Ctrl+P)** | ![Themes](docs/img/16-tema.png) **8 themes**, live preview |
| ![Formatted Markdown](docs/img/19-markdown.png) **Markdown** formatted in the preview | ![Nerd Font icons](docs/img/20-nerd.png) **Nerd Font icons** (`icons = nerd`) and the folder tree |
| ![Type a path](docs/img/18-yol-yaz.png) **Ctrl+L**: type a path, Tab completes | ![Trash asks first](docs/img/21-cop-onay.png) **Trash asks first**, Ctrl+Z undoes |

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
| `icons` | `unicode` (default), `nerd` (needs a Nerd Font) |
| `images` | `auto` (default: asks the terminal for Kitty / Sixel / iTerm2), `halfblocks` |

Inside liman, `?` lists all keys.

## License

MIT
