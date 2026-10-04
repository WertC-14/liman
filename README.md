# liman

A terminal file manager with the feel of a GUI file manager (Nautilus / Dolphin), built with Rust and Ratatui.
Made for people who like GUI file managers but spend their day in SSH and tmux.

**Status:** early development, not usable yet.

Planned views:
- **Detailed** — one line per file, type-colored badge
- **Normal** — type-colored box per file
- **Grid** — large icons drawn with half-block cells (works in every truecolor terminal, no graphics protocol needed)

## Build

```bash
cargo run -p liman
```

## License

MIT
