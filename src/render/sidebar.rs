//! Right-hand sidebar: agents grouped by session, a legend and a key hint.
//!
//! Layout is pure: [`sidebar_rows`] decides what each line shows. The drawer and the mouse
//! handler both use it, so a click always lands on the line that was drawn.
use super::{BOLD, Cell, Ctx, Grid, truncate};
use crate::api::Status;
use crate::api::agent::status as agent_status;
use crate::server::{Session, State};
use crate::theme::Theme;
use crossterm::style::Color;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Text starts this many columns right of the sidebar's left edge (rule + gutter).
const TEXT_INSET: u16 = 2;
/// Columns kept blank at the right edge.
const RIGHT_GUTTER: u16 = 1;
/// `ptr sp glyph sp` before a pane name.
const PANE_PREFIX: u16 = 4;
/// Narrowest name budget that still shows a state word or a count.
const MIN_NAME: u16 = 4;
/// The footer needs at least this many list rows above it.
const MIN_LIST: u16 = 2;
/// Legend switches to the spaced form at this text width.
const LEGEND_SPACED: u16 = 28;
/// A blank line separates the title from the list when the list is at least this tall.
const GAP_MIN_LIST: u16 = 6;
/// Empty-state hint is wrapped to at most this many lines.
const EMPTY_LINES: usize = 3;
const EMPTY_TEXT: &str = "no agent reports — see: tmuxlike pane report-agent --help";
const LEGEND: [(Status, &str); 4] = [
    (Status::Working, "work"),
    (Status::Blocked, "block"),
    (Status::Done, "done"),
    (Status::Idle, "idle"),
];
/// Footer layouts, richest first: (legend, hint). The legend has a blank line above it.
const FOOTER_WITH_HINT: [(bool, bool); 3] = [(true, true), (true, false), (false, false)];
const FOOTER_NO_HINT: [(bool, bool); 2] = [(true, false), (false, false)];

/// What one sidebar line is. Only `Pane` and `Detail` focus a pane; only `SessionClosed` switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarKind {
    Title,
    /// header of the attached session (expanded, accent)
    SessionOpen,
    /// one-line summary of another session, with counts
    SessionClosed,
    Pane,
    /// message under a blocked pane
    Detail,
    /// line `n` of the empty-state hint
    Empty(usize),
    /// `… +N more`: `N` lines below are hidden
    More(usize),
    Legend,
    Hint,
    /// blank spacer, never drawn
    Gap,
}

/// One drawn line. `pane` is a pane id; `session` indexes `State::sessions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarRow {
    pub y: u16,
    pub kind: SidebarKind,
    pub pane: Option<usize>,
    pub session: Option<usize>,
}

#[derive(Debug, Clone)]
struct PaneView {
    pid: usize,
    status: Status,
    name: String,
    /// agent message, only for a blocked pane
    message: Option<String>,
    focused: bool,
}

#[derive(Debug, Clone)]
struct GroupView {
    si: usize,
    name: String,
    open: bool,
    panes: Vec<PaneView>,
}

/// Text width of the sidebar in columns; 0 when the sidebar is hidden.
pub fn sidebar_text_width(st: &State, s: &Session) -> u16 {
    st.sidebar_width(s)
        .saturating_sub(TEXT_INSET + RIGHT_GUTTER)
}

/// Lines the sidebar draws for session `s` in a `height`-row client (the last row is the
/// status bar). `width` is [`sidebar_text_width`].
pub fn sidebar_rows(st: &State, s: &Session, width: u16, height: u16) -> Vec<SidebarRow> {
    let groups = group_views(st, s);
    layout_rows(&groups, any_agent(st), st.cfg.hints, width, height)
}

fn any_agent(st: &State) -> bool {
    st.panes.values().any(|p| p.agent.is_some())
}

fn group_views(st: &State, s: &Session) -> Vec<GroupView> {
    st.sessions
        .iter()
        .enumerate()
        .map(|(si, sess)| {
            let open = sess.name == s.name;
            GroupView {
                si,
                name: sess.name.clone(),
                open,
                panes: pane_views(st, sess, open),
            }
        })
        .collect()
}

