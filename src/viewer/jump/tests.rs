use super::*;

fn screen(ansi: &str, rows: u16, cols: u16) -> vt100::Parser {
    let mut parser = vt100::Parser::new(rows + 1, cols, 0);
    parser.process(ansi.replace('\n', "\r\n").as_bytes());
    parser
}

#[test]
fn visible_matches_use_source_cells_and_keep_unicode_glyphs_whole() {
    let parser = screen("a界x e\u{301}\nelsewhere", 2, 12);
    let mut jump = Jump {
        pattern: "界x".into(),
        ..Jump::default()
    };
    jump.update(parser.screen(), 2, (0, 0), (4, 1), (0, 0));
    assert_eq!(jump.targets.len(), 1);
    assert_eq!(jump.targets[0].position, (0, 1));
    assert_eq!(jump.targets[0].end, 3);
    jump.update(parser.screen(), 2, (2, 0), (4, 1), (0, 2));
    assert!(
        jump.targets.is_empty(),
        "a clipped wide glyph is not a jump target"
    );
    jump.pattern = "e\u{301}".into();
    jump.update(parser.screen(), 2, (0, 0), (12, 1), (0, 0));
    assert_eq!(jump.targets[0].position, (0, 5));
    assert_eq!(jump.targets[0].end, 5);
    jump.pattern = "\u{301}".into();
    jump.update(parser.screen(), 2, (0, 0), (12, 1), (0, 0));
    assert!(
        jump.targets.is_empty(),
        "matches cannot start inside a grapheme"
    );
}

#[test]
fn labels_never_steal_a_valid_pattern_continuation() {
    let parser = screen("ab ac ad ax", 1, 16);
    let mut jump = Jump {
        pattern: "a".into(),
        ..Jump::default()
    };
    jump.update(parser.screen(), 1, (0, 0), (16, 1), (0, 0));
    assert_eq!(jump.targets.len(), 4);
    let labels: Vec<_> = jump
        .targets
        .iter()
        .filter_map(|target| target.label)
        .collect();
    assert_eq!(labels.len(), 4);
    assert!(labels.iter().all(|label| !"bcdx".contains(*label)));
    assert_eq!(labels.iter().collect::<HashSet<_>>().len(), 4);
}

#[test]
fn labels_are_reused_unless_they_become_valid_query_characters() {
    let parser = screen("foo food fool", 1, 16);
    let mut jump = Jump {
        pattern: "f".into(),
        ..Jump::default()
    };
    jump.update(parser.screen(), 1, (0, 0), (16, 1), (0, 0));
    let original: Vec<_> = jump.targets.iter().map(|target| target.label).collect();
    assert_eq!(original, vec![Some('a'), Some('s'), Some('d')]);
    jump.pattern = "fo".into();
    jump.update(parser.screen(), 1, (0, 0), (16, 1), (0, 0));
    assert_eq!(
        jump.targets
            .iter()
            .map(|target| target.label)
            .collect::<Vec<_>>(),
        original
    );
    jump.pattern = "foo".into();
    jump.update(parser.screen(), 1, (0, 0), (16, 1), (0, 0));
    assert_eq!(jump.targets[0].label, Some('a'));
    assert_eq!(jump.targets[1].label, Some('s'));
    assert!(
        jump.targets
            .iter()
            .filter_map(|target| target.label)
            .all(|label| !"dl".contains(label))
    );
}

#[test]
fn nearest_targets_get_labels_and_unlabeled_matches_still_reserve_continuations() {
    let text = format!("{}fooa", "foo ".repeat(60));
    let parser = screen(&text, 1, 244);
    let mut jump = Jump {
        pattern: "foo".into(),
        ..Jump::default()
    };
    jump.update(parser.screen(), 1, (0, 0), (244, 1), (0, 0));
    assert_eq!(jump.targets.len(), 61);
    assert_eq!(jump.targets[0].position, (0, 0));
    let labels: Vec<_> = jump
        .targets
        .iter()
        .filter_map(|target| target.label)
        .collect();
    assert_eq!(labels.len(), 51);
    assert!(!labels.contains(&'a'));
    assert_eq!(labels.iter().collect::<HashSet<_>>().len(), labels.len());
    jump.pattern.push('a');
    jump.update(parser.screen(), 1, (0, 0), (244, 1), (0, 0));
    assert_eq!(jump.targets.len(), 1);
    assert_eq!(jump.targets[0].position, (0, 240));
}

#[test]
fn search_is_case_sensitive_row_local_and_excludes_the_parser_spare_row() {
    let parser = screen("Foo\nfoo", 2, 8);
    let mut jump = Jump {
        pattern: "Foo".into(),
        ..Jump::default()
    };
    jump.update(parser.screen(), 2, (0, 0), (8, 20), (0, 0));
    assert_eq!(jump.targets.len(), 1);
    assert_eq!(jump.targets[0].position, (0, 0));
    jump.pattern = "ofoo".into();
    jump.update(parser.screen(), 2, (0, 0), (8, 20), (0, 0));
    assert!(jump.targets.is_empty());
    jump.pattern = " ".into();
    jump.update(parser.screen(), 2, (0, 0), (8, 20), (0, 0));
    assert!(jump.targets.iter().all(|target| target.position.0 < 2));
}
