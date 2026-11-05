//! Herdr v0.9.5's copy-mode color rules using the locally configured palette.
//!
//! Reads `HERDR_CONFIG_PATH`, otherwise `XDG_CONFIG_HOME/herdr/config.toml`,
//! then `~/.config/herdr/config.toml`. Missing/unreadable/invalid files
//! use defaults. Only theme fields and the legacy `ui.accent` are interpreted.
//!
//! The native API does not expose a palette, terminal background, or client appearance.
//! Auto-switch therefore uses the host's initial dark fallback, including dark
//! overrides; live light/dark detection and interactive theme previews cannot
//! be matched. Files are reread on each call, but unsaved client state, remote
//! client config, and debug-host `herdr-dev` config require an explicit path.
//! Palette values and precedence are pinned to v0.9.5, not a stable native API.

use serde::Deserialize;
use std::{env, fs, path::PathBuf};

pub(crate) struct Styles {
    pub(crate) cursor: String,
    pub(crate) selection: String,
}

pub(crate) fn styles() -> Styles {
    let contents = config_path().and_then(|path| fs::read_to_string(path).ok());
    styles_from_config(contents.as_deref().unwrap_or_default())
}

fn config_path() -> Option<PathBuf> {
    resolve_config_path(|key| env::var_os(key).map(PathBuf::from))
}

fn resolve_config_path(get: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = get("HERDR_CONFIG_PATH") {
        return Some(path);
    }
    let base = get("XDG_CONFIG_HOME").or_else(|| get("HOME").map(|path| path.join(".config")));
    base.map(|p| p.join("herdr/config.toml"))
}

fn styles_from_config(contents: &str) -> Styles {
    let config: Config = toml::from_str(contents).unwrap_or_default();
    let theme = &config.theme;
    let manual = theme.name.as_deref().unwrap_or("catppuccin");
    let name = if theme.auto_switch {
        theme
            .dark_name
            .as_deref()
            .unwrap_or_else(|| dark_sibling(manual))
    } else {
        manual
    };
    let mut palette = palette(name);
    palette.apply(&theme.custom.colors);
    // Native client honors the legacy accent unless a global custom accent exists.
    if theme.custom.colors.accent.is_none()
        && let Some(accent) = config.ui.accent.as_deref().filter(|s| *s != "cyan")
    {
        palette.accent = parse_color(accent);
    }
    if theme.auto_switch
        && let Some(dark) = &theme.custom.dark
    {
        palette.apply(dark);
    }
    let foreground = if palette.panel_bg == Color::Reset {
        palette.surface_dim
    } else {
        palette.panel_bg
    };
    Styles {
        cursor: format!("1;{};{}", sgr(foreground, false), sgr(palette.accent, true)),
        selection: selection_style(foreground),
    }
}

