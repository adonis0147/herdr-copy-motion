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
focused, invoking it again queues a quit for that viewer and restores the previous
pane after any in-progress history capture has restored the source scroll offset.
You can also test entry with `herdr plugin action invoke herdr-copy-motion.toggle`.

No user configuration is changed automatically by this project.

## Controls

| Key | Action |
| --- | --- |
| `h j k l`, arrows | Move one cell/row |
| `w b e` | Next word start, previous word start, next word end |
| `f<char>` / `F<char>` | Find the next / previous occurrence on the current row |
| `t<char>` / `T<char>` | Stop just before / after the next / previous occurrence |
| `;` / `,` | Repeat the last character find in its original / opposite direction |
| `s` | Start a Flash-style labeled jump within the visible viewport |
| `0`, Home | First column |
| `^` | First nonblank cell |
| `$`, End | Last nonblank cell |
| `gg` / `G` | First / last captured row |
| PageUp / PageDown, Ctrl+B / Ctrl+F | Scroll up/down one viewer page |
| Ctrl+U / Ctrl+D | Scroll up/down half a viewer page |
| Mouse wheel | Scroll up/down three rows |
| `v`, Space | Toggle character selection |
| `V` | Start line selection |
| Ctrl+V | Start block selection, or toggle rectangular/character selection without moving its anchor |
| `y`, Enter | Copy the active selection and close |
| Escape | Cancel selection, or close if none |
| `q`, Ctrl+C | Close without copying |
| `prefix+shift+s` | Close via the plugin action |

Character finds wait for one literal character, including command keys such as
`q`, `v`, or Space. They stay on the current captured screen row and leave the
cursor unchanged if no match exists. Escape cancels a pending find without closing
copy mode or clearing a selection. Repeat keys preserve the original find direction;
repeated `t`/`T` advances past the previous target rather than getting stuck beside
it. These motions extend active selections and keep wide glyphs/combining marks
intact; matching uses a cell's base character. Numeric counts and operator-pending
commands are not implemented.

### Flash-style jumps

Press `s`, type a case-sensitive literal pattern, then press a displayed label to
jump to that match. You can keep typing to narrow the search: label characters
are excluded whenever they could continue any visible match. Enter jumps to the
nearest match; Backspace edits the pattern; Escape cancels without moving the
cursor or clearing the active selection. Ctrl+C still closes copy mode.

During jump mode, snapshot text uses a muted, theme-derived foreground plus faint
intensity for a stronger backdrop than faint alone. Existing selections are also
faint, while matches remain at normal intensity and labels stay bold. Background
colors and snapshot data are unchanged; original foregrounds and attributes return
on jump, cancel, or resize. The faint component's strength depends on the attached
terminal.

Matches are highlighted and labels temporarily replace their first display cell.
Nearest targets receive labels first, reusing labels while refining when possible.
There are up to 52 single-letter labels; if a target has no label, narrow the
pattern or use Enter. Labels never become part of copied text. A jump extends
character, line, or block selections using the original snapshot cells.

