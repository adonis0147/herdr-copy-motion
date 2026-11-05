use crate::{
    Result,
    herdr::ansi_text,
    snapshot::{self, Snapshot, Source},
};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

pub(crate) fn load_history(
    snapshot: &Snapshot,
    mut request: impl FnMut(&str, Value) -> Result<Value>,
) -> Result<Option<Snapshot>> {
    let Some(source) = &snapshot.source else {
        return Ok(None);
    };
    let original = source.scroll["offset_from_bottom"].as_u64().unwrap_or(0);
    let history = source.scroll["max_offset_from_bottom"]
        .as_u64()
        .unwrap_or(0)
        .min(u64::from(
            snapshot::row_limit(snapshot.cols).saturating_sub(snapshot.viewport_rows),
        ));
    if history == 0 || original > history {
        return Ok(None);
    }
    let before = request("pane.get", json!({"pane_id": source.pane_id}))?;
    if !source_unchanged(&before["pane"], source) {
        return Ok(None);
    }

    // Only called after painting the overlay: these source scrolls stay hidden.
    let captured = collect_pages(snapshot, source, history, original, &mut request);
    if let Err(restore) = request(
        "pane.scroll",
        json!({
            "pane_id": source.pane_id, "offset_from_bottom": original
        }),
    ) {
        return Err(match captured {
            Err(capture) => format!("{capture}; failed to restore source scroll: {restore}"),
            Ok(_) => format!("failed to restore source scroll: {restore}"),
        }
        .into());
    }
    let Some(expanded) = captured? else {
        return Ok(None);
    };
    let after = request("pane.get", json!({"pane_id": source.pane_id}))?;
    if !source_unchanged(&after["pane"], source) {
        return Ok(None);
    }
    expanded.validate()?;
    Ok(Some(expanded))
}

fn source_unchanged(pane: &Value, source: &Source) -> bool {
    pane["terminal_id"] == source.terminal_id && pane["scroll"] == source.scroll
}

fn collect_pages(
    snapshot: &Snapshot,
    source: &Source,
    history: u64,
    original: u64,
    request: &mut impl FnMut(&str, Value) -> Result<Value>,
) -> Result<Option<Snapshot>> {
    let mut cols = snapshot.cols;
    let mut pages = Vec::new();
    let mut bytes = 0;
    let mut offset = 0u64;
    let mut collected = 0u64;
    loop {
        request(
            "pane.scroll",
            json!({"pane_id": source.pane_id, "offset_from_bottom": offset}),
        )?;
        let ansi = ansi_text(request(
            "pane.read",
            json!({
                "pane_id": source.pane_id, "source": "visible", "format": "ansi", "strip_ansi": false
            }),
        )?)?;
        let width = cols.max(snapshot::exported_width(&ansi));
        if width > snapshot::MAX_COLS {
            return Err("unsupported history width".into());
        }
        let limit = snapshot::row_limit(width);
        if offset + u64::from(snapshot.viewport_rows) > u64::from(limit) {
            break;
        }
        let take = if offset == 0 {
            snapshot.viewport_rows
        } else {
            (offset - collected) as u16
        };
        let page = snapshot::page_rows(&ansi, take);
        let page_bytes = page.len() + usize::from(!pages.is_empty());
        if bytes + page_bytes > snapshot::MAX_ANSI_BYTES {
            break;
        }
        bytes += page_bytes;
        cols = width;
        pages.push(page);
        collected = offset;
        let oldest = history.min(u64::from(limit - snapshot.viewport_rows));
        if offset == oldest {
            break;
        }
        offset = (offset + u64::from(snapshot.viewport_rows)).min(oldest);
    }
    if pages.is_empty() || collected < original {
        return Ok(None);
    }
    pages.reverse();
    Ok(Some(Snapshot {
        rows: snapshot.viewport_rows + collected as u16,
        cols,
        ansi: pages.join("\n"),
        viewport_rows: snapshot.viewport_rows,
        viewport_top: (collected - original) as u16,
        source: None,
    }))
}
