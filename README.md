# herdr-copy-motion

A custom keyboard copy mode for Herdr, implemented as a temporary terminal overlay.
It captures the focused pane **before** opening the overlay and replays its ANSI-styled
cells, including colored blank cells such as Pi tool-card backgrounds. It does not
invoke built-in copy mode or modify Herdr.

- Toggle with `prefix+shift+s` after adding the documented keybinding.
- Move with Vim keys or arrows; select with `v`/`V`; copy with `y`/Enter.
- Preserve captured text styling without reflowing the snapshot on resize.

Requires Rust/Cargo and Herdr 0.9.5+ on Linux or macOS.

```sh
cargo build --release --locked
herdr plugin link "$PWD"
```

**Add the keybinding before using:** [Setup, controls, and compromises](docs/copy-mode.md).
The overlay can change pane geometry; this is a styled snapshot, not an exact native
screen capture, and cannot reproduce images or information missing from ANSI export.
