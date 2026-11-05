mod flash;

use super::*;

fn viewer(ansi: &str, rows: u16, cols: u16) -> Viewer {
    let mut viewer = Viewer::new(Snapshot {
        ansi: ansi.to_owned(),
        rows,
        cols,
        viewport_rows: rows,
        viewport_top: 0,
        source: None,
    });
    viewer.cursor = (0, 0);
    viewer
}

fn press(viewer: &mut Viewer, ch: char) -> Action {
    viewer.input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
}

#[test]
fn current_cursor_fallback_stays_in_original_viewport() {
    let v = Viewer::new(Snapshot {
        ansi: "old\nolder\nvisible\nprompt".into(),
        rows: 4,
        cols: 10,
        viewport_rows: 2,
        viewport_top: 2,
        source: None,
    });
    assert_eq!(v.cursor, (3, 0));
    assert_eq!(v.offset, (0, 2));
}

#[test]
fn paging_uses_viewer_height_and_preserves_selection_across_history() {
    let mut v = viewer("0\n1\n2\n3\n4\n5\n6\n7", 8, 4);
    v.cursor = (6, 0);
    v.offset.1 = 4;
    v.draw(&mut Vec::new(), (4, 4)).unwrap();
    press(&mut v, 'v');
    v.input(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert_eq!(v.cursor, (2, 0));
    assert_eq!(v.offset.1, 0);
    assert_eq!(v.selected_text(), "2\n3\n4\n5\n6");
    v.input(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(v.cursor, (4, 0));
    assert_eq!(v.offset.1, 2);
    v.page(100);
    assert_eq!(v.cursor, (7, 0));
    assert_eq!(v.offset.1, 4);
    v.page(-100);
    assert_eq!(v.cursor, (0, 0));
    assert_eq!(v.offset.1, 0);
}

#[test]
fn motions_clamp_and_skip_wide_continuations() {
    let mut v = viewer("界abc\nnext", 2, 8);
    press(&mut v, 'h');
    assert_eq!(v.cursor, (0, 0));
    press(&mut v, 'l');
    assert_eq!(v.cursor, (0, 2));
    press(&mut v, 'h');
    assert_eq!(v.cursor, (0, 0));
    press(&mut v, 'G');
    press(&mut v, 'j');
    assert_eq!(v.cursor.0, 1);
    press(&mut v, 'g');
    press(&mut v, 'g');
    assert_eq!(v.cursor.0, 0);
}

#[test]
fn character_find_and_till_motions_are_line_local() {
    for (command, start, expected) in [('f', 0, 2), ('F', 6, 2), ('t', 0, 1), ('T', 6, 3)] {
        let mut v = viewer("a:x:y:x\nx", 2, 8);
        v.cursor.1 = start;
        assert!(matches!(press(&mut v, command), Action::Continue));
        assert_eq!(v.cursor, (0, start));
        press(&mut v, 'x');
        assert_eq!(v.cursor, (0, expected), "{command}x");
    }
    let mut v = viewer("abc\naz", 2, 8);
    press(&mut v, 'f');
    press(&mut v, 'z');
    assert_eq!(v.cursor, (0, 0));
    press(&mut v, 'j');
    press(&mut v, ';');
    assert_eq!(v.cursor, (1, 1), "a failed find is still remembered");
}

#[test]
fn character_find_repeats_preserve_the_original_direction() {
    let mut v = viewer("x ax bx cx", 1, 10);
    press(&mut v, ';');
    press(&mut v, ',');
    assert_eq!(v.cursor, (0, 0));
    press(&mut v, 'f');
    press(&mut v, 'x');
    for (key, expected) in [(';', 6), (',', 3), (';', 6)] {
        press(&mut v, key);
        assert_eq!(v.cursor, (0, expected));
    }
    press(&mut v, 'F');
    press(&mut v, 'x');
    assert_eq!(v.cursor, (0, 3));
    for (key, expected) in [(';', 0), (',', 3), (';', 0)] {
        press(&mut v, key);
        assert_eq!(v.cursor, (0, expected));
    }
}

#[test]
fn till_repeats_advance_past_the_previous_target() {
    for (command, start, target, destinations) in [
        ('t', 0, 'x', vec![1, 3, 5, 3, 5]),
        ('T', 6, 'x', vec![5, 3, 1, 3, 1]),
    ] {
        let mut v = viewer("x-x-x-x", 1, 7);
        v.cursor.1 = start;
        press(&mut v, command);
        press(&mut v, target);
        assert_eq!(v.cursor.1, destinations[0]);
        for (key, expected) in [';', ';', ',', ';'].into_iter().zip(&destinations[1..]) {
            press(&mut v, key);
            assert_eq!(v.cursor.1, *expected, "{command}x then {key}");
        }
    }
    for (text, start, command, expected) in [("ax-x", 0, 't', 2), ("x-xa", 3, 'T', 1)] {
        let mut v = viewer(text, 1, 4);
        v.cursor.1 = start;
        press(&mut v, command);
        press(&mut v, 'x');
        assert_eq!(v.cursor.1, start, "an adjacent till target does not move");
        press(&mut v, ';');
        assert_eq!(v.cursor.1, expected);
    }
}

#[test]
fn character_find_targets_are_literal_including_command_keys_and_spaces() {
    let text = "aqyvft;,g z";
    for target in ['q', 'y', 'v', 'f', 't', ';', ',', 'g', ' '] {
        let mut v = viewer(text, 1, 16);
        press(&mut v, 'f');
        assert!(matches!(press(&mut v, target), Action::Continue));
        assert_eq!(usize::from(v.cursor.1), text.find(target).unwrap());
        assert!(v.anchor.is_none());
    }
    let mut v = viewer("aX", 1, 8);
    press(&mut v, 'f');
    v.input(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT));
    assert_eq!(v.cursor, (0, 1));
    press(&mut v, 'f');
    press(&mut v, ' ');
    assert_eq!(
        v.cursor,
        (0, 1),
        "implicit blank padding is not a character target"
    );
}

#[test]
fn character_find_preserves_wide_glyphs_and_selection() {
    let mut v = viewer("a界b界c", 1, 9);
    press(&mut v, 'v');
    press(&mut v, 'f');
    press(&mut v, '界');
    assert_eq!(v.cursor, (0, 1));
    assert_eq!(v.selected_text(), "a界");
    press(&mut v, ';');
    assert_eq!(v.cursor, (0, 4));
    assert_eq!(v.selected_text(), "a界b界");
    press(&mut v, ',');
    assert_eq!(v.cursor, (0, 1));
    v.cursor = (0, 6);
    press(&mut v, 'T');
    press(&mut v, '界');
    assert_eq!(v.cursor, (0, 6));
    press(&mut v, ';');
    assert_eq!(v.cursor, (0, 3), "T skips the whole target glyph");
    let mut v = viewer("界x", 1, 3);
    press(&mut v, 't');
    press(&mut v, 'x');
    assert_eq!(
        v.cursor,
        (0, 0),
        "t stops on the leading cell of the prior wide glyph"
    );
    press(&mut v, ';');
    assert_eq!(v.cursor, (0, 0));
    let mut v = viewer("a e\u{301}", 1, 6);
    press(&mut v, 'v');
    press(&mut v, 'f');
    press(&mut v, 'e');
    assert_eq!(v.selected_text(), "a e\u{301}");
}

#[test]
fn escape_cancels_pending_character_find_without_closing_or_clearing_selection() {
    let mut v = viewer("abc", 1, 4);
    press(&mut v, 'v');
    press(&mut v, 'f');
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Continue
    ));
    assert!(v.anchor.is_some());
    assert!(v.pending_find.is_none());
    press(&mut v, 'l');
    assert_eq!(v.cursor, (0, 1));
    press(&mut v, 'T');
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Continue
    ));
    assert!(
        v.anchor.is_some(),
        "Enter while awaiting a target must not copy"
    );
    press(&mut v, 'f');
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::Quit
    ));
}

