//! Pane contents (vt100 cells, copy-mode label, cursor), pane title rows, and the separators
//! between panes (ui-spec §3.1, §3.4, UI-03, UI-11).
use super::{BOLD, Cell, Ctx, DIM, Grid, INVERSE, ITALIC, UNDER, truncate};
use crate::api::{AgentInfo, pane_id_str};
use crate::layout::Rect;
use crossterm::style::Color;
use unicode_width::UnicodeWidthStr;

fn conv(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::AnsiValue(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}

/// Paint every visible pane's cells, its title row and copy-mode label. Marks each covered
/// cell and returns the cursor position owned by the active pane, if it has one.
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
            let lx = r.x + r.w.saturating_sub(label.width() as u16);
            let theme = &cx.theme;
            let attrs = if theme.mono { INVERSE } else { 0 };
            g.text(
                lx,
                r.y,
                r.w,
                &label,
                theme.c(theme.bar_bg),
                theme.c(theme.warn),
                attrs,
            );
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
    draw_titles(g, cx, covered);
    cursor_pos
}

/// Name shown in a pane title: agent name, else agent kind, else the user label.
pub(crate) fn title_name<'a>(agent: Option<&'a AgentInfo>, label: Option<&'a str>) -> &'a str {
    agent
        .and_then(|a| {
            a.name
                .as_deref()
                .filter(|n| !n.is_empty())
                .or(Some(a.agent.as_str()).filter(|n| !n.is_empty()))
        })
        .or(label.filter(|l| !l.is_empty()))
        .unwrap_or("pane")
}

/// Fit a title row of `width` columns: `left` is cut with `…` (CJK-safe); `right` (the pane id)
/// is kept whole and dropped only when the left part would get less than 4 columns.
pub(crate) fn fit_title(left: &str, right: &str, width: u16) -> (String, Option<String>) {
    const MIN_LEFT: usize = 4;
    let width = width as usize;
    let rw = right.width();
    let (left_max, right) = if rw + 1 + MIN_LEFT <= width {
        (width - rw - 1, Some(right.to_string()))
    } else {
        (width, None)
    };
    let left = if left_max == 0 {
        String::new()
    } else {
        truncate(left, left_max)
    };
    (left, right)
}

/// Paint the first row of every titled pane: `glyph name · state` on the left, `pN` on the
/// right. The focused title is a chip (accent background; INVERSE in mono). Title cells are
/// marked covered so no divider is drawn over them.
fn draw_titles(g: &mut Grid, cx: &Ctx, covered: &mut [bool]) {
    let win = &cx.s.windows[cx.s.active];
    let theme = &cx.theme;
    let muted_attrs = if theme.mono { DIM } else { 0 };
    for (pid, r) in cx.st.title_rows(cx.s, win) {
        for x in r.x..r.x + r.w {
            if x < cx.w && r.y < cx.h {
                covered[r.y as usize * cx.w as usize + x as usize] = true;
            }
        }
        let Some(p) = cx.st.panes.get(&pid) else {
            continue;
        };
        let (glyph, _, word) = theme.status(crate::api::agent::status(p, &cx.st.cfg));
        let name = title_name(p.agent.as_ref(), p.label.as_deref());
        let left = format!(" {glyph} {name} · {word} ");
        let (left, right) = fit_title(&left, &format!("{} ", pane_id_str(pid)), r.w);
        if pid == win.active {
            let attrs = BOLD | if theme.mono { INVERSE } else { 0 };
            g.text(
                r.x,
                r.y,
                r.w,
                &left,
                theme.c(theme.bar_bg),
                theme.c(theme.accent),
                attrs,
            );
        } else {
            g.text(
                r.x,
                r.y,
                r.w,
                &left,
                theme.c(theme.muted),
                Color::Reset,
                muted_attrs,
            );
        }
        if let Some(right) = right {
            let rw = right.width() as u16;
            g.text(
                r.x + r.w - rw,
                r.y,
                rw,
                &right,
                theme.c(theme.muted),
                Color::Reset,
                muted_attrs,
            );
        }
    }
}

/// Which neighbours of a border cell are also border cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Arms {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl Arms {
    fn count(self) -> usize {
        [self.up, self.down, self.left, self.right]
            .iter()
            .filter(|a| **a)
            .count()
    }

    /// Straight vertical or straight horizontal run (no junction).
    fn is_straight(self) -> bool {
        (self.up && self.down && !self.left && !self.right)
            || (self.left && self.right && !self.up && !self.down)
    }
}

