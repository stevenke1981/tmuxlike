//! Pane contents (vt100 cells, copy-mode label, cursor) and the separators between panes.
use super::{BOLD, Cell, Ctx, Grid, INVERSE, ITALIC, UNDER};
use crossterm::style::Color;

fn conv(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::AnsiValue(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}

/// Paint every visible pane's cells and copy-mode label. Marks each covered cell and
/// returns the cursor position owned by the active pane, if it has one.
pub(super) fn draw_panes(g: &mut Grid, cx: &Ctx, covered: &mut [bool]) -> Option<(u16, u16)> {
    let (st, w, h) = (cx.st, cx.w, cx.h);
    let win = &cx.s.windows[cx.s.active];
    let mut cursor_pos = None;

    for (pid, r) in &cx.rects {
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
    cursor_pos
}

/// Fill uncovered cells inside the pane area with box-drawing separators. Separators
/// next to the active pane are green, all others dark grey.
pub(super) fn draw_borders(g: &mut Grid, cx: &Ctx, covered: &[bool]) {
    let (w, h) = (cx.w, cx.h);
    let area = cx.area;
    let win = &cx.s.windows[cx.s.active];
    let active_rect = cx.rects.iter().find(|(i, _)| *i == win.active).map(|x| x.1);
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
}
