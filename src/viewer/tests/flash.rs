use super::*;

#[test]
fn labels_jump_to_visible_matches_without_changing_the_query_cursor() {
    let mut v = viewer("foo xx foo", 1, 12);
    search(&mut v, "foo");
    assert_eq!(v.cursor, (0, 0));
    assert_eq!(v.jump.as_ref().unwrap().pattern, "foo");
    let label = v
        .jump
        .as_ref()
        .unwrap()
        .targets
        .iter()
        .find(|target| target.position == (0, 7))
        .unwrap()
        .label
        .unwrap();
    assert!(matches!(press(&mut v, label), Action::Continue));
    assert_eq!(v.cursor, (0, 7));
    assert!(v.jump.is_none());
}

fn search(v: &mut Viewer, pattern: &str) {
    press(v, 's');
    for ch in pattern.chars() {
        press(v, ch);
    }
    assert!(v.jump.is_some());
}

#[test]
fn labels_extend_character_line_and_block_selections_without_copying() {
    for (code, modifiers, expected) in [
        ('v', KeyModifiers::NONE, "bcd\nxxz"),
        ('V', KeyModifiers::NONE, "abcd\nxxz\n"),
        ('v', KeyModifiers::CONTROL, "bc\nxz"),
    ] {
        let mut v = viewer("abcd\nxxz", 2, 6);
        v.cursor = (0, 1);
        v.input(KeyEvent::new(KeyCode::Char(code), modifiers));
        let anchor = v.anchor;
        search(&mut v, "z");
        let label = v.jump.as_ref().unwrap().targets[0].label.unwrap();
        assert!(matches!(press(&mut v, label), Action::Continue));
        assert_eq!(v.anchor, anchor);
        assert_eq!(v.cursor, (1, 2));
        assert_eq!(v.selected_text(), expected);
        assert!(matches!(press(&mut v, 'y'), Action::Copy));
    }
}

#[test]
fn pattern_continuations_and_command_characters_remain_literal() {
    let mut v = viewer("qyvfs", 1, 8);
    press(&mut v, 'v');
    let anchor = v.anchor;
    search(&mut v, "qyvfs");
    assert_eq!(v.jump.as_ref().unwrap().pattern, "qyvfs");
    assert_eq!(v.anchor, anchor);
    assert_eq!(v.cursor, (0, 0));
    assert!(v.pending_find.is_none());
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::Quit
    ));
}

#[test]
fn backspace_recovers_from_no_matches_and_escape_preserves_selection() {
    let mut v = viewer("foo", 1, 8);
    press(&mut v, 'v');
    search(&mut v, "fz");
    assert!(v.jump.as_ref().unwrap().targets.is_empty());
    v.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(v.jump.as_ref().unwrap().pattern, "f");
    assert_eq!(v.jump.as_ref().unwrap().targets.len(), 1);
    v.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(v.jump.as_ref().unwrap().pattern, "");
    assert!(v.jump.as_ref().unwrap().targets.is_empty());
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Continue
    ));
    assert!(v.jump.is_none());
    assert!(v.anchor.is_some());
    assert_eq!(v.cursor, (0, 0));
}

#[test]
fn enter_jumps_to_the_nearest_match_and_never_copies_a_pending_selection() {
    let mut v = viewer("foo\nbar foo", 2, 8);
    v.cursor = (1, 0);
    search(&mut v, "foo");
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Continue
    ));
    assert_eq!(v.cursor, (1, 4));
    assert!(v.jump.is_none());
    press(&mut v, 'v');
    search(&mut v, "zzz");
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Continue
    ));
    assert!(v.anchor.is_some());
    assert!(v.jump.is_none());
    assert!(matches!(
        v.input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Copy
    ));
}

#[test]
fn jumping_uses_only_the_visible_viewport_and_cancels_on_resize_or_paging() {
    let mut v = viewer("foo hidden\nfoo hidden\nfoo hidden", 3, 12);
    v.cursor = (1, 0);
    v.draw(&mut Vec::new(), (3, 1)).unwrap();
    search(&mut v, "foo");
    assert_eq!(v.jump.as_ref().unwrap().targets.len(), 1);
    assert_eq!(v.jump.as_ref().unwrap().targets[0].position, (1, 0));
    v.draw(&mut Vec::new(), (4, 1)).unwrap();
    assert!(v.jump.is_none());
    assert_eq!(v.cursor, (1, 0));
    search(&mut v, "hidden");
    assert!(v.jump.as_ref().unwrap().targets.is_empty());
    v.page(-1);
    assert!(v.jump.is_none());
    assert_eq!(v.cursor, (0, 0));
}

