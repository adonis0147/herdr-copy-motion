use super::*;

fn style_from_config(contents: &str) -> String {
    styles_from_config(contents).cursor
}

#[test]
fn selection_uses_native_gray_tint_and_contrast() {
    assert_eq!(
        selection_style(Color::Rgb(48, 48, 48)),
        "0;38;2;255;255;255;48;2;106;106;106"
    );
    assert_eq!(
        selection_style(Color::Rgb(0, 0, 0)),
        "0;38;2;255;255;255;48;2;71;71;71"
    );
    assert_eq!(
        selection_style(Color::Rgb(255, 255, 255)),
        "0;38;2;0;0;0;48;2;184;184;184"
    );
}

#[test]
fn selection_uses_native_panel_fallback_for_reset_backgrounds() {
    let terminal = styles_from_config("[theme]\nname = 'terminal'");
    assert_eq!(terminal.cursor, "1;90;44");
    assert_eq!(terminal.selection, "0;97;100");
    let custom = styles_from_config("[theme.custom]\npanel_bg = '#303030'");
    assert_eq!(custom.selection, selection_style(Color::Rgb(48, 48, 48)));
    assert_eq!(selection_style(Color::Reset), "0;39;49");
}

#[test]
fn defaults_and_bad_config() {
    for config in [
        "",
        "not toml",
        "[theme]\nname = 42",
        "[theme]\nname = 'unknown'",
    ] {
        assert_eq!(
            style_from_config(config),
            "1;38;2;24;24;37;48;2;137;180;250"
        );
    }
}

#[test]
fn terminal_and_reset_contrast() {
    assert_eq!(style_from_config("[theme]\nname = 'terminal'"), "1;90;44");
    assert_eq!(
        style_from_config(
            "[theme.custom]\npanel_bg = 'reset'\nsurface_dim = '#abc'\naccent = 'default'"
        ),
        "1;38;2;170;187;204;49"
    );
    assert_eq!(
        style_from_config("[theme.custom]\npanel_bg = 'none'"),
        "1;38;2;30;30;46;48;2;137;180;250"
    );
}

#[test]
fn override_precedence() {
    let legacy = "[ui]\naccent = 'red'\n[theme]\nname = 'nord'\n";
    assert_eq!(style_from_config(legacy), "1;38;2;46;52;64;41");
    let custom = format!("{legacy}[theme.custom]\naccent = 'lightgreen'\n");
    assert_eq!(style_from_config(&custom), "1;38;2;46;52;64;102");
    let dark = format!(
        "{}[theme.custom.dark]\naccent = 'white'",
        custom.replace("name = 'nord'", "name = 'nord'\nauto_switch = true")
    );
    assert_eq!(style_from_config(&dark), "1;38;2;46;52;64;107");
    assert_eq!(
        style_from_config("[ui]\naccent = 'cyan'"),
        style_from_config("")
    );
}

#[test]
fn auto_uses_dark_sibling_or_explicit_name() {
    assert_eq!(
        style_from_config("[theme]\nname = 'latte'\nauto_switch = true"),
        style_from_config("")
    );
    assert_eq!(
        style_from_config("[theme]\nname = 'latte'\nauto_switch = true\ndark_name = 'dracula'"),
        style_from_config("[theme]\nname = 'dracula'")
    );
    assert_eq!(
        style_from_config("[theme.custom.dark]\naccent = 'red'"),
        style_from_config("")
    );
    assert_eq!(
        style_from_config("[theme]\nname = 'Tokyo_Night'"),
        "1;38;2;26;27;38;48;2;122;162;247"
    );
}

#[test]
fn color_formats_are_safe_and_native_compatible() {
    for (value, color) in [
        (" #f0a ", Color::Rgb(255, 0, 170)),
        ("#0102ff", Color::Rgb(1, 2, 255)),
        ("RGB(1, 2, 3)", Color::Rgb(1, 2, 3)),
        ("transparent", Color::Reset),
        ("purple", Color::Ansi(5)),
        ("grey", Color::Ansi(7)),
        ("white", Color::Ansi(15)),
        ("#ééé", Color::Ansi(6)),
        ("rgb(256,0,0)", Color::Ansi(6)),
        ("\x1b[31m", Color::Ansi(6)),
    ] {
        assert_eq!(parse_color(value), color, "{value}");
    }
}

