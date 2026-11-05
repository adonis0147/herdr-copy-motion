#[cfg(test)]
mod tests;

use crate::{
    Result, herdr, history,
    snapshot::{self, Snapshot},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossterm::{
    cursor::Hide,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen},
};
use std::io::{self, Write};

pub(crate) fn run(snapshot: Snapshot) -> Result<()> {
    let mut viewer = Viewer::new(snapshot);
    let mut output = io::stdout().lock();
    viewer.draw(&mut output, terminal::size()?)?;
    if let Some(snapshot) = history::load_history(&viewer.snapshot, herdr::rpc)? {
        viewer = Viewer::new(snapshot);
        viewer.draw(&mut output, terminal::size()?)?;
    }
    loop {
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match viewer.input(key) {
                Action::Quit => break,
                Action::Copy => {
                    let text = viewer.selected_text();
                    if text.len() > 1024 * 1024 {
                        return Err("selection exceeds the 1 MiB clipboard limit".into());
                    }
                    write!(output, "\x1b]52;c;{}\x07", STANDARD.encode(text))?;
                    output.flush()?;
                    break;
                }
                Action::Continue => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => viewer.page(-3),
                MouseEventKind::ScrollDown => viewer.page(3),
                _ => continue,
            },
            Event::Resize(_, _) => {}
            _ => continue,
        }
        viewer.draw(&mut output, terminal::size()?)?;
    }
    Ok(())
}

pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            terminal::BeginSynchronizedUpdate,
            EnterAlternateScreen,
            DisableLineWrap,
            EnableMouseCapture,
            Hide
        )?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // This disposable overlay is removed on exit; retain its last frame until then.
        let _ = execute!(
            io::stdout(),
            terminal::EndSynchronizedUpdate,
            crossterm::style::ResetColor,
            EnableLineWrap,
            DisableMouseCapture
        );
        let _ = terminal::disable_raw_mode();
    }
}

struct Viewer {
    snapshot: Snapshot,
    parser: vt100::Parser,
    cursor: (u16, u16), // row, column, in source cells (never border coordinates)
    anchor: Option<(u16, u16)>,
    selection_mode: SelectionMode,
    offset: (u16, u16), // horizontal, vertical
    pending_g: bool,
    pending_find: Option<char>,
    last_find: Option<CharacterFind>,
    height: u16,
    cursor_style: String,
    selection_style: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionMode {
    Character,
    Line,
    Block,
}

#[derive(Clone, Copy)]
struct CharacterFind {
    target: char,
    backward: bool,
    till: bool,
}

enum Action {
    Continue,
    Copy,
    Quit,
}

impl Viewer {
    fn new(snapshot: Snapshot) -> Self {
        let mut parser = vt100::Parser::new(snapshot.rows + 1, snapshot.cols, 0);
        // The export uses LF as a row separator, not a terminal's LF-without-CR.
        parser.process(snapshot.ansi.replace('\n', "\r\n").as_bytes());
        let styles = crate::theme::styles();
        Self {
            cursor: (snapshot.viewport_top + snapshot.viewport_rows - 1, 0),
            offset: (0, snapshot.viewport_top),
            height: snapshot.viewport_rows,
            snapshot,
            parser,
            anchor: None,
            selection_mode: SelectionMode::Character,
            pending_g: false,
            pending_find: None,
            last_find: None,
            cursor_style: styles.cursor,
            selection_style: styles.selection,
        }
    }

