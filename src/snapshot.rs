use std::fmt::Write;
use unicode_width::UnicodeWidthChar;

/// Replay exported cells, not the text-only projection used for motions/copying.
/// Forward every SGR parameter (including attributes our motion parser doesn't know).
pub fn render(ansi: &str, cols: u16, offset: (u16, u16), size: (u16, u16)) -> Vec<u8> {
    let mut painter = Painter {
        output: String::from("\x1b[0m\x1b[2J\x1b[H"),
        x: 0,
        y: 0,
        cols,
        offset,
        size,
        previous_visible: false,
    };
    vte::Parser::new().advance(&mut painter, ansi.as_bytes());
    painter.output.push_str("\x1b[0m");
    painter.output.into_bytes()
}

/// Exported rows include styled blank cells even when plain trailing whitespace is trimmed.
pub fn exported_width(ansi: &str) -> u16 {
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