#[test]
fn word_and_line_motions() {
    let mut v = viewer("  one two.three", 1, 20);
    press(&mut v, '^');
    assert_eq!(v.cursor.1, 2);
    press(&mut v, 'w');
    assert_eq!(v.cursor.1, 6);
    press(&mut v, 'e');
    assert_eq!(v.cursor.1, 8);
    press(&mut v, 'b');
    assert_eq!(v.cursor.1, 6);
    press(&mut v, '$');
    assert_eq!(v.cursor.1, 14);
}

#[test]
fn word_motions_treat_wide_glyphs_as_whole_characters() {
    let mut v = viewer("界語 ab", 1, 10);
    press(&mut v, 'w');
    assert_eq!(v.cursor, (0, 5));
    press(&mut v, 'b');
    assert_eq!(v.cursor, (0, 0));
    press(&mut v, 'v');
    press(&mut v, 'e');
    assert_eq!(v.cursor, (0, 2));
    assert_eq!(v.selected_text(), "界語");
    press(&mut v, 'e');
    assert_eq!(v.cursor, (0, 6));
}

#[test]
fn word_motions_stay_on_a_wide_glyph_at_the_source_boundary() {
    let mut v = viewer("ab界", 1, 4);
    press(&mut v, 'e');
    assert_eq!(v.cursor, (0, 2));
    press(&mut v, 'e');
    assert_eq!(v.cursor, (0, 2));
    press(&mut v, 'w');
    assert_eq!(v.cursor, (0, 2));
    press(&mut v, 'b');
    assert_eq!(v.cursor, (0, 0));
}