Only fully visible glyphs on the current viewport are searched, without matching
across captured rows. Resize or mouse-wheel paging cancels jump mode. This is a
standalone jump feature inspired by [flash.nvim](https://github.com/folke/flash.nvim),
not its regex/fuzzy search, Treesitter, or cross-window integration.

### Cursor and rendering

Known limitation: the cursor currently starts at the bottom-left of the original
viewport, not at the source application's current cursor. Herdr 0.9.5's plugin API
and ANSI export do not expose that cursor's coordinates. Starting at the actual
cursor requires a host API addition; the export's final text position is not a
valid substitute. Wide-character continuation cells
are not separate cursor stops. Motions use source-cell coordinates, so they work
with both bordered and borderless panes. A small viewer clips the fixed grid and
follows the cursor horizontally/vertically rather than reflowing text. A larger
viewer leaves unused space blank. No status line is painted over captured rows.
Each redraw buffers the snapshot, selection, and cursor inside one synchronized
terminal update (DEC mode 2026), so Herdr does not display a cleared or partially
painted frame while scrolling. Entry includes alternate-screen setup in that
transaction. On exit, the disposable overlay retains its last frame until Herdr
removes it, rather than switching to its empty main screen.

Block selection uses an inclusive rectangle between the anchor and cursor, in any
direction. It works with the same motions and paging keys as other selections.
A rectangle touching either half of a wide glyph includes the whole glyph;
combining marks stay with their base character. `v`, Space, or Escape cancels the
selection as before.

Copy uses OSC 52, forwarded by Herdr to the attached terminal. Clipboard support
and permissions depend on that terminal. There is no confirmation from the system
clipboard. Copies are limited to 1 MiB, contain text rather than ANSI, and insert
newlines between captured rows. Character and line selections trim trailing
whitespace; block selections preserve spaces and blank cells within the rectangle.
Only line selection adds a final newline.

## Rendering contract and compromises

1. Capture the focused pane's **ANSI viewport** before opening the overlay. Paint
   that snapshot first, then collect retained scrollback in viewport-sized pages
   while the source pane is hidden behind the overlay. Restore its original scroll
   offset even if a page read fails. This bypasses the 1000-row `recent` read cap
   without visibly scrolling the source. Reject plain-text or incomplete exports.
2. Save its styling and dimensions. Styled blank cells are not trimmed by this
   plugin: green card padding is as important as the text on it.
3. Replay exported glyphs and SGR attributes directly. This is not a Markdown or
   plain-text reconstruction. Even SGR attributes unknown to the motion parser are
   forwarded. Non-styling terminal commands (clipboard writes, title changes,
   device queries) and OSC hyperlinks are not replayed.
4. Draw a bold block cursor using the configured Herdr theme's accent background
   and panel background foreground (surface-dim when panel background is reset),
   like native copy mode. Selection follows Herdr's automatic gray-tint and contrast
   rules instead of fixed blue: RGB backgrounds blend 28% toward white or black,
   and text uses a contrasting foreground. Moving the cursor or cancelling
   selection restores the original styling. No plugin color configuration is
   needed. Herdr's active client palette and terminal background are not exposed
   by the plugin API, so this uses native's configured-palette fallback; a client
   with a different terminal background or auto-switched theme may differ.

The guarantee is limited to cells and attributes present in Herdr's ANSI export,
not every property of the original terminal surface:

- Herdr first limits the source pane's retained history using
  `advanced.scrollback_limit_bytes` (default: 10,000,000 bytes per pane). Capture
  uses that pane's actual `max_offset_from_bottom`, so it respects the running
  server's buffer retention without separately reading or guessing its config.
  Output already evicted by Herdr cannot be captured.
- Collect as much of that retained scrollback as fits: up to 65,534 total rows,
  a 128 MiB parsed-cell budget (fewer rows for wide panes), and 32 MiB of captured
  ANSI. These additional viewer safety limits can result in less history than
  native copy mode; they are not Herdr's 1000-row read cap. Newest history
  takes priority. If the original viewport is outside the collected range or the
  source identity/scroll geometry changes, retain the initial visible snapshot.
- History collection happens once, not live. Input waits for capture and source
  scroll restoration to finish; large buffers can take longer to open. The initial
  snapshot remains visible during collection. In-place application redraws cannot
  be frozen atomically by this API.
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
needed. Forcibly closing/killing the viewer during history capture can prevent
source-scroll restoration; normal quit and toggle wait for restoration. An abruptly
killed viewer may require a terminal reset.

For launch/capture errors, inspect `herdr plugin log list --plugin herdr-copy-motion`.
If the server does not support ANSI reads or the overlay API, upgrade/restart the
Herdr server; do not switch to a text-only export.

## Code layout

- `src/main.rs`: command dispatch, overlay lifecycle, and initial capture.
- `src/herdr.rs`: bounded socket requests and ANSI response validation.
- `src/history.rs`: hidden-source paging and scroll restoration.
- `src/snapshot.rs`: snapshot validation, memory limits, and styled rendering.
- `src/viewer.rs`: terminal setup, motions, selection, and display.
- `src/viewer/jump.rs`: visible literal matching and collision-free jump labels.
- `src/theme.rs`: native copy-mode color rules using the locally configured palette.

Larger unit-test modules live in `src/<module>/tests.rs`; `tests/plugin.rs` tests
command/socket integration. Snapshot validation applies before parser allocation,
and history capture preserves both page-read and restoration errors if both fail.

## Development checks

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

Tests compare replayed cells (including RGB/indexed colored blanks, wide glyphs,
and combining marks), clipping, selection restoration, motions, and a mock Herdr
socket for capture-before-open, border geometry, deferred hidden-source history
capture, queued toggle exit, and failure cleanup. History tests cover more than
1000 rows, partial-page overlap, styled/omitted blanks, wider older rows, source
identity checks, restoration after read failure, and memory/coordinate limits.
A real Herdr 0.9.5 client/server was also tested in an isolated PTY session:

- With a visible source cursor at row 5/column 9, then row 9/column 13, native copy
  mode highlighted the current cursor cell each time. Both positions produced
  identical `pane.read` ANSI text, and `pane.get` exposed no cursor coordinates.
  This confirms that the plugin cannot infer the actual cursor from those APIs.
- With `panel_bg = '#303030'` and no reported host-terminal background, native
  and plugin selections both rendered white text on `#6a6a6a`, without bold.
  This checks native's palette fallback, not a client's unexported background tint.
- PageUp/PageDown and Ctrl+U/D produced four balanced synchronized client updates
  and no outer-terminal screen clears during scrolling.
- With 1500 source rows, `gg` reached row 0 after history loading; hidden-source
  paging produced no overlay content updates and restored the original offset.
  Exit returned directly to source content without an empty viewer main-screen frame.
  An occasional blank entry frame was still observed while Herdr created the new
  overlay, before its process could paint. Startup synchronization begins as early
  as possible, but eliminating that host-side gap requires Herdr to retain the old
  surface until the overlay's first frame is ready.

These checks inspect terminal output, not a visual recording. Clipboard acceptance
and behavior in the user's particular terminal still need interactive verification.
