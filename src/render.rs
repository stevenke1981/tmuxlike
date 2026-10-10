//! Compose a session's visible state into a cell grid and diff it into ANSI output.
use crate::server::{Client, Session, State};
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::{cursor, queue, terminal};

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
    fn text(&mut self, x: u16, y: u16, max: u16, s: &str, fg: Color, bg: Color, attrs: u8) -> u16 {
        let mut cx = x;
        for ch in s.chars() {
            if cx >= x + max || cx >= self.w {
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
                    wide: false,
                    skip: false,
                },
            );
            cx += 1;
        }
        cx - x
    }
}

fn conv(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::AnsiValue(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}

/// Window labels in the status bar: (window index, start col, end col)
fn status_labels(s: &Session) -> Vec<(usize, u16, u16, String)> {
    let mut x = (s.name.len() + 3) as u16;
    let mut v = vec![];
    for (i, w) in s.windows.iter().enumerate() {
        let flag = if i == s.active {
            "*"
        } else if w.zoomed {
            "Z"
        } else {
            " "
        };
        let label = format!("{}:{}{}", i, w.name, flag);
        let len = label.chars().count() as u16;
        v.push((i, x, x + len, label));
        x += len + 1;
    }
    v
}

pub fn status_hit(s: &Session, col: u16) -> Option<usize> {
    status_labels(s)
        .into_iter()
        .find(|(_, a, b, _)| col >= *a && col < *b)
        .map(|l| l.0)
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
    let mut cursor_pos = None;

    for (pid, r) in &rects {
        let Some(p) = st.panes.get(pid) else { continue };
        let screen = p.parser.screen();
        for row in 0..r.h {
            for col in 0..r.w {
                let (x, y) = (r.x + col, r.y + row);
                if x >= w || y >= h {
                    continue;
                }
                covered[y as usize * w as usize + x as usize] = true;
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                let mut attrs = 0;
                if cell.bold() {
                    attrs |= BOLD;
                }
                if cell.italic() {
                    attrs |= ITALIC;
                }
                if cell.underline() {
                    attrs |= UNDER;
                }
                if cell.inverse() {
                    attrs |= INVERSE;
                }
                let txt = cell.contents();
                g.put(
                    x,
                    y,
                    Cell {
                        s: if txt.is_empty() {
                            " ".into()
                        } else {
                            txt.to_string()
                        },
                        fg: conv(cell.fgcolor()),
                        bg: conv(cell.bgcolor()),
                        attrs,
                        wide: cell.is_wide(),
                        skip: cell.is_wide_continuation(),
                    },
                );
            }
        }
        if p.copy {
            let label = format!("[scroll {}/{}]", screen.scrollback(), st.cfg.scrollback);
            let lx = r.x + r.w.saturating_sub(label.len() as u16);
            g.text(lx, r.y, r.w, &label, Color::Black, Color::Yellow, 0);
        }
        if *pid == win.active && !p.copy && !screen.hide_cursor() {
            let (cy, cx) = screen.cursor_position();
            if cx < r.w && cy < r.h {
                cursor_pos = Some((r.x + cx, r.y + cy));
            }
        }
    }

    // borders
    let active_rect = rects.iter().find(|(i, _)| *i == win.active).map(|x| x.1);
    let cov = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < w as i32
            && y < h as i32
            && covered[y as usize * w as usize + x as usize]
    };
    for y in 0..area.h {
        for x in 0..area.w {
            if covered[y as usize * w as usize + x as usize] {
                continue;
            }
            let (xi, yi) = (x as i32, y as i32);
            let vertical = cov(xi - 1, yi) || cov(xi + 1, yi);
            let horizontal = cov(xi, yi - 1) || cov(xi, yi + 1);
            let ch = match (vertical, horizontal) {
                (true, false) => "│",
                (false, true) => "─",
                _ => "┼",
            };
            let near = active_rect.is_some_and(|r| {
                xi >= r.x as i32 - 1
                    && xi <= (r.x + r.w) as i32
                    && yi >= r.y as i32 - 1
                    && yi <= (r.y + r.h) as i32
            });
            let fg = if near { Color::Green } else { Color::DarkGrey };
            g.put(
                x,
                y,
                Cell {
                    s: ch.into(),
                    fg,
                    ..Cell::default()
                },
            );
        }
    }

    // sidebar (herdr-like agent/pane status)
    let sw = st.sidebar_width(s);
    if sw > 0 {
        let sx = w - sw;
        for y in 0..h.saturating_sub(1) {
            g.put(
                sx,
                y,
                Cell {
                    s: "│".into(),
                    fg: Color::DarkGrey,
                    ..Cell::default()
                },
            );
        }
        let tx = sx + 2;
        let tw = sw - 3;
        let mut y = 0u16;
        g.text(
            tx,
            y,
            tw,
            "tmuxlike · panes",
            Color::Cyan,
            Color::Reset,
            BOLD,
        );
        y += 2;
        for sess in &st.sessions {
            if y >= h - 1 {
                break;
            }
            let cur = sess.name == s.name;
            g.text(
                tx,
                y,
                tw,
                &format!("▸ {}", sess.name),
                if cur { Color::Green } else { Color::White },
                Color::Reset,
                if cur { BOLD } else { 0 },
            );
            y += 1;
            for (wi, wnd) in sess.windows.iter().enumerate() {
                for (pi, pid) in wnd.panes().into_iter().enumerate() {
                    if y >= h - 1 {
                        break;
                    }
                    let Some(p) = st.panes.get(&pid) else {
                        continue;
                    };
                    let (glyph, color, label) = if p.busy(&st.cfg) {
                        ("●", Color::Yellow, "working")
                    } else if p.attention {
                        ("◆", Color::Blue, "done")
                    } else {
                        ("○", Color::DarkGrey, "idle")
                    };
                    let focused = cur && wi == sess.active && pid == wnd.active;
                    let x2 = tx + 1;
                    g.text(x2, y, 2, glyph, color, Color::Reset, 0);
                    let txt = format!("{}.{} {} {}", wi, pi, wnd.name, label);
                    g.text(
                        x2 + 2,
                        y,
                        tw.saturating_sub(3),
                        &txt,
                        if focused { Color::Green } else { Color::Reset },
                        Color::Reset,
                        if focused { BOLD } else { 0 },
                    );
                    y += 1;
                }
            }
            y += 1;
        }
        if h > 4 {
            g.text(
                tx,
                h - 2,
                tw,
                "● work ◆ done ○ idle",
                Color::DarkGrey,
                Color::Reset,
                0,
            );
        }
    }

    // status bar
    let sy = h - 1;
    for x in 0..w {
        g.put(
            x,
            sy,
            Cell {
                bg: Color::Green,
                fg: Color::Black,
                ..Cell::default()
            },
        );
    }
    g.text(
        0,
        sy,
        w,
        &format!("[{}]", s.name),
        Color::Black,
        Color::Green,
        BOLD,
    );
    for (i, a, _, label) in status_labels(s) {
        let act = st
            .panes
            .iter()
            .any(|(id, p)| p.attention && s.windows[i].layout.contains(*id));
        let (fg, bg, at) = if i == s.active {
            (Color::Green, Color::Black, BOLD)
        } else if act {
            (Color::Black, Color::Yellow, 0)
        } else {
            (Color::Black, Color::Green, 0)
        };
        g.text(a, sy, w.saturating_sub(a), &label, fg, bg, at);
    }
    let right = if c.prefix {
        " PREFIX | tmuxlike ".to_string()
    } else {
        format!(" {} | tmuxlike ", st.cfg.prefix)
    };
    let rl = right.chars().count() as u16;
    if w > rl {
        let (fg, bg) = if c.prefix {
            (Color::Black, Color::Yellow)
        } else {
            (Color::Black, Color::Green)
        };
        g.text(w - rl, sy, rl, &right, fg, bg, 0);
    }

    Some(emit(&mut c.frame, g, cursor_pos))
}

