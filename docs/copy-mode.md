# Custom snapshot copy mode

## Setup

Requires Rust/Cargo (edition 2024), Herdr 0.9.5 or newer, and Linux or macOS.
Windows named pipes are not implemented. No Herdr patch is needed.

From this repository:

```sh
cargo build --release --locked
herdr plugin link "$PWD"
```

`plugin link` does not build the binary. Rebuild after changing Rust code.
For a GitHub installation, the manifest's build command builds the binary automatically:

```sh
herdr plugin install adonis0147/herdr-copy-motion
```

Add to Herdr's `config.toml` (usually `~/.config/herdr/config.toml`):

```toml
[[keys.command]]
key = "prefix+shift+s"
type = "plugin_action"
command = "herdr-copy-motion.toggle"
description = "toggle custom copy mode"
```

Reload Herdr's config. Remove any existing binding that conflicts with this key.
The prefix is your configured Herdr prefix; Shift+S means uppercase S.
The key is handled by Herdr, not hard-coded inside the viewer. While the viewer is
focused, invoking it again closes that viewer and restores the previous pane.
You can also test entry with `herdr plugin action invoke herdr-copy-motion.toggle`.

No user configuration is changed automatically by this project.

## Controls

| Key | Action |
| --- | --- |
| `h j k l`, arrows | Move one cell/row |
| `w b e` | Next word start, previous word start, next word end |
| `0`, Home | First column |
| `^` | First nonblank cell |
| `$`, End | Last nonblank cell |
| `gg`, PageUp, Ctrl+B | First captured row |
| `G`, PageDown, Ctrl+F | Last captured row |
| Ctrl+U / Ctrl+D | Move up/down half the captured height |
| `v`, Space | Toggle character selection |
| `V` | Start line selection |
| `y`, Enter | Copy the active selection and close |
| Escape | Cancel selection, or close if none |
| `q`, Ctrl+C | Close without copying |
| `prefix+shift+s` | Close via the plugin action |

The cursor starts at the top-left captured cell. Wide-character continuation cells
are not separate cursor stops. Motions use source-cell coordinates, so they work
with both bordered and borderless panes. A small viewer clips the fixed grid and
follows the cursor horizontally/vertically rather than reflowing text. A larger
viewer leaves unused space blank. No status line is painted over captured rows.

Copy uses OSC 52, forwarded by Herdr to the attached terminal. Clipboard support
and permissions depend on that terminal. There is no confirmation from the system
clipboard. Copies are limited to 1 MiB, contain text rather than ANSI, trim trailing
whitespace on selected rows, and insert newlines between captured rows. Line
selection adds a final newline.

## Rendering contract and compromises

1. Request the focused pane's **visible ANSI viewport** before opening an overlay.
   Reject plain-text or truncated responses rather than silently dropping colors.
2. Save its styling and dimensions. Styled blank cells are not trimmed by this
   plugin: green card padding is as important as the text on it.
3. Replay exported glyphs and SGR attributes directly. This is not a Markdown or
   plain-text reconstruction. Even SGR attributes unknown to the motion parser are
   forwarded. Non-styling terminal commands (clipboard writes, title changes,
   device queries) and OSC hyperlinks are not replayed.
4. Draw a cyan block cursor and white-on-blue selection over that immutable
   snapshot. Moving the cursor or cancelling selection restores the original
   styling. These colors approximate native copy mode; Herdr's active theme
   palette is not exposed to this plugin.

The guarantee is limited to cells and attributes present in Herdr's ANSI export,
not every property of the original terminal surface:

- This is a frozen **visible viewport**, not live output or the entire scrollback.
  Scroll the original pane first to capture older output.
- Images (Kitty/Sixel), hyperlinks, original cursor position, dynamic palette
  changes, and other non-SGR terminal state are not captured. Default/indexed
  colors depend on the destination pane's palette. Unicode width follows the
  parser's width tables and may differ from a terminal's emoji/grapheme policy.
- The overlay is zoomed. Other panes/borders need not remain in the same places;
  opening and closing can resize/redraw the underlying application. The stored
  snapshot itself is not re-rendered by Pi or reflowed by the plugin.
- Herdr exposes outer layout rectangles and viewport rows, but not exact terminal
  columns. Exported cell widths always take precedence; unexported default blank
  padding is estimated from the border insets. Asymmetric borders, zoom, or a
  scrollbar can leave extra/missing default blank cursor columns. No exported
  colored cells are intentionally discarded by that estimate.
- Capturing and opening are separate API calls. Layout/scroll changes detected
  during capture are retried, but there is no atomic host-side freeze operation.

## Privacy and troubleshooting

Snapshots may contain sensitive terminal output. Temporary JSON snapshots are
created with mode `0600` under `HERDR_PLUGIN_STATE_DIR`, removed when the viewer
reads them, and removed after a failed open request. They are never written to
plugin logs. A killed action, failed viewer startup, or machine crash can leave
snapshot files or viewer markers behind. After closing all plugin viewers, remove
stale `snapshot-*.json` and `viewer-*` files from the plugin's state directory if
needed. An abruptly killed viewer may require a terminal reset.

For launch/capture errors, inspect `herdr plugin log list --plugin herdr-copy-motion`.
If the server does not support ANSI reads or the overlay API, upgrade/restart the
Herdr server; do not switch to a text-only export.

## Development checks

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

Tests compare replayed cells (including RGB/indexed colored blanks, wide glyphs,
and combining marks), clipping, selection restoration, motions, and a mock Herdr
socket for capture-before-open, border geometry, toggle exit, and failure cleanup.
Real Herdr layout behavior and clipboard acceptance still need interactive testing.
