//! Compose a session's visible state into a cell grid and diff it into ANSI output.
//!
//! `render_client` computes the shared [`Ctx`] and the grid, then calls the section
//! drawers in paint order: panes, borders, sidebar, status bar, overlays.
mod overlay;
mod panes;
mod sidebar;
pub use sidebar::{SidebarKind, sidebar_rows, sidebar_text_width};
mod status;

pub use overlay::{
    Entry, Filter, chooser_entries, kill_target, pack_chooser, unpack_chooser, visible_entries,
};
pub use status::{status_hit_at, status_regions};

use crate::layout::Rect;
use crate::server::{Client, Session, State};
use crossterm::style::{Attribute, Color, Print, SetAttribute};
use crossterm::{cursor, terminal};

/// Always emit ANSI text. crossterm's `queue!` falls back to calling the WinAPI on Windows
/// when the process has no VT-capable console — and the server is a detached process with
/// no console at all — which silently produced frames without any escape sequences.
macro_rules! ansi {
    ($out:expr, $($c:expr),+ $(,)?) => {{
        $( let _ = crossterm::Command::write_ansi(&$c, &mut $out); )+
    }};
}
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, PartialEq, Debug)]
pub struct Cell {
    pub s: String,
    pub fg: Color,
    pub bg: Color,
    pub attrs: u8,
    pub wide: bool,
    pub skip: bool,
}

const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const UNDER: u8 = 4;
const INVERSE: u8 = 8;
/// SGR 2: faint text (mono-mode replacement for muted color).
const DIM: u8 = 16;

impl Default for Cell {
    fn default() -> Self {
        Cell {
            s: " ".into(),
            fg: Color::Reset,
            bg: Color::Reset,
            attrs: 0,
            wide: false,
            skip: false,
        }
    }
}

#[derive(Default)]
pub struct Frame {
    cells: Vec<Cell>,
    w: u16,
    h: u16,
}

impl Frame {
    pub fn invalidate(&mut self) {
        self.cells.clear();
    }
}

struct Grid {
    w: u16,
    h: u16,
    cells: Vec<Cell>,
}

impl Grid {
    fn put(&mut self, x: u16, y: u16, c: Cell) {
        if x < self.w && y < self.h {
            self.cells[(y as usize) * self.w as usize + x as usize] = c;
        }
    }
    /// Write text truncated to `max` display columns (CJK aware). Returns columns used.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: u16, y: u16, max: u16, s: &str, fg: Color, bg: Color, attrs: u8) -> u16 {
        let mut cx = x;
        let limit = (x as u32 + max as u32).min(self.w as u32) as u16;
        for ch in s.chars() {
            let cw = ch.width().unwrap_or(0) as u16;
            if cw == 0 {
                continue;
            }
            if cx + cw > limit {
                break;
            }
            self.put(
                cx,
                y,
                Cell {
                    s: ch.to_string(),
                    fg,
                    bg,
                    attrs,
                    wide: cw == 2,
                    skip: false,
                },
            );
            if cw == 2 {
                self.put(
                    cx + 1,
                    y,
                    Cell {
                        s: String::new(),
                        fg,
                        bg,
                        attrs,
                        wide: false,
                        skip: true,
                    },
                );
            }
            cx += cw;
        }
        cx - x
    }
}

/// Per-frame context shared by every section drawer. Built once in `render_client`.
struct Ctx<'a> {
    st: &'a State,
    s: &'a Session,
    c: &'a Client,
    w: u16,
    h: u16,
    rects: Vec<(usize, Rect)>,
    area: Rect,
    /// resolved colors/glyphs for this client (config + NO_COLOR/TERM/ASCII)
    theme: crate::theme::Theme,
}

/// Truncate to a display width, appending '…' if cut.
/// Cut `s` to at most `max` display columns, ending in `ell` when cut (use
/// `theme.g.ellipsis`, so ASCII mode gets `~`). Never splits a wide character.
pub fn cut(s: &str, max: usize, ell: &str) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let ew = ell.width();
    if max < ew {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw + ew > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push_str(ell);
    out
}

/// Deprecated: Unicode-only ellipsis; use [`cut`] with the theme ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

pub fn render_client(st: &State, c: &mut Client) -> Option<String> {
    let s = st.sessions.iter().find(|s| s.name == c.session)?;
    let (w, h) = (s.cols, s.rows);
    let mut g = Grid {
        w,
        h,
        cells: vec![Cell::default(); w as usize * h as usize],
    };
    let mut covered = vec![false; w as usize * h as usize];
    let win = &s.windows[s.active];
    let rects = st.visible_rects(s, win);
    let area = st.pane_area(s);
    let cx = Ctx {
        st,
        s,
        c: &*c,
        w,
        h,
        rects,
        area,
        theme: crate::theme::Theme::resolve(&st.cfg.theme, &c.env),
    };
    let mut cursor_pos = panes::draw_panes(&mut g, &cx, &mut covered);
    panes::draw_borders(&mut g, &cx, &covered);
    sidebar::draw_sidebar(&mut g, &cx);
    cursor_pos = status::draw_status(&mut g, &cx).or(cursor_pos);
    if overlay::draw_overlays(&mut g, &cx) {
        cursor_pos = None;
    }
    Some(emit(&mut c.frame, g, cursor_pos))
}