fn emit(frame: &mut Frame, g: Grid, cursor_pos: Option<(u16, u16)>) -> String {
    let mut out: Vec<u8> = Vec::new();
    let full = frame.w != g.w || frame.h != g.h || frame.cells.len() != g.cells.len();
    let _ = queue!(out, terminal::BeginSynchronizedUpdate, cursor::Hide);
    if full {
        let _ = queue!(
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
                let _ = queue!(out, cursor::MoveTo(x, y));
            }
            if cur_attrs != Some(c.attrs) {
                let _ = queue!(out, SetAttribute(Attribute::Reset));
                if c.attrs & BOLD != 0 {
                    let _ = queue!(out, SetAttribute(Attribute::Bold));
                }
                if c.attrs & ITALIC != 0 {
                    let _ = queue!(out, SetAttribute(Attribute::Italic));
                }
                if c.attrs & UNDER != 0 {
                    let _ = queue!(out, SetAttribute(Attribute::Underlined));
                }
                if c.attrs & INVERSE != 0 {
                    let _ = queue!(out, SetAttribute(Attribute::Reverse));
                }
                cur_attrs = Some(c.attrs);
                cur_fg = None;
                cur_bg = None;
            }
            if cur_fg != Some(c.fg) {
                let _ = queue!(out, SetForegroundColor(c.fg));
                cur_fg = Some(c.fg);
            }
            if cur_bg != Some(c.bg) {
                let _ = queue!(out, SetBackgroundColor(c.bg));
                cur_bg = Some(c.bg);
            }
            let _ = queue!(out, Print(&c.s));
            let adv = if c.wide { 2 } else { 1 };
            pos = if x + adv < g.w {
                Some((x + adv, y))
            } else {
                None
            };
        }
    }
    let _ = queue!(out, SetAttribute(Attribute::Reset));
    if let Some((x, y)) = cursor_pos {
        let _ = queue!(out, cursor::MoveTo(x, y), cursor::Show);
    }
    let _ = queue!(out, terminal::EndSynchronizedUpdate);
    let _ = changed;
    frame.w = g.w;
    frame.h = g.h;
    frame.cells = g.cells;
    String::from_utf8(out).unwrap_or_default()
}