#[test]
fn reversed_selection_copies_graphemes_once_without_ansi() {
    let mut v = viewer("\x1b[42m界e\u{301}x\x1b[0m", 1, 8);
    press(&mut v, '$');
    press(&mut v, 'v');
    press(&mut v, '0');
    assert_eq!(v.selected_text(), "界e\u{301}x");
    assert!(matches!(press(&mut v, 'y'), Action::Copy));
}

#[test]
fn block_selection_uses_the_same_rectangle_in_all_four_directions() {
    for (anchor, cursor) in [
        ((0, 1), (2, 3)),
        ((0, 3), (2, 1)),
        ((2, 1), (0, 3)),
        ((2, 3), (0, 1)),
    ] {
        let mut v = viewer("abcdef\nABCDEF\n012345", 3, 8);
        v.cursor = anchor;
        v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        v.cursor = cursor;
        assert_eq!(v.selection_mode, SelectionMode::Block);
        assert_eq!(v.selected_text(), "bcd\nBCD\n123");
        for row in 0..3 {
            for col in 0..8 {
                assert_eq!(
                    v.selected(row, col),
                    (1..=3).contains(&col),
                    "cell {row},{col}"
                );
            }
        }
        assert!(matches!(press(&mut v, 'y'), Action::Copy));
    }
}

#[test]
fn block_selection_preserves_trailing_spaces_and_blank_rows() {
    let mut v = viewer("abcd\nx\n\nab  ", 4, 6);
    v.cursor = (0, 1);
    v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    v.cursor = (3, 3);
    assert_eq!(v.selected_text(), "bcd\n   \n   \nb  ");
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Copy
    ));
}

#[test]
fn block_selection_toggle_preserves_the_existing_anchor() {
    let mut v = viewer("abcd\nABCD", 2, 6);
    v.cursor = (0, 1);
    press(&mut v, 'v');
    press(&mut v, 'j');
    press(&mut v, 'l');
    press(&mut v, 'l');
    assert_eq!(v.selected_text(), "bcd\nABCD");
    v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert_eq!(v.anchor, Some((0, 1)));
    assert_eq!(v.selected_text(), "bcd\nBCD");
    v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert_eq!(v.selection_mode, SelectionMode::Character);
    assert_eq!(v.anchor, Some((0, 1)));
    assert_eq!(v.selected_text(), "bcd\nABCD");
    press(&mut v, 'V');
    assert_eq!(v.selection_mode, SelectionMode::Line);
    v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    assert_eq!(v.selection_mode, SelectionMode::Block);
    assert_eq!(v.selected_text(), "D");
}

