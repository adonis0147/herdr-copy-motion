use super::load_history;
use crate::snapshot::{self, Snapshot, Source};

fn screen(bytes: &[u8], rows: u16, cols: u16) -> vt100::Parser {
    let mut parser = vt100::Parser::new(rows + 1, cols, 0);
    parser.process(bytes);
    parser
}

#[test]
fn hidden_history_capture_exceeds_1000_rows_without_duplicates_and_restores_scroll() {
    for history in [5, 1201] {
        let initial = history_snapshot(history, 1);
        let mut offset = 1;
        let expanded = load_history(&initial, |method, params| {
            assert_eq!(params["pane_id"], "source");
            match method {
                "pane.get" => Ok(serde_json::json!({"pane": {
                    "terminal_id": "source-terminal", "scroll": {
                        "viewport_rows": 3, "max_offset_from_bottom": history, "offset_from_bottom": offset
                    }
                }})),
                "pane.scroll" => {
                    offset = params["offset_from_bottom"].as_u64().unwrap();
                    assert!(offset <= history, "must not scroll beyond Herdr's retained buffer");
                    Ok(serde_json::json!({}))
                }
                "pane.read" => {
                    assert_eq!(params["source"], "visible");
                    let start = history - offset;
                    let first = if history == 5 && start == 0 { "123456789".into() } else { start.to_string() };
                    Ok(serde_json::json!({"read": {"format": "ansi", "truncated": false,
                        "text": format!("{first}\n{}\n{}\n", start + 1, start + 2)
                    }}))
                }
                _ => panic!("unexpected request: {method}"),
            }
        }).unwrap().unwrap();
        assert_eq!(offset, 1);
        assert_eq!(expanded.rows, history as u16 + 3);
        assert_eq!(expanded.viewport_top, history as u16 - 1);
        assert_eq!(expanded.cols, if history == 5 { 9 } else { 8 });
        let replay = screen(
            &snapshot::render(
                &expanded.ansi,
                expanded.cols,
                (0, 0),
                (expanded.cols, expanded.rows),
            ),
            expanded.rows,
            expanded.cols,
        );
        let expected = (0..history + 3)
            .map(|row| {
                if history == 5 && row == 0 {
                    "123456789".into()
                } else {
                    row.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(replay.screen().contents(), expected);
    }
}

fn history_snapshot(history: u64, original: u64) -> Snapshot {
    Snapshot {
        rows: 3,
        cols: 8,
        ansi: "visible".into(),
        viewport_rows: 3,
        viewport_top: 0,
        source: Some(Source {
            pane_id: "source".into(),
            terminal_id: "source-terminal".into(),
            scroll: serde_json::json!({"viewport_rows": 3, "offset_from_bottom": original, "max_offset_from_bottom": history}),
        }),
    }
}

#[test]
fn failed_hidden_page_read_restores_source_scroll() {
    let mut offset = 1;
    let initial = history_snapshot(5, 1);
    let result = load_history(&initial, |method, params| match method {
        "pane.get" => Ok(
            serde_json::json!({"pane": {"terminal_id": "source-terminal", "scroll": initial.source.as_ref().unwrap().scroll}}),
        ),
        "pane.scroll" => {
            offset = params["offset_from_bottom"].as_u64().unwrap();
            Ok(serde_json::json!({}))
        }
        "pane.read" => Err("page read failed".into()),
        _ => panic!("unexpected request: {method}"),
    });
    assert!(result.is_err());
    assert_eq!(offset, 1);
}

#[test]
fn capture_and_restore_failures_are_both_reported() {
    let initial = history_snapshot(5, 1);
    let result = load_history(&initial, |method, params| match method {
        "pane.get" => Ok(serde_json::json!({"pane": {
            "terminal_id": "source-terminal", "scroll": initial.source.as_ref().unwrap().scroll
        }})),
        "pane.scroll" if params["offset_from_bottom"] == 0 => Ok(serde_json::json!({})),
        "pane.scroll" => Err("restore failed".into()),
        "pane.read" => Err("page failed".into()),
        _ => panic!("unexpected request: {method}"),
    });
    let error = result.err().unwrap().to_string();
    assert!(error.contains("page failed"), "{error}");
    assert!(error.contains("restore failed"), "{error}");
}

#[test]
fn changed_source_is_not_scrolled() {
    let initial = history_snapshot(5, 1);
    let result = load_history(&initial, |method, _| {
        assert_eq!(method, "pane.get");
        Ok(serde_json::json!({"pane": {"terminal_id": "other-terminal"}}))
    })
    .unwrap();
    assert!(result.is_none());
}

#[test]
fn history_grid_respects_coordinate_and_memory_limits() {
    assert!(snapshot::row_limit(80) > 1000);
    for cols in [1, 80, 4096] {
        let rows = snapshot::row_limit(cols);
        assert!(rows > 0);
        assert!(rows < u16::MAX);
        assert!(
            (usize::from(rows) + 1) * usize::from(cols) * std::mem::size_of::<vt100::Cell>()
                <= snapshot::MAX_GRID_BYTES
        );
    }
}
