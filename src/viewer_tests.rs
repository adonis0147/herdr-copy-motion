use super::*;

fn viewer(ansi: &str, rows: u16, cols: u16) -> Viewer {
    let mut parser = vt100::Parser::new(rows + 1, cols, 0);
    parser.process(ansi.replace('\n', "\r\n").as_bytes());
    Viewer {
        snapshot: Snapshot {
            ansi: ansi.to_owned(),
            rows,
            cols,
        },
        parser,
        cursor: (0, 0),
        anchor: None,
        linewise: false,
        offset: (0, 0),
        pending_g: false,
    }
}

fn press(viewer: &mut Viewer, ch: char) -> Action {
    viewer.input(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
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
fn reversed_selection_copies_graphemes_once_without_ansi() {
    let mut v = viewer("\x1b[42m界e\u{301}x\x1b[0m", 1, 8);
    press(&mut v, '$');
    press(&mut v, 'v');
    press(&mut v, '0');
    assert_eq!(v.selected_text(), "界e\u{301}x");
    assert!(matches!(press(&mut v, 'y'), Action::Copy));
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
fn viewport_follows_cursor_without_border_offsets() {
    let mut v = viewer("abcdefgh\n12345678", 2, 8);
    v.cursor = (1, 7);
    v.draw(&mut Vec::new(), (4, 1)).unwrap();
    assert_eq!(v.offset, (4, 1));
    assert_eq!(follow(0, 4, 4), 0);
}