/// SGR for a colour, written by hand so NO_COLOR or platform quirks never drop it.
fn sgr_color(c: Color, fg: bool) -> String {
    let base = |n: u8| format!("\x1b[{}m", if fg { n } else { n + 10 });
    match c {
        Color::Reset => base(39),
        Color::Black => base(30),
        Color::DarkRed => base(31),
        Color::DarkGreen => base(32),
        Color::DarkYellow => base(33),
        Color::DarkBlue => base(34),
        Color::DarkMagenta => base(35),
        Color::DarkCyan => base(36),
        Color::Grey => base(37),
        Color::DarkGrey => base(90),
        Color::Red => base(91),
        Color::Green => base(92),
        Color::Yellow => base(93),
        Color::Blue => base(94),
        Color::Magenta => base(95),
        Color::Cyan => base(96),
        Color::White => base(97),
        // ui-spec §2: indices 0-7 and 8-15 use the classic 3x/4x and 9x/10x codes.
        Color::AnsiValue(i) if i < 8 => base(30 + i),
        Color::AnsiValue(i) if i < 16 => base(90 + i - 8),
        Color::AnsiValue(i) => format!("\x1b[{};5;{}m", if fg { 38 } else { 48 }, i),
        Color::Rgb { r, g, b } => format!("\x1b[{};2;{};{};{}m", if fg { 38 } else { 48 }, r, g, b),
    }
}

/// Write the attribute SGRs for `attrs` (after a reset).
fn push_attrs(out: &mut String, attrs: u8) {
    let flags = [
        (BOLD, Attribute::Bold),
        (DIM, Attribute::Dim),
        (ITALIC, Attribute::Italic),
        (UNDER, Attribute::Underlined),
        (INVERSE, Attribute::Reverse),
    ];
    for (bit, attr) in flags {
        if attrs & bit != 0 {
            let _ = crossterm::Command::write_ansi(&SetAttribute(attr), out);
        }
    }
}

