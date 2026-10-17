//! Compose a session's visible state into a cell grid and diff it into ANSI output.
//!
//! `render_client` computes the shared [`Ctx`] and the grid, then calls the section
//! drawers in paint order: panes, borders, sidebar, status bar, overlays.
mod overlay;
mod panes;
mod sidebar;
pub use sidebar::{SidebarKind, sidebar_rows, sidebar_text_width};
mod status;

pub use overlay::chooser_entries;
pub use status::status_hit;

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
        Color::AnsiValue(i) => format!("\x1b[{};5;{}m", if fg { 38 } else { 48 }, i),
        Color::Rgb { r, g, b } => format!("\x1b[{};2;{};{};{}m", if fg { 38 } else { 48 }, r, g, b),
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
            if pos != Some((x, y)) {
                ansi!(out, cursor::MoveTo(x, y));
            }
            if cur_attrs != Some(c.attrs) {
                ansi!(out, SetAttribute(Attribute::Reset));
                if c.attrs & BOLD != 0 {
                    ansi!(out, SetAttribute(Attribute::Bold));
                }
                if c.attrs & ITALIC != 0 {
                    ansi!(out, SetAttribute(Attribute::Italic));
                }
                if c.attrs & UNDER != 0 {
                    ansi!(out, SetAttribute(Attribute::Underlined));
                }
                if c.attrs & INVERSE != 0 {
                    ansi!(out, SetAttribute(Attribute::Reverse));
                }
                cur_attrs = Some(c.attrs);
                cur_fg = None;
                cur_bg = None;
            }
            if cur_fg != Some(c.fg) {
                out.push_str(&sgr_color(c.fg, true));
                cur_fg = Some(c.fg);
            }
            if cur_bg != Some(c.bg) {
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
}
