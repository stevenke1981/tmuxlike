//! Semantic UI colors and glyphs (docs/team/ui-ux/ui-spec.md §2).
//!
//! Defaults are ANSI-16 indices so the chrome follows the user's terminal theme. State is
//! always shape + word; color only reinforces it. `mono` drops chrome colors (NO_COLOR,
//! TERM=dumb, preset "mono"); `ascii` swaps every glyph for an ASCII one.
use crate::api::Status;
use crossterm::style::Color;
use serde::Deserialize;
use std::collections::BTreeMap;

/// `[theme]` table in config.toml.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct ThemeCfg {
    /// terminal (ANSI-16, default) | dark | light (truecolor) | mono (no color)
    pub preset: String,
    /// auto | true | false
    pub ascii_glyphs: String,
    /// per-token override: 0-255 | "#rrggbb" | "reset"
    pub colors: BTreeMap<String, String>,
}

impl Default for ThemeCfg {
    fn default() -> Self {
        ThemeCfg {
            preset: "terminal".into(),
            ascii_glyphs: "auto".into(),
            colors: BTreeMap::new(),
        }
    }
}

/// What the attaching client's environment asks for (sent with `Command::Attach`).
#[derive(serde::Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientEnv {
    /// NO_COLOR is set and non-empty
    pub no_color: bool,
    /// TERM=dumb
    pub dumb: bool,
    /// TMUXLIKE_ASCII=1 or a non-UTF-8 locale
    pub ascii: bool,
}

impl ClientEnv {
    /// Read the current process environment (client side).
    pub fn detect() -> ClientEnv {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let locale = var("LC_ALL")
            .or_else(|| var("LC_CTYPE"))
            .or_else(|| var("LANG"));
        let non_utf8 = locale.is_some_and(|l| {
            let l = l.to_ascii_lowercase();
            !(l.contains("utf-8") || l.contains("utf8")) && l != "c.utf8"
        });
        ClientEnv {
            no_color: var("NO_COLOR").is_some(),
            dumb: var("TERM").is_some_and(|t| t == "dumb"),
            ascii: var("TMUXLIKE_ASCII").is_some_and(|v| v == "1") || (cfg!(unix) && non_utf8),
        }
    }
}

/// Glyph table; `ascii` swaps the whole table.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyphs {
    pub working: &'static str,
    pub blocked: &'static str,
    pub done: &'static str,
    pub idle: &'static str,
    pub unknown: &'static str,
    pub vline: &'static str,
    pub hline: &'static str,
    pub vline_active: &'static str,
    pub hline_active: &'static str,
    /// ├ ┤ ┬ ┴ ┼
    pub tee_right: &'static str,
    pub tee_left: &'static str,
    pub tee_down: &'static str,
    pub tee_up: &'static str,
    pub cross: &'static str,
    /// ┌ ┐ └ ┘ (two perpendicular arms)
    pub corner_tl: &'static str,
    pub corner_tr: &'static str,
    pub corner_bl: &'static str,
    pub corner_br: &'static str,
    pub group_open: &'static str,
    pub group_closed: &'static str,
    pub pointer: &'static str,
    pub ellipsis: &'static str,
    pub tree_mid: &'static str,
    pub tree_last: &'static str,
    pub ok: &'static str,
    pub err: &'static str,
}

pub const UNICODE: Glyphs = Glyphs {
    working: "●",
    blocked: "▲",
    done: "◆",
    idle: "○",
    unknown: "?",
    vline: "│",
    hline: "─",
    vline_active: "┃",
    hline_active: "━",
    tee_right: "├",
    tee_left: "┤",
    tee_down: "┬",
    tee_up: "┴",
    cross: "┼",
    corner_tl: "┌",
    corner_tr: "┐",
    corner_bl: "└",
    corner_br: "┘",
    group_open: "▾",
    group_closed: "▸",
    pointer: "›",
    ellipsis: "…",
    tree_mid: "├",
    tree_last: "└",
    ok: "✓",
    err: "✗",
};

pub const ASCII: Glyphs = Glyphs {
    working: "@",
    blocked: "!",
    done: "+",
    idle: "-",
    unknown: "?",
    vline: "|",
    hline: "-",
    vline_active: "#",
    hline_active: "=",
    tee_right: "+",
    tee_left: "+",
    tee_down: "+",
    tee_up: "+",
    cross: "+",
    corner_tl: "+",
    corner_tr: "+",
    corner_bl: "+",
    corner_br: "+",
    group_open: "v",
    group_closed: ">",
    pointer: ">",
    ellipsis: "~",
    tree_mid: "|",
    tree_last: "\\",
    ok: "+",
    err: "x",
};

/// Resolved theme for one client frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// no chrome colors: use BOLD / INVERSE / DIM instead
    pub mono: bool,
    pub ascii: bool,
    pub accent: Color,
    pub muted: Color,
    pub border: Color,
    pub border_active: Color,
    pub working: Color,
    pub blocked: Color,
    pub done: Color,
    pub idle: Color,
    pub unknown: Color,
    pub bar_bg: Color,
    pub bar_fg: Color,
    pub selection: Color,
    pub warn: Color,
    pub g: Glyphs,
}

