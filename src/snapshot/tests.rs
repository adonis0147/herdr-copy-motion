use super::*;
use crate::snapshot;

fn screen(bytes: &[u8], rows: u16, cols: u16) -> vt100::Parser {
    let mut parser = vt100::Parser::new(rows + 1, cols, 0);
    parser.process(bytes);
    parser
}

#[test]
fn snapshot_validation_rejects_invalid_coordinates_and_oversized_grids() {
    for (rows, cols, viewport_rows, viewport_top) in [
        (0, 8, 1, 0),
        (1, 0, 1, 0),
        (1, MAX_COLS + 1, 1, 0),
        (u16::MAX, 8, 1, 0),
        (2, 8, 0, 0),
        (2, 8, 2, 1),
        (4096, MAX_COLS, 1, 0),
    ] {
        let snapshot = Snapshot {
            rows,
            cols,
            viewport_rows,
            viewport_top,
            ansi: String::new(),
            source: None,
        };
        assert!(
            snapshot.validate().is_err(),
            "accepted {rows}x{cols} with viewport {viewport_top}+{viewport_rows}"
        );
    }
    let snapshot = Snapshot {
        rows: 1,
        cols: 8,
        viewport_rows: 1,
        viewport_top: 0,
        ansi: "valid".into(),
        source: None,
    };
    assert!(snapshot.validate().is_ok());
    let oversized = Snapshot {
        ansi: "x".repeat(MAX_ANSI_BYTES + 1),
        ..snapshot
    };
    assert!(oversized.validate().is_err());
}

#[test]
fn renderer_stops_before_styles_below_the_viewport() {
    let ansi = "visible\n\x1b[31mbelow\n\x1b[32mstill below";
    let output = snapshot::render(ansi, 20, (0, 0), (20, 1));
    assert!(!String::from_utf8_lossy(&output).contains("[31m"));
    assert_eq!(screen(&output, 1, 20).screen().contents(), "visible");
}

#[test]
fn replay_preserves_every_exported_cell_including_colored_blanks() {
    let ansi = "\x1b[48;2;20;80;30m        \x1b[0m\r\n\x1b[1;3;4;38;2;100;200;150;48;5;22mtool    \x1b[0m\r\n\x1b[7m界e\u{301}     \x1b[0m";
    let original = screen(ansi.as_bytes(), 3, 8);
    let rendered = snapshot::render(ansi, 8, (0, 0), (8, 3));
    let replay = screen(&rendered, 3, 8);
    for row in 0..3 {
        for col in 0..8 {
            assert_eq!(
                original.screen().cell(row, col),
                replay.screen().cell(row, col),
                "cell {row},{col}"
            );
        }
    }
}

#[test]
fn renderer_forwards_styles_not_understood_by_the_motion_parser() {
    let ansi = "\x1b[9;4:3;58:2::20:80:30mtext\x1b[0m";
    let rendered = String::from_utf8(snapshot::render(ansi, 20, (0, 0), (20, 3))).unwrap();
    assert!(rendered.contains("\x1b[9;4:3;58:2:0:20:80:30m"));
}

#[test]
fn clipping_preserves_style_carried_from_hidden_rows() {
    let ansi = "\x1b[48;5;22mabcdefgh\r\n12345678\x1b[0m";
    let replay = screen(&snapshot::render(ansi, 8, (2, 1), (4, 1)), 1, 4);
    assert_eq!(replay.screen().contents().trim(), "3456");
    for col in 0..4 {
        assert_eq!(
            replay.screen().cell(0, col).unwrap().bgcolor(),
            vt100::Color::Idx(22)
        );
    }
}

#[test]
fn terminal_control_payloads_are_not_replayed() {
    let ansi = "a\x1b]52;c;c2VjcmV0\x07\x1b]0;title\x07\x1b[6nb";
    let bytes = snapshot::render(ansi, 8, (0, 0), (8, 2));
    let output = String::from_utf8(bytes.clone()).unwrap();
    assert!(!output.contains("]52"));
    assert!(!output.contains("title"));
    assert!(!output.contains("[6n"));
    assert_eq!(screen(&bytes, 2, 8).screen().contents().trim(), "ab");
}

#[test]
fn resizing_does_not_reflow_captured_rows() {
    let ansi = "abcdefgh\r\n12345678";
    let small = screen(&snapshot::render(ansi, 8, (0, 0), (4, 2)), 2, 4);
    assert_eq!(small.screen().contents().trim(), "abcd\n1234");
    let wide = screen(&snapshot::render(ansi, 8, (0, 0), (12, 2)), 2, 12);
    assert_eq!(wide.screen().contents().trim(), "abcdefgh\n12345678");
}

#[test]
fn history_pages_pad_blanks_and_preserve_styles() {
    assert_eq!(snapshot::page_rows("one\n", 3), "\x1b[0mone\n\n\x1b[0m");
    let ansi = snapshot::page_rows("\x1b[42m  \x1b[0m\nlast\n", 1);
    let replay = screen(&snapshot::render(&ansi, 4, (0, 0), (4, 1)), 1, 4);
    assert_eq!(
        replay.screen().cell(0, 1).unwrap().bgcolor(),
        vt100::Color::Idx(2)
    );
    assert!(!ansi.contains("last"));
}
