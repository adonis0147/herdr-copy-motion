#[cfg(test)]
#[path = "viewer_tests.rs"]
mod tests;

use crate::{Result, Snapshot, snapshot};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};

pub fn run(snapshot: Snapshot) -> Result<()> {
    let mut parser = vt100::Parser::new(snapshot.rows + 1, snapshot.cols, 0);
    // The export uses LF as a row separator, not a terminal's LF-without-CR.
    parser.process(snapshot.ansi.replace('\n', "\r\n").as_bytes());
    let mut viewer = Viewer {
        snapshot,
        parser,
        cursor: (0, 0),
        anchor: None,
        linewise: false,
        offset: (0, 0),
        pending_g: false,
    };
    let _terminal = TerminalGuard::enter()?;
    let mut output = io::stdout().lock();
    loop {
        viewer.draw(&mut output, terminal::size()?)?;
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
            Event::Resize(_, _) => {}
            _ => continue,
        }
    }
    Ok(())
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, DisableLineWrap, Hide)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            crossterm::style::ResetColor,
            EnableLineWrap,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

struct Viewer {
    snapshot: Snapshot,
    parser: vt100::Parser,
    cursor: (u16, u16), // row, column, in source cells (never border coordinates)
    anchor: Option<(u16, u16)>,
    linewise: bool,
    offset: (u16, u16), // horizontal, vertical
    pending_g: bool,
}

enum Action {
    Continue,
    Copy,
    Quit,
}

impl Viewer {
    fn input(&mut self, key: KeyEvent) -> Action {
        let previous = self.cursor;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return Action::Quit,
                KeyCode::Char('u') => {
                    self.cursor.0 = self.cursor.0.saturating_sub(self.snapshot.rows / 2)
                }
                KeyCode::Char('d') => {
                    self.cursor.0 = self.cursor.0.saturating_add(self.snapshot.rows / 2)
                }
                KeyCode::Char('b') => self.cursor.0 = 0,
                KeyCode::Char('f') => self.cursor.0 = self.snapshot.rows - 1,
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
                    self.linewise = false;
                }
                KeyCode::Char('V') => {
                    self.anchor = Some(self.cursor);
                    self.linewise = true;
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
                KeyCode::Char('G') | KeyCode::PageDown => self.cursor.0 = self.snapshot.rows - 1,
                KeyCode::PageUp => self.cursor.0 = 0,
                KeyCode::Char('w') => self.word_motion(false, false),
                KeyCode::Char('b') => self.word_motion(true, false),
                KeyCode::Char('e') => self.word_motion(false, true),
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

    fn word_motion(&mut self, backward: bool, end: bool) {
        let cols = usize::from(self.snapshot.cols);
        let total = cols * usize::from(self.snapshot.rows);
        let mut pos = usize::from(self.cursor.0) * cols + usize::from(self.cursor.1);
        let class = |p: usize| {
            let ch = self
                .cell((p / cols) as u16, (p % cols) as u16)
                .contents()
                .chars()
                .next()
                .unwrap_or(' ');
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
            pos = (pos + 1).min(total - 1);
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
        if self.linewise {
            row >= start.0 && row <= end.0
        } else {
            (row, col) >= start && (row, col) <= end
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
            lines.push(text.trim_end().to_owned());
        }
        let mut text = lines.join("\n");
        if self.linewise {
            text.push('\n');
        }
        text
    }

    fn draw(&mut self, output: &mut impl Write, size: (u16, u16)) -> io::Result<()> {
        if size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        self.offset.0 = follow(self.cursor.1, self.offset.0, size.0);
        self.offset.1 = follow(self.cursor.0, self.offset.1, size.1);
        output.write_all(&snapshot::render(
            &self.snapshot.ansi,
            self.snapshot.cols,
            self.offset,
            size,
        ))?;
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
                        "0;1;30;106"
                    } else {
                        "0;97;44"
                    };
                    write!(
                        output,
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