impl Theme {
    /// Default "terminal" preset (ANSI-16).
    pub fn terminal() -> Theme {
        let a = Color::AnsiValue;
        Theme {
            mono: false,
            ascii: false,
            accent: a(6),
            muted: a(8),
            border: a(8),
            border_active: a(6),
            working: a(3),
            blocked: a(1),
            done: a(2),
            idle: a(8),
            unknown: a(8),
            bar_bg: a(0),
            bar_fg: a(7),
            selection: a(6),
            warn: a(3),
            g: UNICODE,
        }
    }

    /// Resolve config + client environment. Precedence: explicit preset > NO_COLOR/dumb -> mono.
    /// Presets "dark"/"light" and `colors` overrides are applied by `apply_preset`/`apply_overrides`.
    pub fn resolve(cfg: &ThemeCfg, env: &ClientEnv) -> Theme {
        let mut t = Theme::terminal();
        let explicit = cfg.preset != "terminal";
        t.mono = cfg.preset == "mono" || (!explicit && (env.no_color || env.dumb));
        t.apply_preset(&cfg.preset);
        t.apply_overrides(&cfg.colors);
        t.ascii = match cfg.ascii_glyphs.as_str() {
            "true" | "on" | "yes" => true,
            "false" | "off" | "no" => false,
            _ => env.ascii || env.dumb,
        };
        if t.ascii {
            t.g = ASCII;
        }
        t
    }

    /// Truecolor presets (ui-spec §2). "terminal" and "mono" keep the ANSI-16 defaults
    /// (mono is a flag set by `resolve`); unknown names are ignored.
    pub fn apply_preset(&mut self, preset: &str) {
        let Some(p) = PRESETS.iter().find(|(name, _)| *name == preset) else {
            return;
        };
        for (token, value) in p.1 {
            if let (Some(slot), Some(c)) = (self.slot(token), parse_color(value)) {
                *slot = c;
            }
        }
    }

    /// Per-token overrides from `[theme.colors]`: token = Theme field name, value =
    /// 0-255 | "#rrggbb" | "reset". Unknown tokens and unparsable values are ignored.
    pub fn apply_overrides(&mut self, colors: &BTreeMap<String, String>) {
        for (token, value) in colors {
            if let (Some(slot), Some(c)) = (self.slot(token), parse_color(value)) {
                *slot = c;
            }
        }
    }

    /// The color field named by a token (ui-spec §2 names).
    fn slot(&mut self, token: &str) -> Option<&mut Color> {
        Some(match token {
            "accent" => &mut self.accent,
            "muted" => &mut self.muted,
            "border" => &mut self.border,
            "border_active" => &mut self.border_active,
            "working" => &mut self.working,
            "blocked" => &mut self.blocked,
            "done" => &mut self.done,
            "idle" => &mut self.idle,
            "unknown" => &mut self.unknown,
            "bar_bg" => &mut self.bar_bg,
            "bar_fg" => &mut self.bar_fg,
            "selection" => &mut self.selection,
            "warn" => &mut self.warn,
            _ => return None,
        })
    }

    /// (glyph, color, word) for a status. Chrome callers must still honor `mono`.
    pub fn status(&self, s: Status) -> (&'static str, Color, &'static str) {
        match s {
            Status::Working => (self.g.working, self.working, "working"),
            Status::Blocked => (self.g.blocked, self.blocked, "blocked"),
            Status::Done => (self.g.done, self.done, "done"),
            Status::Idle => (self.g.idle, self.idle, "idle"),
            Status::Unknown => (self.g.unknown, self.unknown, "unknown"),
        }
    }

    /// `c`, or `Color::Reset` in mono mode. Use for every chrome color.
    pub fn c(&self, c: Color) -> Color {
        if self.mono { Color::Reset } else { c }
    }
}

/// Token values of the truecolor presets (ui-spec §2 "truecolor dark / light").
/// `border_active` and `selection` equal `accent`; `idle`/`unknown` equal `muted`;
/// `warn` equals `working`.
const PRESETS: &[(&str, &[(&str, &str)])] = &[
    (
        "dark",
        &[
            ("accent", "#56b6c2"),
            ("muted", "#7f8796"),
            ("border", "#6b7280"),
            ("border_active", "#56b6c2"),
            ("working", "#e5c07b"),
            ("blocked", "#e06c75"),
            ("done", "#98c379"),
            ("idle", "#7f8796"),
            ("unknown", "#7f8796"),
            ("bar_bg", "#1f2430"),
            ("bar_fg", "#c8ccd4"),
            ("selection", "#56b6c2"),
            ("warn", "#e5c07b"),
        ],
    ),
    (
        "light",
        &[
            ("accent", "#0e7490"),
            ("muted", "#6b7280"),
            ("border", "#6b7280"),
            ("border_active", "#0e7490"),
            ("working", "#a16207"),
            ("blocked", "#b91c1c"),
            ("done", "#15803d"),
            ("idle", "#6b7280"),
            ("unknown", "#6b7280"),
            ("bar_bg", "#e5e7eb"),
            ("bar_fg", "#1f2937"),
            ("selection", "#0e7490"),
            ("warn", "#a16207"),
        ],
    ),
];

