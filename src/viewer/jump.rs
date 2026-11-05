use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests;

const LABELS: &str = "asdfghjklqwertyuiopzxcvbnmASDFGHJKLQWERTYUIOPZXCVBNM";

#[derive(Default)]
pub(super) struct Jump {
    pub(super) pattern: String,
    pub(super) targets: Vec<Target>,
}

pub(super) struct Target {
    pub(super) position: (u16, u16),
    pub(super) end: u16,
    pub(super) label: Option<char>,
}

impl Jump {
    pub(super) fn update(
        &mut self,
        screen: &vt100::Screen,
        rows: u16,
        offset: (u16, u16),
        size: (u16, u16),
        cursor: (u16, u16),
    ) {
        let old_labels: HashMap<_, _> = self
            .targets
            .iter()
            .filter_map(|target| target.label.map(|label| (target.position, label)))
            .collect();
        self.targets.clear();
        if self.pattern.is_empty() {
            return;
        }
        let (_, cols) = screen.size();
        let right = cols.min(offset.0.saturating_add(size.0));
        let bottom = rows.min(offset.1.saturating_add(size.1));
        let mut continuations = HashSet::new();
        for row in offset.1..bottom {
            let mut text = String::new();
            let mut cells = Vec::new();
            for col in offset.0..right {
                let cell = screen.cell(row, col).expect("visible snapshot cell");
                let width = if cell.is_wide() { 2 } else { 1 };
                if cell.is_wide_continuation() || col + width > right {
                    continue;
                }
                cells.push((text.len(), col, width));
                if cell.contents().is_empty() {
                    text.push(' ');
                } else {
                    text.push_str(cell.contents());
                }
            }
            for &(byte, col, _) in &cells {
                if !text[byte..].starts_with(&self.pattern) {
                    continue;
                }
                let end_byte = byte + self.pattern.len();
                let last = cells.partition_point(|&(start, _, _)| start < end_byte) - 1;
                let (_, last_col, width) = cells[last];
                self.targets.push(Target {
                    position: (row, col),
                    end: last_col + width - 1,
                    label: None,
                });
                if let Some(next) = text[end_byte..].chars().next()
                    && LABELS.contains(next)
                {
                    continuations.insert(next);
                }
            }
        }
        let origin = usize::from(cursor.0) * usize::from(cols) + usize::from(cursor.1);
        self.targets.sort_by_key(|target| {
            let position =
                usize::from(target.position.0) * usize::from(cols) + usize::from(target.position.1);
            position.abs_diff(origin)
        });
        // A label must never steal a character that could extend any visible match.
        let mut available: Vec<_> = LABELS
            .chars()
            .filter(|label| !continuations.contains(label))
            .collect();
        for target in &mut self.targets {
            if let Some(&label) = old_labels.get(&target.position)
                && let Some(index) = available.iter().position(|&candidate| candidate == label)
            {
                target.label = Some(label);
                available.remove(index);
            }
        }
        let mut available = available.into_iter();
        for target in &mut self.targets {
            if target.label.is_none() {
                target.label = available.next();
            }
        }
    }
}