/// One border cell of the separator grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub x: u16,
    pub y: u16,
    pub glyph: &'static str,
    /// the cell touches the focused pane (or, for a junction, one of its arms does)
    pub active: bool,
}

/// Glyph for one border cell. Straight segments use the heavy glyph when `active`; junctions
/// keep the thin tee/corner glyph; `┼` only where four arms meet.
pub fn edge_kind(arms: Arms, active: bool, g: &crate::theme::Glyphs) -> &'static str {
    let (u, d, l, r) = (arms.up, arms.down, arms.left, arms.right);
    match (u, d, l, r) {
        (true, true, true, true) => g.cross,
        (true, true, true, false) => g.tee_left,
        (true, true, false, true) => g.tee_right,
        (true, false, true, true) => g.tee_up,
        (false, true, true, true) => g.tee_down,
        (false, true, false, true) => g.corner_tl,
        (false, true, true, false) => g.corner_tr,
        (true, false, false, true) => g.corner_bl,
        (true, false, true, false) => g.corner_br,
        _ => {
            let horizontal = (l || r) && !(u || d);
            match (horizontal, active) {
                (true, true) => g.hline_active,
                (true, false) => g.hline,
                (false, true) => g.vline_active,
                (false, false) => g.vline,
            }
        }
    }
}

fn neighbours(x: i32, y: i32) -> [(i32, i32); 4] {
    [(x, y - 1), (x, y + 1), (x - 1, y), (x + 1, y)]
}

fn in_rect(r: Rect, x: i32, y: i32) -> bool {
    x >= r.x as i32 && x < (r.x + r.w) as i32 && y >= r.y as i32 && y < (r.y + r.h) as i32
}

/// Every uncovered cell of `area` with its separator glyph. `covered` tells whether a cell
/// belongs to a pane or title row; `focus` is the active pane's full outline (title row
/// included). A cell is active when it touches `focus`; a junction is also active when one of
/// its border arms touches `focus`.
pub fn edge_map(
    area: Rect,
    covered: impl Fn(i32, i32) -> bool,
    focus: Option<Rect>,
    g: &crate::theme::Glyphs,
) -> Vec<Edge> {
    let (x0, y0) = (area.x as i32, area.y as i32);
    let (x1, y1) = (x0 + area.w as i32, y0 + area.h as i32);
    let border = |x: i32, y: i32| x >= x0 && y >= y0 && x < x1 && y < y1 && !covered(x, y);
    let touches = |x: i32, y: i32| {
        focus.is_some_and(|f| neighbours(x, y).iter().any(|&(nx, ny)| in_rect(f, nx, ny)))
    };
    let mut out = vec![];
    for y in y0..y1 {
        for x in x0..x1 {
            if !border(x, y) {
                continue;
            }
            let arms = Arms {
                up: border(x, y - 1),
                down: border(x, y + 1),
                left: border(x - 1, y),
                right: border(x + 1, y),
            };
            let is_junction = !arms.is_straight() && arms.count() >= 2;
            let arm_touches = is_junction
                && neighbours(x, y)
                    .iter()
                    .any(|&(nx, ny)| border(nx, ny) && touches(nx, ny));
            let active = touches(x, y) || arm_touches;
            out.push(Edge {
                x: x as u16,
                y: y as u16,
                glyph: edge_kind(arms, active, g),
                active,
            });
        }
    }
    out
}

/// Full outline of pane `pid` in the active window: its content rect, plus the title row above
/// it when the pane is titled.
fn outline(cx: &Ctx, pid: usize) -> Option<Rect> {
    let win = &cx.s.windows[cx.s.active];
    let content = cx.rects.iter().find(|(i, _)| *i == pid)?.1;
    let titled = cx.st.title_rows(cx.s, win).iter().any(|(i, _)| *i == pid);
    if titled {
        Some(Rect {
            y: content.y - 1,
            h: content.h + 1,
            ..content
        })
    } else {
        Some(content)
    }
}

