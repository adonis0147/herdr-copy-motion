use crate::snapshot;

fn screen(bytes: &[u8], rows: u16, cols: u16) -> vt100::Parser {
    let mut parser = vt100::Parser::new(rows + 1, cols, 0);
    parser.process(bytes);
    parser
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
fn dimensions_reject_invalid_host_values() {
    for value in [
        serde_json::json!(0),
        serde_json::json!(100_000),
        serde_json::Value::Null,
    ] {
        assert!(crate::dimension(&value).is_err());
    }
}