#[test]
fn cancelling_block_selection_preserves_existing_cancel_controls() {
    for key in [KeyCode::Char('v'), KeyCode::Char(' '), KeyCode::Esc] {
        let mut v = viewer("one\ntwo", 2, 6);
        v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        press(&mut v, 'j');
        assert!(matches!(
            v.input(KeyEvent::new(key, KeyModifiers::NONE)),
            Action::Continue
        ));
        assert!(v.anchor.is_none());
        assert!(!v.selected(0, 0));
        assert_eq!(v.selected_text(), "");
        v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        assert_eq!(
            v.selection_mode,
            SelectionMode::Block,
            "a new selection must start in block mode"
        );
        assert_eq!(v.anchor, Some(v.cursor));
    }
}

#[test]
fn block_selection_keeps_touched_wide_glyphs_and_combining_marks_whole() {
    for (column, expected) in [(0, "a\n界\na"), (1, "b\n界\ne\u{301}")] {
        let mut v = viewer("abcde\n\x1b[48;2;20;80;30m界xy\x1b[0m\nae\u{301}bc", 3, 6);
        v.selection_style = "0;97;44".into();
        v.cursor = (0, column);
        v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
        v.cursor = (2, column);
        assert_eq!(v.selected_text(), expected);
        assert!(v.selected(1, 0));
        assert!(v.selected(1, 1));
        assert!(!v.selected(1, 2));
        let mut output = Vec::new();
        v.draw(&mut output, (6, 3)).unwrap();
        let mut replay = vt100::Parser::new(4, 6, 0);
        replay.process(&output);
        assert_eq!(replay.screen().cell(1, 0).unwrap().contents(), "界");
        assert_eq!(
            replay.screen().cell(1, 0).unwrap().bgcolor(),
            vt100::Color::Idx(4)
        );
        v.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        output.clear();
        v.draw(&mut output, (6, 3)).unwrap();
        replay.process(&output);
        assert_eq!(
            replay.screen().cell(1, 0).unwrap().bgcolor(),
            vt100::Color::Rgb(20, 80, 30)
        );
    }
}

#[test]
fn block_selection_survives_paging_character_finds_and_viewport_clipping() {
    let mut v = viewer("abcde\nABCDE\n01234\nvwxyz\n.....\nlast", 6, 8);
    v.cursor = (0, 1);
    v.draw(&mut Vec::new(), (8, 2)).unwrap();
    v.input(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL));
    v.input(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    v.input(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    press(&mut v, 'l');
    press(&mut v, 'f');
    press(&mut v, 'y');
    v.draw(&mut Vec::new(), (2, 2)).unwrap();
    assert_eq!(v.offset, (2, 3));
    assert_eq!(v.selected_text(), "bcd\nBCD\n123\nwxy");
}

#[test]
fn linewise_selection_and_escape() {
    let mut v = viewer("one\ntwo", 2, 8);
    press(&mut v, 'V');
    press(&mut v, 'j');
    assert_eq!(v.selected_text(), "one\ntwo\n");
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Continue
    ));
    assert!(v.anchor.is_none());
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Quit
    ));
}

#[test]
fn scrolling_publishes_only_complete_synchronized_frames() {
    #[derive(Default)]
    struct Output {
        writes: Vec<Vec<u8>>,
        flushes: usize,
    }
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes.push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
    let mut v = viewer("old\nnew", 2, 4);
    let mut output = Output::default();
    v.draw(&mut output, (4, 1)).unwrap();
    v.page(1);
    v.draw(&mut output, (4, 1)).unwrap();
    assert_eq!(output.writes.len(), 2);
    assert_eq!(output.flushes, 2);
    for frame in &output.writes {
        assert!(frame.starts_with(b"\x1b[?2026h"));
        assert!(frame.ends_with(b"\x1b[?2026l"));
        assert!(frame.windows(4).any(|bytes| bytes == b"\x1b[2J"));
    }
    let mut replay = vt100::Parser::new(2, 4, 0);
    replay.process(&output.writes[1]);
    assert_eq!(replay.screen().contents(), "new");
    assert_eq!(v.cursor, (1, 0));
}