/// Parse a theme color value: "reset", a 0-255 palette index, or "#rrggbb".
pub fn parse_color(value: &str) -> Option<Color> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("reset") {
        return Some(Color::Reset);
    }
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return Some(Color::Rgb {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
        });
    }
    v.parse::<u8>().ok().map(Color::AnsiValue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parse_color_accepts_index_hex_and_reset_only() {
        assert_eq!(parse_color("3"), Some(Color::AnsiValue(3)));
        assert_eq!(parse_color("255"), Some(Color::AnsiValue(255)));
        assert_eq!(parse_color("256"), None);
        assert_eq!(
            parse_color("#e5c07b"),
            Some(Color::Rgb {
                r: 0xe5,
                g: 0xc0,
                b: 0x7b
            })
        );
        assert_eq!(parse_color("#E5C07"), None);
        assert_eq!(parse_color("#zz0000"), None);
        assert_eq!(parse_color("reset"), Some(Color::Reset));
        assert_eq!(parse_color("Reset"), Some(Color::Reset));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn dark_and_light_presets_set_truecolor_tokens() {
        let mut t = Theme::terminal();
        t.apply_preset("dark");
        assert_eq!(
            t.accent,
            Color::Rgb {
                r: 0x56,
                g: 0xb6,
                b: 0xc2
            }
        );
        assert_eq!(t.border_active, t.accent);
        assert_eq!(t.idle, t.muted);
        let mut l = Theme::terminal();
        l.apply_preset("light");
        assert_eq!(
            l.bar_bg,
            Color::Rgb {
                r: 0xe5,
                g: 0xe7,
                b: 0xeb
            }
        );
        assert_ne!(l.accent, t.accent);
    }

    #[test]
    fn terminal_and_unknown_presets_keep_ansi16() {
        let base = Theme::terminal();
        let mut t = Theme::terminal();
        t.apply_preset("terminal");
        t.apply_preset("nope");
        assert_eq!(t, base);
        assert_eq!(base.accent, Color::AnsiValue(6));
        assert_eq!(base.border, Color::AnsiValue(8));
    }

    #[test]
    fn overrides_apply_by_field_name_and_ignore_bad_input() {
        let mut t = Theme::terminal();
        t.apply_overrides(&map(&[
            ("border_active", "#e5c07b"),
            ("accent", "74"),
            ("muted", "reset"),
            ("no_such_token", "1"),
            ("warn", "999"),
            ("blocked", "#12"),
        ]));
        assert_eq!(
            t.border_active,
            Color::Rgb {
                r: 0xe5,
                g: 0xc0,
                b: 0x7b
            }
        );
        assert_eq!(t.accent, Color::AnsiValue(74));
        assert_eq!(t.muted, Color::Reset);
        assert_eq!(t.warn, Color::AnsiValue(3), "out-of-range value ignored");
        assert_eq!(t.blocked, Color::AnsiValue(1), "bad hex ignored");
    }

    #[test]
    fn resolve_applies_preset_then_overrides() {
        let cfg = ThemeCfg {
            preset: "dark".into(),
            ascii_glyphs: "auto".into(),
            colors: map(&[("accent", "#000001")]),
        };
        let t = Theme::resolve(&cfg, &ClientEnv::default());
        assert_eq!(t.accent, Color::Rgb { r: 0, g: 0, b: 1 });
        assert_eq!(
            t.blocked,
            Color::Rgb {
                r: 0xe0,
                g: 0x6c,
                b: 0x75
            }
        );
    }

    #[test]
    fn resolve_mono_and_ascii() {
        let cfg = ThemeCfg::default();
        let plain = Theme::resolve(&cfg, &ClientEnv::default());
        assert!(!plain.mono && !plain.ascii);
        assert_eq!(plain.status(Status::Blocked).0, "▲");
        let nc = Theme::resolve(
            &cfg,
            &ClientEnv {
                no_color: true,
                ..Default::default()
            },
        );
        assert!(nc.mono && !nc.ascii, "NO_COLOR keeps unicode shapes");
        assert_eq!(nc.c(nc.accent), Color::Reset);
        let dumb = Theme::resolve(
            &cfg,
            &ClientEnv {
                dumb: true,
                ..Default::default()
            },
        );
        assert!(dumb.mono && dumb.ascii);
        assert_eq!(dumb.status(Status::Working).0, "@");
        let forced = ThemeCfg {
            preset: "dark".into(),
            ..ThemeCfg::default()
        };
        assert!(
            !Theme::resolve(
                &forced,
                &ClientEnv {
                    no_color: true,
                    ..Default::default()
                }
            )
            .mono
        );
    }
}