fn selection_style(background: Color) -> String {
    // The host background isn't exported; use native's configured-palette fallback.
    let background = match background {
        Color::Rgb(r, g, b) => {
            let target = if relative_luminance((r, g, b)) < 0.5 {
                255.0
            } else {
                0.0
            };
            let mix = |value| (f32::from(value) + (target - f32::from(value)) * 0.28).round() as u8;
            Color::Rgb(mix(r), mix(g), mix(b))
        }
        color => color,
    };
    let foreground = match background {
        Color::Rgb(r, g, b) => {
            let luminance = relative_luminance((r, g, b));
            if (luminance + 0.05) / 0.05 > 1.05 / (luminance + 0.05) {
                Color::Rgb(0, 0, 0)
            } else {
                Color::Rgb(255, 255, 255)
            }
        }
        Color::Ansi(index) => {
            let colors = [
                (0, 0, 0),
                (128, 0, 0),
                (0, 128, 0),
                (128, 128, 0),
                (0, 0, 128),
                (128, 0, 128),
                (0, 128, 128),
                (192, 192, 192),
                (128, 128, 128),
                (255, 0, 0),
                (0, 255, 0),
                (255, 255, 0),
                (0, 0, 255),
                (255, 0, 255),
                (0, 255, 255),
                (255, 255, 255),
            ];
            Color::Ansi(if relative_luminance(colors[usize::from(index)]) < 0.5 {
                15
            } else {
                0
            })
        }
        Color::Reset => Color::Reset,
    };
    format!("0;{};{}", sgr(foreground, false), sgr(background, true))
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn dark_sibling(name: &str) -> &str {
    match canonical_name(name).as_str() {
        "catppuccin" | "catppuccin-latte" => "catppuccin",
        "tokyo-night" | "tokyo-night-day" => "tokyo-night",
        "gruvbox" | "gruvbox-light" => "gruvbox",
        "one-dark" | "one-light" => "one-dark",
        "solarized" | "solarized-light" => "solarized",
        "kanagawa" | "kanagawa-lotus" => "kanagawa",
        "rose-pine" | "rose-pine-dawn" => "rose-pine",
        _ => name,
    }
}

fn canonical_name(name: &str) -> String {
    let normalized = name.to_lowercase().replace([' ', '_'], "-");
    match normalized.as_str() {
        "catppuccin-mocha" => "catppuccin",
        "latte" | "light" => "catppuccin-latte",
        "tokyonight" => "tokyo-night",
        "tokyo-day" | "tokyonight-day" => "tokyo-night-day",
        "gruvbox-dark" => "gruvbox",
        "onedark" => "one-dark",
        "onelight" => "one-light",
        "solarized-dark" => "solarized",
        "lotus" => "kanagawa-lotus",
        "rosepine" => "rose-pine",
        "rosepine-dawn" | "dawn" => "rose-pine-dawn",
        _ => return normalized,
    }
    .to_owned()
}

fn palette(name: &str) -> Palette {
    use Color::{Ansi, Reset, Rgb};
    // Only the three tokens used by the native cursor, from app/state.rs v0.9.5.
    let name = canonical_name(name);
    let (panel_bg, accent) = match name.as_str() {
        "catppuccin-latte" => (Rgb(239, 241, 245), Rgb(30, 102, 245)),
        "terminal" => {
            return Palette {
                panel_bg: Reset,
                surface_dim: Ansi(8),
                accent: Ansi(4),
            };
        }
        "tokyo-night" => (Rgb(26, 27, 38), Rgb(122, 162, 247)),
        "tokyo-night-day" => (Rgb(225, 226, 231), Rgb(46, 125, 233)),
        "dracula" => (Rgb(40, 42, 54), Rgb(189, 147, 249)),
        "nord" => (Rgb(46, 52, 64), Rgb(136, 192, 208)),
        "gruvbox" => (Rgb(40, 40, 40), Rgb(215, 153, 33)),
        "gruvbox-light" => (Rgb(251, 241, 199), Rgb(7, 102, 120)),
        "one-dark" => (Rgb(40, 44, 52), Rgb(97, 175, 239)),
        "one-light" => (Rgb(250, 250, 250), Rgb(64, 120, 242)),
        "solarized" => (Rgb(0, 43, 54), Rgb(38, 139, 210)),
        "solarized-light" => (Rgb(253, 246, 227), Rgb(38, 139, 210)),
        "kanagawa" => (Rgb(31, 31, 40), Rgb(126, 156, 216)),
        "kanagawa-lotus" => (Rgb(242, 236, 188), Rgb(77, 105, 155)),
        "rose-pine" => (Rgb(25, 23, 36), Rgb(196, 167, 231)),
        "rose-pine-dawn" => (Rgb(250, 244, 237), Rgb(144, 122, 169)),
        "vesper" => (Rgb(26, 26, 26), Rgb(255, 199, 153)),
        _ => (Rgb(24, 24, 37), Rgb(137, 180, 250)),
    };
    let surface_dim = match name.as_str() {
        "catppuccin-latte" => Rgb(230, 233, 239),
        "tokyo-night-day" => Rgb(210, 211, 218),
        "gruvbox-light" => Rgb(242, 229, 188),
        "one-light" => Rgb(245, 245, 246),
        "solarized-light" => Rgb(238, 232, 213),
        "kanagawa-lotus" => Rgb(213, 206, 163),
        "rose-pine" => Rgb(38, 35, 58),
        "rose-pine-dawn" => Rgb(242, 233, 225),
        "vesper" => Rgb(16, 16, 16),
        "catppuccin" => Rgb(30, 30, 46),
        _ if panel_bg == Rgb(24, 24, 37) => Rgb(30, 30, 46),
        _ => panel_bg,
    };
    Palette {
        panel_bg,
        surface_dim,
        accent,
    }
}

impl Palette {
    fn apply(&mut self, custom: &Overrides) {
        for (target, value) in [
            (&mut self.panel_bg, &custom.panel_bg),
            (&mut self.surface_dim, &custom.surface_dim),
            (&mut self.accent, &custom.accent),
        ] {
            if let Some(value) = value {
                *target = parse_color(value);
            }
        }
    }
}

fn parse_color(value: &str) -> Color {
    use Color::{Ansi, Reset, Rgb};
    let value = value.trim().to_lowercase();
    if let Some(hex) = value.strip_prefix('#').filter(|s| s.is_ascii()) {
        if hex.len() == 6 {
            // Match the native parser's per-channel syntax (including leading +).
            // The ASCII check above keeps byte slicing safe for invalid Unicode.
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..], 16),
            ) {
                return Rgb(r, g, b);
            }
        } else if hex.len() == 3
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            && let Ok(rgb) = u16::from_str_radix(hex, 16)
        {
            return Rgb(
                ((rgb >> 8) as u8) * 17,
                (((rgb >> 4) & 15) as u8) * 17,
                ((rgb & 15) as u8) * 17,
            );
        }
    }
    if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<_> = inner.split(',').map(|s| s.trim().parse::<u8>()).collect();
        if let [Ok(r), Ok(g), Ok(b)] = parts.as_slice() {
            return Rgb(*r, *g, *b);
        }
    }
    match value.as_str() {
        "reset" | "default" | "none" | "transparent" => Reset,
        "black" => Ansi(0),
        "red" => Ansi(1),
        "green" => Ansi(2),
        "yellow" => Ansi(3),
        "blue" => Ansi(4),
        "magenta" | "purple" => Ansi(5),
        "cyan" => Ansi(6),
        "gray" | "grey" => Ansi(7),
        "darkgray" | "darkgrey" => Ansi(8),
        "lightred" => Ansi(9),
        "lightgreen" => Ansi(10),
        "lightyellow" => Ansi(11),
        "lightblue" => Ansi(12),
        "lightmagenta" => Ansi(13),
        "lightcyan" => Ansi(14),
        "white" => Ansi(15),
        _ => Ansi(6), // Native parser's invalid-color fallback.
    }
}

fn sgr(color: Color, background: bool) -> String {
    let offset = if background { 10 } else { 0 };
    match color {
        Color::Reset => (39 + offset).to_string(),
        Color::Ansi(index) => (if index < 8 {
            30 + index + offset
        } else {
            90 + index - 8 + offset
        })
        .to_string(),
        Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", 38 + offset),
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Config {
    theme: Theme,
    ui: Ui,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Theme {
    name: Option<String>,
    auto_switch: bool,
    dark_name: Option<String>,
    custom: Custom,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Custom {
    #[serde(flatten)]
    colors: Overrides,
    dark: Option<Overrides>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Overrides {
    panel_bg: Option<String>,
    surface_dim: Option<String>,
    accent: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Ui {
    accent: Option<String>,
}

struct Palette {
    panel_bg: Color,
    surface_dim: Color,
    accent: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Color {
    Reset,
    Ansi(u8),
    Rgb(u8, u8, u8),
}

#[cfg(test)]
mod tests;