#[test]
fn cursor_uses_theme_colors_and_bold() {
    let mut v = viewer("x", 1, 4);
    v.cursor_style = "1;38;2;48;48;48;44".into();
    let mut output = Vec::new();
    v.draw(&mut output, (4, 1)).unwrap();
    let mut replay = vt100::Parser::new(2, 4, 0);
    replay.process(&output);
    let cell = replay.screen().cell(0, 0).unwrap();
    assert_eq!(cell.fgcolor(), vt100::Color::Rgb(48, 48, 48));
    assert_eq!(cell.bgcolor(), vt100::Color::Idx(4));
    assert!(cell.bold());
}

#[test]
fn selection_uses_native_gray_style_without_changing_cursor_color() {
    let mut v = viewer("ab", 1, 4);
    v.cursor_style = "1;30;46".into();
    v.selection_style = "0;38;2;255;255;255;48;2;106;106;106".into();
    press(&mut v, 'v');
    press(&mut v, 'l');
    let mut output = Vec::new();
    v.draw(&mut output, (4, 1)).unwrap();
    let mut replay = vt100::Parser::new(2, 4, 0);
    replay.process(&output);
    let selected = replay.screen().cell(0, 0).unwrap();
    assert_eq!(selected.bgcolor(), vt100::Color::Rgb(106, 106, 106));
    assert_eq!(selected.fgcolor(), vt100::Color::Rgb(255, 255, 255));
    assert!(!selected.bold());
    let cursor = replay.screen().cell(0, 1).unwrap();
    assert_eq!(cursor.bgcolor(), vt100::Color::Idx(6));
    assert!(cursor.bold());
}

#[test]
fn clearing_selection_restores_original_cell_styles() {
    let mut v = viewer("\x1b[48;2;20;80;30m        \x1b[0m", 1, 8);
    let source = v.snapshot.ansi.clone();
    press(&mut v, 'v');
    press(&mut v, 'l');
    let mut selected = Vec::new();
    v.draw(&mut selected, (8, 1)).unwrap();
    press(&mut v, 'v');
    let mut cleared = Vec::new();
    v.draw(&mut cleared, (8, 1)).unwrap();
    let mut replay = vt100::Parser::new(2, 8, 0);
    replay.process(&cleared);
    assert_eq!(
        replay.screen().cell(0, 0).unwrap().bgcolor(),
        vt100::Color::Rgb(20, 80, 30)
    );
    assert_eq!(v.snapshot.ansi, source);
    assert_ne!(selected, cleared);
}

#[test]
fn viewport_keeps_the_whole_wide_cursor_glyph_visible() {
    let mut v = viewer("ab界x", 1, 5);
    v.cursor_style = "1;97;44".into();
    press(&mut v, 'l');
    press(&mut v, 'l');
    press(&mut v, 'v');
    let mut output = Vec::new();
    v.draw(&mut output, (3, 1)).unwrap();
    assert_eq!(v.offset, (1, 0));
    let mut replay = vt100::Parser::new(2, 3, 0);
    replay.process(&output);
    let cell = replay.screen().cell(0, 1).unwrap();
    assert_eq!(cell.contents(), "界");
    assert!(cell.bold());
    assert_eq!(cell.bgcolor(), vt100::Color::Idx(4));
    assert_eq!(v.selected_text(), "界");
}

#[test]
fn viewport_follows_cursor_without_border_offsets() {
    let mut v = viewer("abcdefgh\n12345678", 2, 8);
    v.cursor = (1, 7);
    v.draw(&mut Vec::new(), (4, 1)).unwrap();
    assert_eq!(v.offset, (4, 1));
    assert_eq!(follow(0, 4, 4), 0);
}