fn emit(frame: &mut Frame, g: Grid, cursor_pos: Option<(u16, u16)>) -> String {
    let mut out = String::new();
    let full = frame.w != g.w || frame.h != g.h || frame.cells.len() != g.cells.len();
    ansi!(out, terminal::BeginSynchronizedUpdate, cursor::Hide);
    if full {
        ansi!(
            out,
            SetAttribute(Attribute::Reset),
            terminal::Clear(terminal::ClearType::All)
        );
    }
    let mut cur_fg = None;
    let mut cur_bg = None;
    let mut cur_attrs: Option<u8> = None;
    let mut pos: Option<(u16, u16)> = None;
    let mut changed = false;
    for y in 0..g.h {
        for x in 0..g.w {
            let i = y as usize * g.w as usize + x as usize;
            let c = &g.cells[i];
            if !full && frame.cells[i] == *c {
                continue;
            }
            if c.skip {
                continue;
            }
            changed = true;
            let moved = pos != Some((x, y));
            if moved {
                ansi!(out, cursor::MoveTo(x, y));
            }
            let needs_reset = cur_attrs != Some(c.attrs)
                || (c.fg == Color::Reset && cur_fg != Some(Color::Reset))
                || (c.bg == Color::Reset && cur_bg != Some(Color::Reset));
            if needs_reset {
                // A full SGR reset also returns both colors to default, so a Reset color
                // never needs an SGR of its own (chrome in mono emits no 3x/4x codes).
                ansi!(out, SetAttribute(Attribute::Reset));
                push_attrs(&mut out, c.attrs);
                cur_attrs = Some(c.attrs);
                cur_fg = Some(Color::Reset);
                cur_bg = Some(Color::Reset);
            }
            // Colors are restated after every cursor move, so each run of a colored chrome
            // cell (for example the focused border) begins with its SGR.
            if c.fg != Color::Reset && (moved || cur_fg != Some(c.fg)) {
                out.push_str(&sgr_color(c.fg, true));
                cur_fg = Some(c.fg);
            }
            if c.bg != Color::Reset && (moved || cur_bg != Some(c.bg)) {
                out.push_str(&sgr_color(c.bg, false));
                cur_bg = Some(c.bg);
            }
            ansi!(out, Print(&c.s));
            let adv = if c.wide { 2 } else { 1 };
            pos = if x + adv < g.w {
                Some((x + adv, y))
            } else {
                None
            };
        }
    }
    ansi!(out, SetAttribute(Attribute::Reset));
    if let Some((x, y)) = cursor_pos {
        ansi!(out, cursor::MoveTo(x, y), cursor::Show);
    }
    ansi!(out, terminal::EndSynchronizedUpdate);
    let _ = changed;
    frame.w = g.w;
    frame.h = g.h;
    frame.cells = g.cells;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emit_is_ansi_on_every_platform() {
        let mut g = Grid {
            w: 6,
            h: 2,
            cells: vec![Cell::default(); 12],
        };
        g.text(0, 1, 6, "ab", Color::Black, Color::Green, BOLD);
        g.text(3, 0, 3, "中", Color::Reset, Color::Reset, 0);
        let mut f = Frame::default();
        let out = emit(&mut f, g, Some((1, 0)));
        assert!(out.contains("\x1b[2;1H"), "{out:?}"); // MoveTo(0,1)
        assert!(out.contains("\x1b[102m"), "{out:?}");
        assert!(out.contains("中"));
        assert!(out.ends_with("\x1b[?2026l"), "{out:?}");
        assert_eq!(truncate("中文名稱很長", 7), "中文名…");
    }

    /// Any SGR that sets a classic or bright 8/16-color foreground or background.
    fn has_classic_color_sgr(out: &str) -> bool {
        let bytes = out.as_bytes();
        let mut i = 0;
        while i + 2 < bytes.len() {
            if bytes[i] == 0x1b && bytes[i + 1] == b'[' {
                let body: String = out[i + 2..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == ';')
                    .collect();
                let end = i + 2 + body.len();
                if out[end..].starts_with('m') && body.split(';').any(|p| {
                    matches!(p.parse::<u32>(), Ok(n) if (30..=37).contains(&n) || (40..=47).contains(&n)
                        || (90..=97).contains(&n) || (100..=107).contains(&n))
                }) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn chrome_grid(fg: Color, bg: Color, attrs: u8) -> Grid {
        let mut g = Grid {
            w: 8,
            h: 2,
            cells: vec![Cell::default(); 16],
        };
        g.text(0, 0, 8, "chrome", fg, bg, attrs);
        g
    }

    #[test]
    fn mono_chrome_emits_no_classic_color_sgr() {
        // Reset colors (what Theme::c returns in mono) must not produce 3x/4x codes.
        let mut f = Frame::default();
        let out = emit(
            &mut f,
            chrome_grid(Color::Reset, Color::Reset, BOLD | INVERSE),
            None,
        );
        assert!(!has_classic_color_sgr(&out), "{out:?}");
        assert!(
            out.contains("\x1b[1m") && out.contains("\x1b[7m"),
            "{out:?}"
        );
    }

    #[test]
    fn mono_switch_from_pane_color_to_chrome_resets_without_color_sgr() {
        // A colored pane cell followed by a chrome cell: the chrome side needs a full reset,
        // not a 39m/49m, and the pane color itself stays allowed.
        let mut g = Grid {
            w: 2,
            h: 1,
            cells: vec![Cell::default(); 2],
        };
        g.put(
            0,
            0,
            Cell {
                s: "a".into(),
                fg: Color::Red,
                ..Cell::default()
            },
        );
        g.put(
            1,
            0,
            Cell {
                s: "b".into(),
                fg: Color::Reset,
                bg: Color::Reset,
                ..Cell::default()
            },
        );
        let mut f = Frame::default();
        let out = emit(&mut f, g, None);
        assert!(
            out.contains("\x1b[91m"),
            "pane color passes through: {out:?}"
        );
        assert!(
            !out.contains("\x1b[39m") && !out.contains("\x1b[49m"),
            "{out:?}"
        );
    }

    #[test]
    fn dim_attribute_and_indexed_palette_sgr() {
        let mut f = Frame::default();
        let out = emit(
            &mut f,
            chrome_grid(Color::AnsiValue(8), Color::Reset, DIM),
            None,
        );
        assert!(out.contains("\x1b[2m"), "{out:?}");
        assert!(
            out.contains("\x1b[90m"),
            "ANSI 8 is the bright-black code: {out:?}"
        );
        assert_eq!(sgr_color(Color::AnsiValue(6), true), "\x1b[36m");
        assert_eq!(sgr_color(Color::AnsiValue(14), false), "\x1b[106m");
        assert_eq!(sgr_color(Color::AnsiValue(74), true), "\x1b[38;5;74m");
    }

    #[test]
    fn colored_cell_after_move_restates_its_sgr() {
        let mut g = Grid {
            w: 3,
            h: 2,
            cells: vec![Cell::default(); 6],
        };
        for y in 0..2 {
            g.put(
                0,
                y,
                Cell {
                    s: "┃".into(),
                    fg: Color::AnsiValue(6),
                    ..Cell::default()
                },
            );
        }
        let mut f = Frame::default();
        let out = emit(&mut f, g, None);
        assert_eq!(out.matches("\x1b[36m┃").count(), 2, "{out:?}");
    }
}