fn pane_views(st: &State, sess: &Session, open: bool) -> Vec<PaneView> {
    let mut v = vec![];
    for (wi, wnd) in sess.windows.iter().enumerate() {
        for pid in wnd.panes() {
            let Some(p) = st.panes.get(&pid) else {
                continue;
            };
            let agent = p.agent.as_ref();
            let status = agent_status(p, &st.cfg);
            let name = agent
                .and_then(|a| non_empty(a.name.as_deref()).or(non_empty(Some(a.agent.as_str()))))
                .or(non_empty(p.label.as_deref()))
                .unwrap_or(wnd.name.as_str());
            let message = match status {
                Status::Blocked => agent
                    .and_then(|a| non_empty(a.message.as_deref()))
                    .map(clean),
                _ => None,
            };
            v.push(PaneView {
                pid,
                status,
                name: clean(name),
                message,
                focused: open && wi == sess.active && pid == wnd.active,
            });
        }
    }
    v
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|x| !x.trim().is_empty())
}

/// Replace control characters so a name or message cannot break the row.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn item(kind: SidebarKind, pane: Option<usize>, session: Option<usize>) -> SidebarRow {
    SidebarRow {
        y: 0,
        kind,
        pane,
        session,
    }
}

/// Pure line layout: title, list (ends with `… +N more` on overflow), then the footer.
fn layout_rows(
    groups: &[GroupView],
    any_agent: bool,
    hints: bool,
    width: u16,
    height: u16,
) -> Vec<SidebarRow> {
    let avail = height.saturating_sub(1);
    if width == 0 || avail == 0 {
        return vec![];
    }
    let (legend, hint) = footer_plan(hints, avail);
    let footer = if legend { 2 + u16::from(hint) } else { 0 };
    let list_h = avail - footer;
    let mut rows = vec![];
    if list_h >= 1 {
        rows.push(item(SidebarKind::Title, None, None));
    }
    let top = if list_h >= GAP_MIN_LIST { 2 } else { 1 };
    let room = list_h.saturating_sub(top) as usize;
    let mut items = list_items(groups, any_agent, hints, width);
    if room == 0 {
        items.clear();
    } else if items.len() > room {
        let keep = room - 1;
        let hidden = items.len() - keep;
        items.truncate(keep);
        items.push(item(SidebarKind::More(hidden), None, None));
    }
    for (i, mut it) in items.into_iter().enumerate() {
        it.y = top + i as u16;
        rows.push(it);
    }
    push_footer(&mut rows, avail, legend, hint);
    rows
}

/// The richest footer that still leaves `MIN_LIST` rows for the list.
fn footer_plan(hints: bool, avail: u16) -> (bool, bool) {
    let plans: &[(bool, bool)] = if hints {
        &FOOTER_WITH_HINT
    } else {
        &FOOTER_NO_HINT
    };
    for &(legend, hint) in plans {
        let total = if legend { 2 + u16::from(hint) } else { 0 };
        if avail >= total + MIN_LIST {
            return (legend, hint);
        }
    }
    (false, false)
}

fn push_footer(rows: &mut Vec<SidebarRow>, avail: u16, legend: bool, hint: bool) {
    let mut y = avail;
    if hint {
        y -= 1;
        rows.push(item(SidebarKind::Hint, None, None));
        if let Some(r) = rows.last_mut() {
            r.y = y;
        }
    }
    if legend {
        y -= 1;
        rows.push(item(SidebarKind::Legend, None, None));
        if let Some(r) = rows.last_mut() {
            r.y = y;
        }
    }
}

/// Rows above the footer, in order. Overflow is trimmed by `layout_rows`.
fn list_items(groups: &[GroupView], any_agent: bool, hints: bool, width: u16) -> Vec<SidebarRow> {
    let mut items = vec![];
    if hints && !any_agent {
        let n = wrap(EMPTY_TEXT, width).len().min(EMPTY_LINES);
        items.extend((0..n).map(|i| item(SidebarKind::Empty(i), None, None)));
    }
    for g in groups {
        if !g.open {
            items.push(item(SidebarKind::SessionClosed, None, Some(g.si)));
            continue;
        }
        items.push(item(SidebarKind::SessionOpen, None, Some(g.si)));
        for p in &g.panes {
            items.push(item(SidebarKind::Pane, Some(p.pid), Some(g.si)));
            if p.message.is_some() {
                items.push(item(SidebarKind::Detail, Some(p.pid), Some(g.si)));
            }
        }
        items.push(item(SidebarKind::Gap, None, None));
    }
    while items.last().is_some_and(|r| r.kind == SidebarKind::Gap) {
        items.pop();
    }
    items
}

