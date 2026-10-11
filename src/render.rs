//! Compose a session's visible state into a cell grid and diff it into ANSI output.
use crate::server::{Client, PromptKind, Session, State};
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

/// Entries for the session/window chooser: (session index, window index or None for the session row)
pub fn chooser_entries(st: &State) -> Vec<(usize, Option<usize>)> {
    let mut v = vec![];
    for (si, s) in st.sessions.iter().enumerate() {
        v.push((si, None));
        for wi in 0..s.windows.len() {
            v.push((si, Some(wi)));
        }
    }
    v
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
        let len = label.width() as u16;
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
        let off = screen.scrollback();
        let sel = p.copy.as_ref().and_then(|c| {
            c.anchor
                .map(|a| (a, (crate::copy::abs_row(c.cy, off), c.cx), c.line))
        });
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
                if let Some((a, b, line)) = sel {
                    if crate::copy::in_selection(a, b, line, (crate::copy::abs_row(row, off), col))
                    {
                        attrs ^= INVERSE;
                    }
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
        if let Some(cs) = &p.copy {
            let label = format!(
                "[copy {}/{}{}]",
                screen.scrollback(),
                st.cfg.scrollback,
                if cs.anchor.is_some() { " sel" } else { "" }
            );
            let lx = r.x + r.w.saturating_sub(label.len() as u16);
            g.text(lx, r.y, r.w, &label, Color::Black, Color::Yellow, 0);
        }
        if let Some(cs) = &p.copy {
            if *pid == win.active {
                cursor_pos = Some((r.x + cs.cx, r.y + cs.cy));
            }
        } else if *pid == win.active && !screen.hide_cursor() {
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
    let mut cursor_pos = cursor_pos;
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
    let rl = right.width() as u16;
    if w > rl {
        let (fg, bg) = if c.prefix {
            (Color::Black, Color::Yellow)
        } else {
            (Color::Black, Color::Green)
        };
        g.text(w - rl, sy, rl, &right, fg, bg, 0);
    }

    if let Some(p) = &c.prompt {
        let label = match p.kind {
            PromptKind::Command => ":".to_string(),
            PromptKind::RenameWindow => "(rename-window) ".into(),
            PromptKind::RenameSession => "(rename-session) ".into(),
        };
        for x in 0..w {
            g.put(
                x,
                sy,
                Cell {
                    bg: Color::Yellow,
                    fg: Color::Black,
                    ..Cell::default()
                },
            );
        }
        let text = format!("{label}{}", p.buf);
        // keep the end of the input visible
        let mut shown = text.as_str();
        while shown.width() as u16 >= w && !shown.is_empty() {
            let mut it = shown.chars();
            it.next();
            shown = it.as_str();
        }
        let used = g.text(0, sy, w, shown, Color::Black, Color::Yellow, 0);
        cursor_pos = Some((used.min(w - 1), sy));
    } else if let Some((m, _)) = &c.message {
        for x in 0..w {
            g.put(
                x,
                sy,
                Cell {
                    bg: Color::Yellow,
                    fg: Color::Black,
                    ..Cell::default()
                },
            );
        }
        g.text(0, sy, w, m, Color::Black, Color::Yellow, 0);
    }

    // overlays
    let overlay =
        |g: &mut Grid, title: &str, lines: &[(String, bool)], sel: Option<usize>, scroll: usize| {
            let bw = (lines
                .iter()
                .map(|l| l.0.width())
                .max()
                .unwrap_or(10)
                .max(title.width()) as u16
                + 4)
            .min(w.saturating_sub(2))
            .max(10);
            let maxh = h.saturating_sub(3).max(3);
            let bh = (lines.len() as u16 + 2).min(maxh);
            let bx = w.saturating_sub(bw) / 2;
            let by = h.saturating_sub(1).saturating_sub(bh) / 2;
            let inner = (bh - 2) as usize;
            let start = sel
                .map(|s| s.saturating_sub(inner.saturating_sub(1)))
                .unwrap_or(scroll.min(lines.len().saturating_sub(inner)));
            for yy in 0..bh {
                for xx in 0..bw {
                    let edge = yy == 0 || yy == bh - 1 || xx == 0 || xx == bw - 1;
                    let ch = match (yy, xx) {
                        (0, 0) => "┌",
                        (0, x) if x == bw - 1 => "┐",
                        (y, 0) if y == bh - 1 => "└",
                        (y, x) if y == bh - 1 && x == bw - 1 => "┘",
                        (0, _) | (_, _) if edge && (yy == 0 || yy == bh - 1) => "─",
                        _ if edge => "│",
                        _ => " ",
                    };
                    g.put(
                        bx + xx,
                        by + yy,
                        Cell {
                            s: ch.into(),
                            fg: Color::Cyan,
                            ..Cell::default()
                        },
                    );
                }
            }
            g.text(
                bx + 2,
                by,
                bw.saturating_sub(4),
                &format!(" {title} "),
                Color::Cyan,
                Color::Reset,
                BOLD,
            );
            for (i, (l, bold)) in lines.iter().enumerate().skip(start).take(inner) {
                let y = by + 1 + (i - start) as u16;
                let selected = sel == Some(i);
                let (fg, bg) = if selected {
                    (Color::Black, Color::Cyan)
                } else {
                    (Color::Reset, Color::Reset)
                };
                if selected {
                    for xx in 1..bw - 1 {
                        g.put(
                            bx + xx,
                            y,
                            Cell {
                                bg,
                                ..Cell::default()
                            },
                        );
                    }
                }
                let t = truncate(l, (bw - 4) as usize);
                g.text(bx + 2, y, bw - 4, &t, fg, bg, if *bold { BOLD } else { 0 });
            }
        };
    if let Some(sel) = c.chooser {
        let lines: Vec<(String, bool)> = chooser_entries(st)
            .into_iter()
            .map(|(si, wi)| {
                let ss = &st.sessions[si];
                match wi {
                    None => (
                        format!(
                            "{}: {} windows{}",
                            ss.name,
                            ss.windows.len(),
                            if ss.name == s.name { " (attached)" } else { "" }
                        ),
                        true,
                    ),
                    Some(wi) => {
                        let wn = &ss.windows[wi];
                        let busy = wn
                            .panes()
                            .iter()
                            .any(|p| st.panes.get(p).is_some_and(|p| p.busy(&st.cfg)));
                        (
                            format!(
                                "  ├─ {}: {} [{} panes]{}{}",
                                wi,
                                wn.name,
                                wn.panes().len(),
                                if wi == ss.active { " *" } else { "" },
                                if busy { " ●" } else { "" }
                            ),
                            false,
                        )
                    }
                }
            })
            .collect();
        overlay(
            &mut g,
            "choose session/window — ↑↓ Enter, x kill, Esc",
            &lines,
            Some(sel),
            0,
        );
        cursor_pos = None;
    } else if let Some((p, scroll)) = &c.popup {
        let lines: Vec<(String, bool)> = p.iter().map(|l| (l.clone(), false)).collect();
        overlay(&mut g, "keys", &lines, None, *scroll);
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