/// Fill uncovered cells inside the pane area with box-drawing separators. Only the cells
/// touching the focused pane are drawn in `border_active` (heavy when straight); all other
/// separators use the thin glyph in `border`. Mono: the focused edge is BOLD.
pub(super) fn draw_borders(g: &mut Grid, cx: &Ctx, covered: &[bool]) {
    let (w, h) = (cx.w as i32, cx.h as i32);
    let win = &cx.s.windows[cx.s.active];
    let focus = outline(cx, win.active);
    let cov = |x: i32, y: i32| {
        x >= 0 && y >= 0 && x < w && y < h && covered[y as usize * w as usize + x as usize]
    };
    let theme = &cx.theme;
    for e in edge_map(cx.area, cov, focus, &theme.g) {
        let (fg, attrs) = if e.active {
            (
                theme.c(theme.border_active),
                if theme.mono { BOLD } else { 0 },
            )
        } else {
            (theme.c(theme.border), 0)
        };
        g.put(
            e.x,
            e.y,
            Cell {
                s: e.glyph.into(),
                fg,
                attrs,
                ..Cell::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::UNICODE;

    fn covered_by(rects: &[Rect]) -> impl Fn(i32, i32) -> bool + '_ {
        move |x, y| rects.iter().any(|r| in_rect(*r, x, y))
    }

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    fn glyph_at(edges: &[Edge], x: u16, y: u16) -> (&'static str, bool) {
        let e = edges
            .iter()
            .find(|e| e.x == x && e.y == y)
            .unwrap_or_else(|| panic!("no edge at {x},{y}"));
        (e.glyph, e.active)
    }

    /// Two panes side by side: A = 0..5, divider col 5, B = 6..10; 11x5 area.
    fn side_by_side() -> Vec<Rect> {
        vec![r(0, 0, 5, 5), r(6, 0, 5, 5)]
    }

    /// 3-pane L: A full height on the left, B top-right, C bottom-right; 11x11 area.
    fn three_pane_l() -> Vec<Rect> {
        vec![r(0, 0, 5, 11), r(6, 0, 5, 5), r(6, 6, 5, 5)]
    }

    fn edges(rects: &[Rect], area: Rect, focus: Option<usize>) -> Vec<Edge> {
        let f = focus.map(|i| rects[i]);
        edge_map(area, covered_by(rects), f, &UNICODE)
    }

    #[test]
    fn side_by_side_focus_lights_only_the_shared_edge() {
        let rects = side_by_side();
        let area = r(0, 0, 11, 5);
        let e = edges(&rects, area, Some(0));
        assert_eq!(e.len(), 5, "only the divider column is border");
        for y in 0..5 {
            assert_eq!(glyph_at(&e, 5, y), ("┃", true), "row {y}");
        }
        let none = edges(&rects, area, None);
        for y in 0..5 {
            assert_eq!(glyph_at(&none, 5, y), ("│", false));
        }
    }

    #[test]
    fn focus_on_b_also_marks_the_divider_heavy() {
        let rects = side_by_side();
        let e = edges(&rects, r(0, 0, 11, 5), Some(1));
        assert!(e.iter().all(|e| e.active && e.glyph == "┃"));
    }

    #[test]
    fn l_layout_focus_on_bottom_right_is_per_segment() {
        let rects = three_pane_l();
        let e = edges(&rects, r(0, 0, 11, 11), Some(2));
        // the vertical divider beside the top-right pane stays thin
        for y in 0..5 {
            assert_eq!(glyph_at(&e, 5, y), ("│", false), "top row {y}");
        }
        // the divider segment beside C is heavy
        for y in 6..11 {
            assert_eq!(glyph_at(&e, 5, y), ("┃", true), "bottom row {y}");
        }
        // the join where the three arms meet is a thin tee drawn in the active color
        assert_eq!(glyph_at(&e, 5, 5), ("├", true));
        // the horizontal divider under B is the segment that touches C
        for x in 6..11 {
            assert_eq!(glyph_at(&e, x, 5), ("━", true), "x {x}");
        }
    }

    #[test]
    fn l_layout_focus_on_top_right_moves_the_active_edge() {
        let rects = three_pane_l();
        let e = edges(&rects, r(0, 0, 11, 11), Some(1));
        for y in 0..5 {
            assert_eq!(glyph_at(&e, 5, y), ("┃", true));
        }
        for y in 6..11 {
            assert_eq!(glyph_at(&e, 5, y), ("│", false));
        }
        assert_eq!(glyph_at(&e, 5, 5), ("├", true));
        assert_eq!(glyph_at(&e, 8, 5), ("━", true));
    }

    #[test]
    fn l_layout_focus_on_left_pane_keeps_other_joins_thin() {
        let rects = three_pane_l();
        let e = edges(&rects, r(0, 0, 11, 11), Some(0));
        assert_eq!(glyph_at(&e, 5, 3), ("┃", true));
        // the stem of the join touches A; the horizontal divider does not
        assert_eq!(glyph_at(&e, 5, 5), ("├", true));
        // the horizontal divider under B touches neither A nor a junction arm: thin
        assert_eq!(glyph_at(&e, 8, 5), ("─", false));
    }

    #[test]
    fn four_arms_meet_gives_cross() {
        let rects = vec![r(0, 0, 5, 5), r(6, 0, 5, 5), r(0, 6, 5, 5), r(6, 6, 5, 5)];
        let e = edges(&rects, r(0, 0, 11, 11), Some(0));
        assert_eq!(glyph_at(&e, 5, 5).0, "┼");
        // the cross touches A through its upper arm
        assert!(glyph_at(&e, 5, 5).1);
        assert_eq!(glyph_at(&e, 5, 8), ("│", false));
    }

    #[test]
    fn divider_beside_title_row_stays_straight() {
        // Pane A's title row is y=0 (covered); the gap cell (5,0) has covered neighbours on
        // both sides, so it is a plain vertical separator.
        let rects = vec![r(0, 0, 5, 5), r(6, 0, 5, 5)];
        let e = edges(&rects, r(0, 0, 11, 5), Some(0));
        assert_eq!(glyph_at(&e, 5, 0), ("┃", true));
    }

    #[test]
    fn edge_kind_glyph_table() {
        let g = &UNICODE;
        let arms = |u, d, l, rr| Arms {
            up: u,
            down: d,
            left: l,
            right: rr,
        };
        assert_eq!(edge_kind(arms(true, true, false, false), false, g), "│");
        assert_eq!(edge_kind(arms(true, true, false, false), true, g), "┃");
        assert_eq!(edge_kind(arms(false, false, true, true), true, g), "━");
        assert_eq!(edge_kind(arms(true, true, true, false), true, g), "┤");
        assert_eq!(edge_kind(arms(true, true, false, true), false, g), "├");
        assert_eq!(edge_kind(arms(true, false, true, true), false, g), "┴");
        assert_eq!(edge_kind(arms(false, true, true, true), false, g), "┬");
        assert_eq!(edge_kind(arms(true, true, true, true), false, g), "┼");
        assert_eq!(edge_kind(arms(false, true, false, true), false, g), "┌");
        assert_eq!(edge_kind(arms(false, true, true, false), false, g), "┐");
        assert_eq!(edge_kind(arms(true, false, false, true), false, g), "└");
        assert_eq!(edge_kind(arms(true, false, true, false), false, g), "┘");
        assert_eq!(edge_kind(arms(true, false, false, false), false, g), "│");
    }

    #[test]
    fn fit_title_cuts_cjk_safely_and_keeps_pane_id() {
        let (left, right) = fit_title(" ○ 測試代理人 · idle ", "p5 ", 16);
        assert_eq!(right.as_deref(), Some("p5 "));
        assert!(left.ends_with('…'), "{left:?}");
        assert!(left.width() < 16 - 3, "{left:?}");
        let (left, right) = fit_title(" ▲ claude · blocked ", "p12 ", 8);
        assert!(right.is_none(), "too narrow: id dropped");
        assert!(left.width() <= 8);
        assert_eq!(fit_title("abc", "p1 ", 0), (String::new(), None));
    }

    #[test]
    fn title_name_prefers_agent_name_then_kind_then_label() {
        let mut a = AgentInfo {
            source: "s".into(),
            agent: "claude".into(),
            state: crate::api::Status::Blocked,
            message: None,
            seq: None,
            name: Some("reviewer".into()),
            resume: vec![],
        };
        assert_eq!(title_name(Some(&a), Some("lab")), "reviewer");
        a.name = None;
        assert_eq!(title_name(Some(&a), Some("lab")), "claude");
        a.agent = String::new();
        assert_eq!(title_name(Some(&a), Some("lab")), "lab");
        assert_eq!(title_name(None, None), "pane");
    }
}
