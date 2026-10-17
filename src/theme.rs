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

    /// Truecolor presets (ui-spec §2). OWNER: UI worker; default keeps ANSI.
    pub fn apply_preset(&mut self, preset: &str) {
        let _ = preset;
    }

    /// Per-token overrides from `[theme.colors]`. OWNER: UI worker.
    pub fn apply_overrides(&mut self, colors: &BTreeMap<String, String>) {
        let _ = colors;
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

#[cfg(test)]
mod tests {
    use super::*;
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