    fn input(&mut self, key: KeyEvent) -> Action {
        if let Some(command) = self.pending_find.take() {
            self.pending_g = false;
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Action::Quit;
            }
            if let KeyCode::Char(target) = key.code
                && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
            {
                let find = CharacterFind {
                    target,
                    backward: matches!(command, 'F' | 'T'),
                    till: matches!(command, 't' | 'T'),
                };
                self.last_find = Some(find);
                self.find_character(find, false);
            }
            return Action::Continue;
        }
        let previous = self.cursor;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            self.pending_g = false;
            match key.code {
                KeyCode::Char('c') => return Action::Quit,
                KeyCode::Char('v') => {
                    self.selection_mode =
                        if self.anchor.is_some() && self.selection_mode == SelectionMode::Block {
                            SelectionMode::Character
                        } else {
                            SelectionMode::Block
                        };
                    self.anchor.get_or_insert(self.cursor);
                }
                KeyCode::Char('u') => self.page(-i32::from((self.height / 2).max(1))),
                KeyCode::Char('d') => self.page(i32::from((self.height / 2).max(1))),
                KeyCode::Char('b') => self.page(-i32::from(self.height)),
                KeyCode::Char('f') => self.page(i32::from(self.height)),
                _ => {}
            }
        } else {
            let second_g = self.pending_g && key.code == KeyCode::Char('g');
            self.pending_g = false;
            match key.code {
                KeyCode::Char('q') => return Action::Quit,
                KeyCode::Esc => {
                    if self.anchor.take().is_none() {
                        return Action::Quit;
                    }
                }
                KeyCode::Char('y') | KeyCode::Enter if self.anchor.is_some() => {
                    return Action::Copy;
                }
                KeyCode::Char('v' | ' ') => {
                    self.anchor = if self.anchor.is_some() {
                        None
                    } else {
                        Some(self.cursor)
                    };
                    self.selection_mode = SelectionMode::Character;
                }
                KeyCode::Char('V') => {
                    self.anchor = Some(self.cursor);
                    self.selection_mode = SelectionMode::Line;
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    self.cursor.1 = self.cursor.1.saturating_sub(1)
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.cursor.1 = self.cursor.1.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => self.cursor.0 = self.cursor.0.saturating_sub(1),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.cursor.0 = self.cursor.0.saturating_add(1)
                }
                KeyCode::Char('0') | KeyCode::Home => self.cursor.1 = 0,
                KeyCode::Char('^') => {
                    self.cursor.1 = (0..self.snapshot.cols)
                        .find(|&col| !self.cell(self.cursor.0, col).contents().trim().is_empty())
                        .unwrap_or(0);
                }
                KeyCode::Char('$') | KeyCode::End => {
                    self.cursor.1 = (0..self.snapshot.cols)
                        .rfind(|&col| !self.cell(self.cursor.0, col).contents().trim().is_empty())
                        .unwrap_or(0);
                }
                KeyCode::Char('g') if second_g => self.cursor.0 = 0,
                KeyCode::Char('g') => self.pending_g = true,
                KeyCode::Char('G') => self.cursor.0 = self.snapshot.rows - 1,
                KeyCode::PageDown => self.page(i32::from(self.height)),
                KeyCode::PageUp => self.page(-i32::from(self.height)),
                KeyCode::Char('w') => self.word_motion(false, false),
                KeyCode::Char('b') => self.word_motion(true, false),
                KeyCode::Char('e') => self.word_motion(false, true),
                KeyCode::Char(command @ ('f' | 'F' | 't' | 'T')) => {
                    self.pending_find = Some(command);
                }
                KeyCode::Char(';' | ',') => {
                    if let Some(mut find) = self.last_find {
                        if key.code == KeyCode::Char(',') {
                            find.backward = !find.backward;
                        }
                        self.find_character(find, true);
                    }
                }
                _ => {}
            }
        }
        self.cursor.0 = self.cursor.0.min(self.snapshot.rows - 1);
        self.cursor.1 = self.cursor.1.min(self.snapshot.cols - 1);
        if self
            .cell(self.cursor.0, self.cursor.1)
            .is_wide_continuation()
        {
            if self.cursor.0 == previous.0
                && self.cursor.1 > previous.1
                && self.cursor.1 + 1 < self.snapshot.cols
            {
                self.cursor.1 += 1;
            } else {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
            }
        }
        Action::Continue
    }

    fn find_character(&mut self, find: CharacterFind, repeat: bool) {
        let (row, start) = self.cursor;
        for distance in 1..self.snapshot.cols {
            let column = if find.backward {
                start.checked_sub(distance)
            } else {
                start
                    .checked_add(distance)
                    .filter(|&col| col < self.snapshot.cols)
            };
            let Some(column) = column else { break };
            let cell = self.cell(row, column);
            if cell.is_wide_continuation() || !cell.contents().starts_with(find.target) {
                continue;
            }
            let mut destination = if !find.till {
                column
            } else if find.backward {
                column + if cell.is_wide() { 2 } else { 1 }
            } else {
                column - 1
            };
            if self.cell(row, destination).is_wide_continuation() {
                destination = destination.saturating_sub(1);
            }
            // Repeating t/T must pass the adjacent target that stopped the last motion.
            if repeat && destination == start {
                continue;
            }
            self.cursor.1 = destination;
            return;
        }
    }

    fn page(&mut self, delta: i32) {
        self.cursor.0 =
            (i32::from(self.cursor.0) + delta).clamp(0, i32::from(self.snapshot.rows - 1)) as u16;
        self.offset.1 = (i32::from(self.offset.1) + delta)
            .clamp(0, i32::from(self.snapshot.rows.saturating_sub(self.height)))
            as u16;
        if self
            .cell(self.cursor.0, self.cursor.1)
            .is_wide_continuation()
        {
            self.cursor.1 = self.cursor.1.saturating_sub(1);
        }
    }

    fn word_motion(&mut self, backward: bool, end: bool) {
        let cols = usize::from(self.snapshot.cols);
        let total = cols * usize::from(self.snapshot.rows);
        let mut pos = usize::from(self.cursor.0) * cols + usize::from(self.cursor.1);
        let class = |p: usize| {
            let row = (p / cols) as u16;
            let col = (p % cols) as u16;
            // A wide glyph's second cell has empty contents, but is not whitespace.
            let col = if self.cell(row, col).is_wide_continuation() {
                col.saturating_sub(1)
            } else {
                col
            };
            let ch = self.cell(row, col).contents().chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                0
            } else if ch.is_alphanumeric() || ch == '_' {
                1
            } else {
                2
            }
        };
        if backward {
            pos = pos.saturating_sub(1);
            while pos > 0 && class(pos) == 0 {
                pos -= 1;
            }
            let target = class(pos);
            while pos > 0 && class(pos - 1) == target {
                pos -= 1;
            }
        } else if end {
            let width = if self.cell(self.cursor.0, self.cursor.1).is_wide() {
                2
            } else {
                1
            };
            pos = (pos + width).min(total - 1);
            while pos + 1 < total && class(pos) == 0 {
                pos += 1;
            }
            let target = class(pos);
            while pos + 1 < total && class(pos + 1) == target {
                pos += 1;
            }
        } else {
            let target = class(pos);
            while pos + 1 < total && class(pos) == target {
                pos += 1;
            }
            while pos + 1 < total && class(pos) == 0 {
                pos += 1;
            }
        }
        self.cursor = ((pos / cols) as u16, (pos % cols) as u16);
        // Word ends can fall on the second cell; keep the cursor on the glyph,
        // rather than letting input's rightward normalization skip past it.
        if self
            .cell(self.cursor.0, self.cursor.1)
            .is_wide_continuation()
        {
            self.cursor.1 = self.cursor.1.saturating_sub(1);
        }
    }

    fn cell(&self, row: u16, col: u16) -> &vt100::Cell {
        self.parser
            .screen()
            .cell(row, col)
            .expect("validated snapshot coordinates")
    }

    fn selected(&self, row: u16, col: u16) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        let (start, end) = (anchor.min(self.cursor), anchor.max(self.cursor));
        if row < start.0 || row > end.0 {
            return false;
        }
        match self.selection_mode {
            SelectionMode::Character => (row, col) >= start && (row, col) <= end,
            SelectionMode::Line => true,
            SelectionMode::Block => {
                let left = anchor.1.min(self.cursor.1);
                let right = anchor.1.max(self.cursor.1);
                // A rectangle touching either half includes the whole wide glyph.
                let first = if self.cell(row, col).is_wide_continuation() {
                    col.saturating_sub(1)
                } else {
                    col
                };
                let last = first + u16::from(self.cell(row, first).is_wide());
                first <= right && last >= left
            }
        }
    }

    fn selected_text(&self) -> String {
        let Some(anchor) = self.anchor else {
            return String::new();
        };
        let (start, end) = (anchor.min(self.cursor), anchor.max(self.cursor));
        let mut lines = Vec::new();
        for row in start.0..=end.0 {
            let mut text = String::new();
            for col in 0..self.snapshot.cols {
                let cell = self.cell(row, col);
                if self.selected(row, col) && !cell.is_wide_continuation() {
                    if cell.contents().is_empty() {
                        text.push(' ');
                    } else {
                        text.push_str(cell.contents());
                    }
                }
            }
            lines.push(if self.selection_mode == SelectionMode::Block {
                text
            } else {
                text.trim_end().to_owned()
            });
        }
        let mut text = lines.join("\n");
        if self.selection_mode == SelectionMode::Line {
            text.push('\n');
        }
        text
    }

    fn draw(&mut self, output: &mut impl Write, size: (u16, u16)) -> io::Result<()> {
        if size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        self.height = size.1;
        self.offset.1 = self.offset.1.min(self.snapshot.rows.saturating_sub(size.1));
        self.offset.0 = follow(self.cursor.1, self.offset.0, size.0);
        if size.0 > 1 && self.cell(self.cursor.0, self.cursor.1).is_wide() {
            self.offset.0 = follow(self.cursor.1 + 1, self.offset.0, size.0);
        }
        self.offset.1 = follow(self.cursor.0, self.offset.1, size.1);
        // Herdr must not render the clear or an incomplete snapshot between PTY reads.
        let mut frame = Vec::from(b"\x1b[?2026h".as_slice());
        frame.extend(snapshot::render(
            &self.snapshot.ansi,
            self.snapshot.cols,
            self.offset,
            size,
        ));
        for row in self.offset.1..self.snapshot.rows.min(self.offset.1.saturating_add(size.1)) {
            for col in self.offset.0..self.snapshot.cols.min(self.offset.0.saturating_add(size.0)) {
                let cell = self.cell(row, col);
                if cell.is_wide_continuation() {
                    continue;
                }
                if cell.is_wide() && col - self.offset.0 + 1 >= size.0 {
                    continue;
                }
                if (row, col) == self.cursor || self.selected(row, col) {
                    let style = if (row, col) == self.cursor {
                        self.cursor_style.as_str()
                    } else {
                        self.selection_style.as_str()
                    };
                    write!(
                        frame,
                        "\x1b[{};{}H\x1b[{style}m{}\x1b[0m",
                        row - self.offset.1 + 1,
                        col - self.offset.0 + 1,
                        if cell.contents().is_empty() {
                            " "
                        } else {
                            cell.contents()
                        }
                    )?;
                }
            }
        }
        frame.extend_from_slice(b"\x1b[?2026l");
        output.write_all(&frame)?;
        output.flush()
    }
}

fn follow(cursor: u16, offset: u16, extent: u16) -> u16 {
    if cursor < offset {
        cursor
    } else if cursor >= offset.saturating_add(extent) {
        cursor - extent + 1
    } else {
        offset
    }
}