/// Greedy word wrap by display width; a word wider than `width` is split between characters.
fn wrap(text: &str, width: u16) -> Vec<String> {
    let max = width as usize;
    let mut lines = vec![];
    if max == 0 {
        return lines;
    }
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let fits = if cur.is_empty() {
            word.width() <= max
        } else {
            cur.width() + 1 + word.width() <= max
        };
        if fits {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        for ch in word.chars() {
            let cw = ch.width().unwrap_or(0);
            if !cur.is_empty() && cur.width() + cw > max {
                lines.push(std::mem::take(&mut cur));
            }
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Blocked, done, working counts for a session. Idle is shown only when nothing else is.
fn counts(panes: &[PaneView]) -> Vec<(Status, usize)> {
    let n = |want: Status| panes.iter().filter(|p| p.status == want).count();
    let mut v: Vec<(Status, usize)> = [Status::Blocked, Status::Done, Status::Working]
        .into_iter()
        .map(|s| (s, n(s)))
        .filter(|(_, c)| *c > 0)
        .collect();
    if v.is_empty() {
        let idle = n(Status::Idle);
        if idle > 0 {
            v.push((Status::Idle, idle));
        }
    }
    v
}

/// Legend such as `●work ▲block ◆done ○idle`, or `● work ▲ block ◆ done ○ idle` when it fits.
fn legend_text(t: &Theme, spaced: bool) -> String {
    LEGEND
        .iter()
        .map(|(s, word)| {
            let glyph = t.status(*s).0;
            if spaced {
                format!("{glyph} {word}")
            } else {
                format!("{glyph}{word}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn hint_text(prefix: &str) -> String {
    format!("{prefix} a hide  {prefix} s switch")
}

/// Where the sidebar draws: text origin and width, plus the theme and the prefix key.
struct Lane<'a> {
    t: &'a Theme,
    tx: u16,
    tw: u16,
    prefix: String,
}

/// Draw the sidebar (if the session has room for one) along the right edge.
pub(super) fn draw_sidebar(g: &mut Grid, cx: &Ctx) {
    let (st, s, w, h) = (cx.st, cx.s, cx.w, cx.h);
    let sw = st.sidebar_width(s);
    if sw == 0 {
        return;
    }
    let sx = w - sw;
    let t = &cx.theme;
    for y in 0..h.saturating_sub(1) {
        g.put(
            sx,
            y,
            Cell {
                s: t.g.vline.into(),
                fg: t.c(t.border),
                ..Cell::default()
            },
        );
    }
    let tw = sidebar_text_width(st, s);
    let groups = group_views(st, s);
    let lane = Lane {
        t,
        tx: sx + TEXT_INSET,
        tw,
        prefix: st.cfg.prefix_name(),
    };
    for row in layout_rows(&groups, any_agent(st), st.cfg.hints, tw, h) {
        draw_row(g, &lane, &row, &groups);
    }
}

fn draw_row(g: &mut Grid, l: &Lane, row: &SidebarRow, groups: &[GroupView]) {
    let y = row.y;
    let group = row
        .session
        .and_then(|si| groups.iter().find(|x| x.si == si));
    let pane = row.pane.and_then(|pid| {
        groups
            .iter()
            .flat_map(|x| x.panes.iter())
            .find(|p| p.pid == pid)
    });
    let muted = l.t.c(l.t.muted);
    match row.kind {
        SidebarKind::Title => {
            g.text(
                l.tx,
                y,
                l.tw,
                "agents",
                l.t.c(l.t.accent),
                Color::Reset,
                BOLD,
            );
        }
        SidebarKind::SessionOpen => {
            if let Some(gr) = group {
                let label = truncate(&format!("{} {}", l.t.g.group_open, gr.name), l.tw as usize);
                g.text(l.tx, y, l.tw, &label, l.t.c(l.t.accent), Color::Reset, BOLD);
            }
        }
        SidebarKind::SessionClosed => {
            if let Some(gr) = group {
                draw_closed(g, l, y, gr);
            }
        }
        SidebarKind::Pane => {
            if let Some(p) = pane {
                draw_pane(g, l, y, p);
            }
        }
        SidebarKind::Detail => {
            if let Some(m) = pane.and_then(|p| p.message.as_deref()) {
                let room = l.tw.saturating_sub(PANE_PREFIX);
                let m = truncate(m, room as usize);
                g.text(l.tx + PANE_PREFIX, y, room, &m, muted, Color::Reset, 0);
            }
        }
        SidebarKind::Empty(i) => {
            if let Some(line) = wrap(EMPTY_TEXT, l.tw).get(i) {
                let line = truncate(line, l.tw as usize);
                g.text(l.tx, y, l.tw, &line, muted, Color::Reset, 0);
            }
        }
        SidebarKind::More(n) => {
            let label = truncate(&format!("{} +{} more", l.t.g.ellipsis, n), l.tw as usize);
            g.text(l.tx, y, l.tw, &label, muted, Color::Reset, 0);
        }
        SidebarKind::Legend => {
            let text = truncate(&legend_text(l.t, l.tw >= LEGEND_SPACED), l.tw as usize);
            g.text(l.tx, y, l.tw, &text, muted, Color::Reset, 0);
        }
        SidebarKind::Hint => {
            let text = truncate(&hint_text(&l.prefix), l.tw as usize);
            g.text(l.tx, y, l.tw, &text, muted, Color::Reset, 0);
        }
        SidebarKind::Gap => {}
    }
}

/// `▸ name` with right-aligned counts. Counts go first when the name would get < `MIN_NAME`.
fn draw_closed(g: &mut Grid, l: &Lane, y: u16, gr: &GroupView) {
    let t = l.t;
    let pieces: Vec<(String, Color)> = counts(&gr.panes)
        .into_iter()
        .map(|(s, n)| {
            let (glyph, color, _) = t.status(s);
            (format!("{glyph}{n}"), t.c(color))
        })
        .collect();
    let joined = pieces.iter().map(|(s, _)| s.width()).sum::<usize>();
    let cw = (joined + pieces.len().saturating_sub(1)) as u16;
    let name_room = l.tw.saturating_sub(2);
    let show_counts = cw > 0 && name_room >= cw + 1 + MIN_NAME;
    let name_budget = if show_counts {
        name_room - cw - 1
    } else {
        name_room
    };
    let label = format!(
        "{} {}",
        t.g.group_closed,
        truncate(&gr.name, name_budget as usize)
    );
    g.text(l.tx, y, l.tw, &label, Color::Reset, Color::Reset, 0);
    if !show_counts {
        return;
    }
    let end = l.tx + l.tw;
    let mut x = end - cw;
    for (i, (s, color)) in pieces.iter().enumerate() {
        if i > 0 {
            x += 1;
        }
        x += g.text(x, y, end - x, s, *color, Color::Reset, 0);
    }
}

/// `› ▲ name ..... blocked`: pointer and bold mark focus; the state word is muted and is
/// dropped when the name would get < `MIN_NAME` columns beside it.
fn draw_pane(g: &mut Grid, l: &Lane, y: u16, p: &PaneView) {
    let t = l.t;
    let (glyph, color, word) = t.status(p.status);
    let (ptr, ptr_attr) = if p.focused {
        (t.g.pointer, BOLD)
    } else {
        (" ", 0)
    };
    g.text(l.tx, y, 1, ptr, t.c(t.accent), Color::Reset, ptr_attr);
    g.text(l.tx + 2, y, 1, glyph, t.c(color), Color::Reset, 0);
    let room = l.tw.saturating_sub(PANE_PREFIX);
    let word_w = word.width() as u16;
    let show_word = room >= word_w + 1 + MIN_NAME;
    let name_room = if show_word { room - word_w - 1 } else { room };
    if name_room > 0 {
        let name = truncate(&p.name, name_room as usize);
        let attr = if p.focused { BOLD } else { 0 };
        g.text(
            l.tx + PANE_PREFIX,
            y,
            name_room,
            &name,
            Color::Reset,
            Color::Reset,
            attr,
        );
    }
    if show_word {
        let x = l.tx + l.tw - word_w;
        g.text(x, y, word_w, word, t.c(t.muted), Color::Reset, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ASCII;

    fn pv(
        pid: usize,
        status: Status,
        name: &str,
        message: Option<&str>,
        focused: bool,
    ) -> PaneView {
        PaneView {
            pid,
            status,
            name: name.into(),
            message: message.map(String::from),
            focused,
        }
    }

    fn group(si: usize, name: &str, open: bool, panes: Vec<PaneView>) -> GroupView {
        GroupView {
            si,
            name: name.into(),
            open,
            panes,
        }
    }

    fn blank(w: u16, h: u16) -> Grid {
        Grid {
            w,
            h,
            cells: vec![Cell::default(); w as usize * h as usize],
        }
    }

    fn row_text(g: &Grid, y: u16) -> String {
        (0..g.w)
            .map(|x| &g.cells[y as usize * g.w as usize + x as usize])
            .filter(|c| !c.skip)
            .map(|c| c.s.as_str())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// Lay out and draw the groups into a `width` x `height` grid, as `draw_sidebar` does.
    fn draw(groups: &[GroupView], any: bool, hints: bool, width: u16, height: u16) -> Grid {
        let rows = layout_rows(groups, any, hints, width, height);
        let t = Theme::terminal();
        let lane = Lane {
            t: &t,
            tx: 0,
            tw: width,
            prefix: "C-b".into(),
        };
        let mut g = blank(width, height);
        for r in &rows {
            draw_row(&mut g, &lane, r, groups);
        }
        g
    }

    fn y_of(rows: &[SidebarRow], kind: SidebarKind) -> Option<u16> {
        rows.iter().find(|r| r.kind == kind).map(|r| r.y)
    }

    #[test]
    fn focused_row_has_pointer_and_bold() {
        let groups = vec![group(
            0,
            "demo",
            true,
            vec![
                pv(1, Status::Idle, "pi", None, false),
                pv(2, Status::Blocked, "claude", Some("need approval"), true),
            ],
        )];
        let rows = layout_rows(&groups, true, true, 25, 12);
        let g = draw(&groups, true, true, 25, 12);
        let focused = rows
            .iter()
            .find(|r| r.kind == SidebarKind::Pane && r.pane == Some(2))
            .unwrap();
        let plain = rows
            .iter()
            .find(|r| r.kind == SidebarKind::Pane && r.pane == Some(1))
            .unwrap();
        assert!(
            row_text(&g, focused.y).starts_with("› ▲ claude"),
            "{:?}",
            row_text(&g, focused.y)
        );
        assert!(
            row_text(&g, plain.y).starts_with("  ○ pi"),
            "{:?}",
            row_text(&g, plain.y)
        );
        let pointer = &g.cells[focused.y as usize * 25];
        assert_eq!(pointer.attrs & BOLD, BOLD, "focused pointer is bold");
    }

    #[test]
    fn blocked_message_sits_on_indented_detail_line() {
        let groups = vec![group(
            0,
            "demo",
            true,
            vec![pv(
                2,
                Status::Blocked,
                "claude",
                Some("need approval"),
                false,
            )],
        )];
        let rows = layout_rows(&groups, true, true, 25, 12);
        let pane_y = y_of(&rows, SidebarKind::Pane).unwrap();
        let g = draw(&groups, true, true, 25, 12);
        assert_eq!(y_of(&rows, SidebarKind::Detail), Some(pane_y + 1));
        assert_eq!(row_text(&g, pane_y + 1), "    need approval");
    }

    #[test]
    fn legend_fits_a_25_column_slot() {
        let t = Theme::terminal();
        let mut a = Theme::terminal();
        a.g = ASCII;
        assert_eq!(legend_text(&t, false), "●work ▲block ◆done ○idle");
        assert_eq!(legend_text(&t, true), "● work ▲ block ◆ done ○ idle");
        assert!(legend_text(&t, false).width() <= 25);
        assert!(legend_text(&a, false).width() <= 25);
        assert!(legend_text(&t, true).width() <= 28);
    }

    #[test]
    fn overflow_ends_with_more_marker_and_keeps_footer() {
        let panes: Vec<PaneView> = (1..=40)
            .map(|i| pv(i, Status::Idle, &format!("a{i}"), None, false))
            .collect();
        let rows = layout_rows(&[group(0, "demo", true, panes)], true, true, 25, 30);
        let more = rows
            .iter()
            .find(|r| matches!(r.kind, SidebarKind::More(_)))
            .unwrap();
        assert_eq!(more.kind, SidebarKind::More(18));
        assert_eq!(more.y, 25);
        assert_eq!(y_of(&rows, SidebarKind::Legend), Some(27));
        assert_eq!(y_of(&rows, SidebarKind::Hint), Some(28));
        assert!(
            rows.iter().all(|r| r.y <= 25 || r.y >= 27),
            "no row under the footer"
        );
    }

    #[test]
    fn collapsed_session_shows_counts_and_hides_zeros() {
        let groups = vec![
            group(0, "demo", true, vec![pv(1, Status::Idle, "pi", None, true)]),
            group(
                1,
                "build",
                false,
                vec![
                    pv(2, Status::Working, "w1", None, false),
                    pv(3, Status::Working, "w2", None, false),
                    pv(4, Status::Blocked, "b1", Some("x"), false),
                ],
            ),
            group(
                2,
                "idle-only",
                false,
                vec![
                    pv(5, Status::Idle, "i", None, false),
                    pv(6, Status::Idle, "j", None, false),
                ],
            ),
        ];
        assert_eq!(
            counts(&groups[1].panes),
            vec![(Status::Blocked, 1), (Status::Working, 2)]
        );
        assert_eq!(counts(&groups[2].panes), vec![(Status::Idle, 2)]);
        let rows = layout_rows(&groups, true, true, 25, 14);
        let closed: Vec<u16> = rows
            .iter()
            .filter(|r| r.kind == SidebarKind::SessionClosed)
            .map(|r| r.y)
            .collect();
        let g = draw(&groups, true, true, 25, 14);
        let build = row_text(&g, closed[0]);
        assert!(build.starts_with("▸ build"), "{build:?}");
        assert!(build.ends_with("▲1 ●2"), "{build:?}");
        assert_eq!(build.width(), 25);
        assert!(row_text(&g, closed[1]).ends_with("○2"));
    }

    #[test]
    fn state_word_drops_when_name_budget_is_small() {
        let groups = vec![group(
            0,
            "demo",
            true,
            vec![pv(2, Status::Blocked, "claude-long-name", None, false)],
        )];
        let rows = layout_rows(&groups, true, false, 9, 6);
        let g = draw(&groups, true, false, 9, 6);
        let y = y_of(&rows, SidebarKind::Pane).unwrap();
        let text = row_text(&g, y);
        assert!(!text.contains("blocked"), "{text:?}");
        assert!(text.starts_with("  ▲ "), "{text:?}");
        assert!(
            text.ends_with('…'),
            "name is cut with an ellipsis: {text:?}"
        );
        assert!(text.width() <= 9);
    }

    #[test]
    fn empty_state_shows_only_without_agents_and_with_hints() {
        let groups = vec![group(
            0,
            "demo",
            true,
            vec![pv(1, Status::Idle, "sh", None, false)],
        )];
        assert_eq!(
            wrap(EMPTY_TEXT, 25),
            vec![
                "no agent reports — see:",
                "tmuxlike pane",
                "report-agent --help"
            ]
        );
        let count = |rows: &[SidebarRow]| {
            rows.iter()
                .filter(|r| matches!(r.kind, SidebarKind::Empty(_)))
                .count()
        };
        assert_eq!(count(&layout_rows(&groups, false, true, 25, 20)), 3);
        assert_eq!(count(&layout_rows(&groups, false, false, 25, 20)), 0);
        assert_eq!(count(&layout_rows(&groups, true, true, 25, 20)), 0);
    }

    #[test]
    fn footer_is_dropped_before_the_list_is_squeezed() {
        assert_eq!(footer_plan(true, 29), (true, true));
        assert_eq!(footer_plan(true, 4), (true, false));
        assert_eq!(footer_plan(true, 2), (false, false));
        assert_eq!(footer_plan(false, 29), (true, false));
    }

    #[test]
    fn wrap_splits_long_words_and_respects_width() {
        assert_eq!(wrap("abcdefghij kl", 4), vec!["abcd", "efgh", "ij", "kl"]);
        assert!(wrap("x", 0).is_empty());
    }

    #[test]
    fn clean_replaces_control_characters() {
        assert_eq!(clean("a\tb\nc"), "a b c");
    }
}