#[test]
fn config_path_precedence() {
    let values = [
        ("HERDR_CONFIG_PATH", "/explicit"),
        ("XDG_CONFIG_HOME", "/xdg"),
        ("HOME", "/home"),
    ];
    let lookup = |key: &str| {
        values
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| PathBuf::from(v))
    };
    assert_eq!(
        resolve_config_path(lookup),
        Some(PathBuf::from("/explicit"))
    );
    assert_eq!(
        resolve_config_path(|k| if k == "HERDR_CONFIG_PATH" {
            None
        } else {
            lookup(k)
        }),
        Some(PathBuf::from("/xdg/herdr/config.toml"))
    );
    assert_eq!(
        resolve_config_path(|k| if k == "HOME" { lookup(k) } else { None }),
        Some(PathBuf::from("/home/.config/herdr/config.toml"))
    );
    assert_eq!(resolve_config_path(|_| None), None);
}

#[test]
fn tokyo_day_reset_uses_native_dim_surface() {
    assert_eq!(
        style_from_config("[theme]\nname = 'tokyo-night-day'\n[theme.custom]\npanel_bg = 'reset'"),
        "1;38;2;210;211;218;48;2;46;125;233"
    );
}

#[test]
fn mode_overrides_ignore_unknown_nested_fields() {
    assert_eq!(
        style_from_config(
            "[theme]\nname = 'nord'\nauto_switch = true\n\
             [theme.custom.dark]\naccent = 'red'\n\
             [theme.custom.dark.dark]\naccent = 42"
        ),
        "1;38;2;46;52;64;41"
    );
}

#[test]
fn dark_overrides_follow_legacy_and_global_tokens() {
    assert_eq!(
        style_from_config(
            "[ui]\naccent = 'red'\n\
             [theme]\nname = 'terminal'\nauto_switch = true\n\
             [theme.custom]\nsurface_dim = 'green'\n\
             [theme.custom.dark]\npanel_bg = 'reset'\naccent = 'blue'\n\
             [theme.custom.light]\naccent = 'yellow'"
        ),
        "1;32;44"
    );
    assert_eq!(
        style_from_config("[theme]\nname = 'nord'\ndark_name = 'dracula'"),
        style_from_config("[theme]\nname = 'nord'")
    );
    assert_eq!(
        style_from_config("[theme]\nname = 'nord'\nauto_switch = true\ndark_name = 'unknown'"),
        style_from_config("")
    );
}

#[test]
fn light_aliases_use_dark_siblings_in_auto_mode() {
    for (light, dark) in [
        ("Light", "catppuccin"),
        ("Tokyo_Day", "tokyo-night"),
        ("Gruvbox Light", "gruvbox"),
        ("onelight", "one-dark"),
        ("Solarized_Light", "solarized"),
        ("lotus", "kanagawa"),
        ("dawn", "rose-pine"),
        ("terminal", "terminal"),
        ("unknown", "catppuccin"),
    ] {
        assert_eq!(
            style_from_config(&format!("[theme]\nname = '{light}'\nauto_switch = true")),
            style_from_config(&format!("[theme]\nname = '{dark}'")),
            "{light}"
        );
    }
}

#[test]
fn hex_channels_follow_native_parsing_without_unicode_panics() {
    for (value, expected) in [
        ("#+ab", Color::Ansi(6)),
        ("#ab+def", Color::Rgb(171, 13, 239)),
        ("#+a+b+c", Color::Rgb(10, 11, 12)),
        ("#aébcd", Color::Ansi(6)),
        ("#abécd", Color::Ansi(6)),
        ("#abcdé", Color::Ansi(6)),
        ("#12345", Color::Ansi(6)),
        ("#12gg56", Color::Ansi(6)),
    ] {
        assert_eq!(parse_color(value), expected, "{value}");
    }
}