#[test]
fn labels_over_wide_glyphs_restore_original_cells_and_never_enter_copied_text() {
    let mut v = viewer("\x1b[48;2;20;80;30m界foo foo\x1b[0m", 1, 12);
    v.cursor = (0, 8);
    let source = v.snapshot.ansi.clone();
    v.cursor_style = "1;30;46".into();
    v.selection_style = "0;97;44".into();
    search(&mut v, "界");
    let label = v.jump.as_ref().unwrap().targets[0].label.unwrap();
    let mut output = Vec::new();
    v.draw(&mut output, (12, 1)).unwrap();
    let mut replay = vt100::Parser::new(2, 12, 0);
    replay.process(&output);
    assert_eq!(
        replay.screen().cell(0, 0).unwrap().contents(),
        label.to_string()
    );
    assert_eq!(replay.screen().cell(0, 1).unwrap().contents(), " ");
    assert_eq!(v.cell(0, 0).contents(), "界");
    v.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    output.clear();
    v.draw(&mut output, (12, 1)).unwrap();
    replay.process(&output);
    assert_eq!(replay.screen().cell(0, 0).unwrap().contents(), "界");
    assert_eq!(
        replay.screen().cell(0, 0).unwrap().bgcolor(),
        vt100::Color::Rgb(20, 80, 30)
    );
    assert_eq!(v.snapshot.ansi, source);
    v.anchor = Some((0, 0));
    assert_eq!(v.selected_text(), "界foo foo");
}

#[test]
fn flash_dims_context_but_not_matches_or_labels_and_restores_on_exit() {
    for finish in [Some(KeyCode::Esc), Some(KeyCode::Enter), None] {
        let ansi = "\x1b[1;38;2;100;200;150;48;2;20;80;30malpha beta gamma\x1b[0m";
        let mut v = viewer(ansi, 1, 20);
        v.cursor_style = "1;30;46".into();
        v.selection_style = "0;97;44".into();
        v.backdrop_style = "22;2;38;2;106;106;106".into();
        press(&mut v, 'v');
        for _ in 0..3 {
            press(&mut v, 'l');
        }
        press(&mut v, 's');
        let mut output = Vec::new();
        v.draw(&mut output, (20, 1)).unwrap();
        let mut replay = vt100::Parser::new(2, 20, 0);
        replay.process(&output);
        assert!(
            replay.screen().cell(0, 11).unwrap().dim(),
            "the empty-pattern backdrop is already dim"
        );
        for ch in "beta".chars() {
            press(&mut v, ch);
        }
        let jump_label = v.jump.as_ref().unwrap().targets[0].label.unwrap();
        output.clear();
        v.draw(&mut output, (20, 1)).unwrap();
        replay.process(&output);
        let context = replay.screen().cell(0, 11).unwrap();
        assert!(context.dim());
        assert!(!context.bold());
        assert_eq!(context.fgcolor(), vt100::Color::Rgb(106, 106, 106));
        assert_eq!(context.bgcolor(), vt100::Color::Rgb(20, 80, 30));
        assert!(
            replay.screen().cell(0, 1).unwrap().dim(),
            "an existing selection must not compete with labels"
        );
        let matched = replay.screen().cell(0, 7).unwrap();
        assert!(!matched.dim());
        assert_eq!(matched.bgcolor(), vt100::Color::Idx(4));
        let label = replay.screen().cell(0, 6).unwrap();
        assert!(!label.dim());
        assert!(label.bold());
        assert_eq!(label.bgcolor(), vt100::Color::Idx(6));
        v.input(KeyEvent::new(
            finish.unwrap_or(KeyCode::Char(jump_label)),
            KeyModifiers::NONE,
        ));
        assert!(v.jump.is_none());
        output.clear();
        v.draw(&mut output, (20, 1)).unwrap();
        replay.process(&output);
        let restored = replay.screen().cell(0, 11).unwrap();
        assert!(!restored.dim());
        assert!(restored.bold());
        assert_eq!(restored.fgcolor(), vt100::Color::Rgb(100, 200, 150));
        assert_eq!(restored.bgcolor(), vt100::Color::Rgb(20, 80, 30));
        assert!(!replay.screen().cell(0, 1).unwrap().dim());
        assert_eq!(v.snapshot.ansi, ansi);
        assert_eq!(v.cell(0, 6).contents(), "b");
    }
}

#[test]
fn resizing_restores_normal_intensity_when_it_cancels_flash() {
    let mut v = viewer("abc def", 1, 8);
    v.cursor = (0, 6);
    press(&mut v, 's');
    let mut output = Vec::new();
    v.draw(&mut output, (8, 1)).unwrap();
    let mut replay = vt100::Parser::new(2, 8, 0);
    replay.process(&output);
    assert!(replay.screen().cell(0, 0).unwrap().dim());
    output.clear();
    v.draw(&mut output, (7, 1)).unwrap();
    replay.process(&output);
    assert!(v.jump.is_none());
    assert!(!replay.screen().cell(0, 0).unwrap().dim());
}

#[test]
fn character_find_can_still_search_for_the_jump_key() {
    let mut v = viewer("a s", 1, 4);
    press(&mut v, 'f');
    press(&mut v, 's');
    assert_eq!(v.cursor, (0, 2));
    assert!(v.jump.is_none());
}

#[test]
fn uppercase_labels_are_selectable_with_shift() {
    let mut v = viewer(&"foo ".repeat(100), 1, 400);
    search(&mut v, "foo");
    let target = v
        .jump
        .as_ref()
        .unwrap()
        .targets
        .iter()
        .find(|target| target.label == Some('A'))
        .unwrap();
    let expected = target.position;
    v.input(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
    assert_eq!(v.cursor, expected);
    assert!(v.jump.is_none());
}
