use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write;
use unicode_width::UnicodeWidthChar;

#[cfg(test)]
mod tests;

pub(crate) const MAX_COLS: u16 = 4096;
pub(crate) const MAX_ANSI_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_GRID_BYTES: usize = 128 * 1024 * 1024;

/// Replay exported cells, not the text-only projection used for motions/copying.
/// Forward every SGR parameter (including attributes our motion parser doesn't know).
pub(crate) fn render(ansi: &str, cols: u16, offset: (u16, u16), size: (u16, u16)) -> Vec<u8> {
    let mut painter = Painter {
        output: String::from("\x1b[0m\x1b[2J\x1b[H"),
        x: 0,
        y: 0,
        cols,
        offset,
        size,
        previous_visible: false,
    };
    let _ = vte::Parser::new().advance_until_terminated(&mut painter, ansi.as_bytes());
    painter.output.push_str("\x1b[0m");
    painter.output.into_bytes()
}

struct Painter {
    output: String,
    x: u16,
    y: u16,
    cols: u16,
    offset: (u16, u16),
    size: (u16, u16),
    previous_visible: bool,
}

impl vte::Perform for Painter {
    fn terminated(&self) -> bool {
        self.y >= self.offset.1.saturating_add(self.size.1)
    }

    fn print(&mut self, ch: char) {
        let width = ch.width().unwrap_or(0) as u16;
        if width == 0 {
            if self.previous_visible {
                self.output.push(ch);
            }
            return;
        }
        if self.x >= self.cols {
            self.x = 0;
            self.y = self.y.saturating_add(1);
        }
        let visible = self.x >= self.offset.0
            && self.y >= self.offset.1
            && self.x.saturating_add(width) <= self.offset.0.saturating_add(self.size.0)
            && self.y < self.offset.1.saturating_add(self.size.1);
        if visible {
            let _ = write!(
                self.output,
                "\x1b[{};{}H{ch}",
                self.y - self.offset.1 + 1,
                self.x - self.offset.0 + 1
            );
        }
        self.previous_visible = visible;
        self.x = self.x.saturating_add(width);
    }

    fn execute(&mut self, byte: u8) {
        self.previous_visible = false;
        match byte {
            b'\n' => {
                self.y = self.y.saturating_add(1);
                self.x = 0;
            }
            b'\r' => self.x = 0,
            b'\t' => self.x = (self.x / 8 + 1).saturating_mul(8).min(self.cols),
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        // Never replay clipboard writes, terminal queries, or arbitrary control sequences.
        if action != 'm' || !intermediates.is_empty() || ignore {
            return;
        }
        self.output.push_str("\x1b[");
        for (i, group) in params.iter().enumerate() {
            if i != 0 {
                self.output.push(';');
            }
            for (j, value) in group.iter().enumerate() {
                if j != 0 {
                    self.output.push(':');
                }
                let _ = write!(self.output, "{value}");
            }
        }
        self.output.push('m');
    }
}

/// Exported rows include styled blank cells even when plain trailing whitespace is trimmed.
pub(crate) fn exported_width(ansi: &str) -> u16 {
    let mut measure = Measure::default();
    vte::Parser::new().advance(&mut measure, ansi.as_bytes());
    measure.widest
}

#[derive(Default)]
struct Measure {
    col: u16,
    widest: u16,
}

impl vte::Perform for Measure {
    fn print(&mut self, ch: char) {
        self.col = self.col.saturating_add(ch.width().unwrap_or(0) as u16);
        self.widest = self.widest.max(self.col);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' | b'\n' => self.col = 0,
            b'\t' => {
                self.col = (self.col / 8 + 1).saturating_mul(8);
                self.widest = self.widest.max(self.col);
            }
            _ => {}
        }
    }
}

/// Take the older, non-overlapping rows of a page and pad omitted blank rows.
pub(crate) fn page_rows(ansi: &str, rows: u16) -> String {
    let mut lines: Vec<_> = ansi.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines.resize(usize::from(rows), "");
    format!("\x1b[0m{}\x1b[0m", lines.join("\n"))
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
    pub(crate) ansi: String,
    pub(crate) viewport_rows: u16,
    pub(crate) viewport_top: u16,
    #[serde(default)]
    pub(crate) source: Option<Source>,
}

impl Snapshot {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if self.rows == 0 || self.rows == u16::MAX || self.cols == 0 || self.cols > MAX_COLS {
            return Err("invalid snapshot dimensions".into());
        }
        if self.viewport_rows == 0
            || u32::from(self.viewport_top) + u32::from(self.viewport_rows) > u32::from(self.rows)
        {
            return Err("invalid snapshot viewport".into());
        }
        if self.rows > row_limit(self.cols) || self.ansi.len() > MAX_ANSI_BYTES {
            return Err("snapshot exceeds viewer memory limits".into());
        }
        Ok(())
    }
}

pub(crate) fn row_limit(cols: u16) -> u16 {
    // shortcut: 16-bit parser coordinates and a bounded cell grid; use disk-backed storage for larger histories.
    let cells = MAX_GRID_BYTES / std::mem::size_of::<vt100::Cell>();
    (cells / usize::from(cols))
        .saturating_sub(1)
        .min(usize::from(u16::MAX - 1)) as u16
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Source {
    pub(crate) pane_id: String,
    pub(crate) terminal_id: String,
    pub(crate) scroll: Value,
}
