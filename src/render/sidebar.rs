//! Right-hand sidebar: agents grouped by session, a legend and a key hint.
//!
//! Layout is pure: [`sidebar_rows`] decides what each line shows. The drawer and the mouse
//! handler both use it, so a click always lands on the line that was drawn.
use super::overlay::status_word;
use super::{BOLD, Cell, Ctx, DIM, Grid, cut};
use crate::api::Status;
use crate::api::agent::status as agent_status;
use crate::i18n::{Lang, tr, trf};
use crate::server::{Session, State};
use crate::theme::{Theme, UNICODE};
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
const LEGEND: [(Status, &str); 4] = [
    (Status::Working, "render.legend.working"),
    (Status::Blocked, "render.legend.blocked"),
    (Status::Done, "render.legend.done"),
    (Status::Idle, "render.legend.idle"),
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
    /// `… +N more`: `N` lines (rows) below are hidden
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

/// [`sidebar_rows`] for a client whose UI language is `lang`.
pub fn sidebar_rows_lang(
    st: &State,
    s: &Session,
    width: u16,
    height: u16,
    lang: Lang,
) -> Vec<SidebarRow> {
    let groups = group_views(st, s);
    layout_rows(&groups, any_agent(st), st.cfg.hints, width, height, lang)
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

/// Pure line layout: title, list (see [`fit_list`]), then the footer.
fn layout_rows(
    groups: &[GroupView],
    any_agent: bool,
    hints: bool,
    width: u16,
    height: u16,
    lang: Lang,
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
    let list = fit_list(groups, any_agent, hints, width, room, lang);
    for (i, mut it) in list.into_iter().enumerate() {
        it.y = top + i as u16;
        rows.push(it);
    }
    push_footer(&mut rows, avail, legend, hint);
    rows
}

/// Rows of the list that fit in `room` lines, marker included, with one blank line between
/// group blocks. On overflow the empty-state hint goes first, then closed groups nearest the
/// attached one, then the attached group's panes far from the focused pane (see [`shed`]).
fn fit_list(
    groups: &[GroupView],
    any_agent: bool,
    hints: bool,
    width: u16,
    room: usize,
    lang: Lang,
) -> Vec<SidebarRow> {
    if room == 0 {
        return vec![];
    }
    let flat = list_items(groups, any_agent, hints, width, lang);
    let all = vec![true; flat.len()];
    if lines(&flat, &all) <= room {
        return assemble(&flat, &all);
    }
    let attached = groups.iter().find(|g| g.open);
    let focus = attached
        .and_then(|g| g.panes.iter().find(|p| p.focused))
        .map(|p| p.pid);
    let keep = shed(&flat, attached.map(|g| g.si), focus, room - 1);
    assemble(&flat, &keep)
}

/// Lines that the rows marked in `keep` take: one per row, plus one blank line before every
/// group block after the first.
fn lines(flat: &[SidebarRow], keep: &[bool]) -> usize {
    let mut rows = 0;
    let mut groups = 0;
    for (r, &k) in flat.iter().zip(keep) {
        if k {
            rows += 1;
            groups += usize::from(is_group_start(r));
        }
    }
    rows + groups.saturating_sub(1)
}

fn is_group_start(r: &SidebarRow) -> bool {
    matches!(
        r.kind,
        SidebarKind::SessionOpen | SidebarKind::SessionClosed
    )
}

/// The kept rows in order, a blank line before each group block after the first, and
/// `… +N more` counting the rows that were dropped.
fn assemble(flat: &[SidebarRow], keep: &[bool]) -> Vec<SidebarRow> {
    let mut out = vec![];
    let mut seen_group = false;
    for (r, &k) in flat.iter().zip(keep) {
        if !k {
            continue;
        }
        if is_group_start(r) {
            if seen_group {
                out.push(item(SidebarKind::Gap, None, None));
            }
            seen_group = true;
        }
        out.push(*r);
    }
    let hidden = keep.iter().filter(|k| !**k).count();
    if hidden > 0 {
        out.push(item(SidebarKind::More(hidden), None, None));
    }
    out
}

/// Keep mask for an overflowing list; `budget` is the line count for everything but the
/// marker. The hint rows start dropped. Closed groups are kept nearest the attached one
/// first. When the attached group alone does not fit, it is windowed around the focus.
fn shed(
    flat: &[SidebarRow],
    open_si: Option<usize>,
    focus: Option<usize>,
    budget: usize,
) -> Vec<bool> {
    let mut keep: Vec<bool> = flat
        .iter()
        .map(|r| r.session.is_some() && r.session == open_si)
        .collect();
    if lines(flat, &keep) > budget {
        return window_open(flat, &keep, focus, budget);
    }
    let center = open_si.unwrap_or(0);
    let mut closed: Vec<usize> = flat
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == SidebarKind::SessionClosed)
        .map(|(i, _)| i)
        .collect();
    closed.sort_by_key(|&i| (flat[i].session.unwrap_or(center).abs_diff(center), i));
    for i in closed {
        keep[i] = true;
        if lines(flat, &keep) > budget {
            keep[i] = false;
        }
    }
    keep
}

/// Keeps the attached header plus a run of pane units around the focused pane, as many as
/// `budget` lines allow. A unit is a pane row with the detail line under it; it is never split,
/// except that the focused pane may lose its detail line.
fn window_open(
    flat: &[SidebarRow],
    attached: &[bool],
    focus: Option<usize>,
    budget: usize,
) -> Vec<bool> {
    let mut keep = vec![false; flat.len()];
    let rows: Vec<usize> = (0..flat.len()).filter(|&i| attached[i]).collect();
    let Some((&header, body)) = rows.split_first() else {
        return keep;
    };
    if !try_keep(flat, &mut keep, &[header], budget) {
        return keep;
    }
    let units = pane_units(flat, body);
    if units.is_empty() {
        return keep;
    }
    let f = units
        .iter()
        .position(|u| focus.is_some() && flat[u[0]].pane == focus)
        .unwrap_or(0);
    let focus_fits = try_keep(flat, &mut keep, &units[f], budget)
        || try_keep(flat, &mut keep, &units[f][..1], budget);
    if !focus_fits {
        return keep;
    }
    let (mut lo, mut hi) = (f, f + 1);
    let mut grew = true;
    while grew {
        grew = false;
        if lo > 0 && try_keep(flat, &mut keep, &units[lo - 1], budget) {
            lo -= 1;
            grew = true;
        }
        if hi < units.len() && try_keep(flat, &mut keep, &units[hi], budget) {
            hi += 1;
            grew = true;
        }
    }
    keep
}

/// Pane rows with the detail line that follows each, one unit per pane.
fn pane_units(flat: &[SidebarRow], rows: &[usize]) -> Vec<Vec<usize>> {
    let mut units: Vec<Vec<usize>> = vec![];
    for &i in rows {
        let is_detail = flat[i].kind == SidebarKind::Detail;
        match units.last_mut() {
            Some(u) if is_detail => u.push(i),
            _ => units.push(vec![i]),
        }
    }
    units
}

/// Marks `rows` kept when the list still fits `budget` lines; otherwise `keep` is unchanged.
fn try_keep(flat: &[SidebarRow], keep: &mut [bool], rows: &[usize], budget: usize) -> bool {
    for &i in rows {
        keep[i] = true;
    }
    if lines(flat, keep) <= budget {
        return true;
    }
    for &i in rows {
        keep[i] = false;
    }
    false
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

/// Every row above the footer, in draw order, without blank spacers (see [`fit_list`]).
fn list_items(
    groups: &[GroupView],
    any_agent: bool,
    hints: bool,
    width: u16,
    lang: Lang,
) -> Vec<SidebarRow> {
    let mut items = vec![];
    if hints && !any_agent {
        let n = empty_lines(width, UNICODE.dash, lang)
            .len()
            .min(EMPTY_LINES);
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
    }
    items
}

/// Empty-state hint. `dash` is `theme.g.dash` (an em dash, or `-` in ASCII mode). Both are one
/// column wide, so the wrapped line count does not depend on the glyph set.
fn empty_lines(width: u16, dash: &str, lang: Lang) -> Vec<String> {
    wrap(
        &trf(lang, "render.sidebar.empty", &[("dash", &dash)]),
        width,
    )
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
fn legend_text(t: &Theme, spaced: bool, lang: Lang) -> String {
    LEGEND
        .iter()
        .map(|(s, word_key)| {
            let glyph = t.status(*s).0;
            let word = tr(lang, word_key);
            if spaced {
                format!("{glyph} {word}")
            } else {
                format!("{glyph}{word}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn hint_text(prefix: &str, lang: Lang) -> String {
    trf(lang, "render.sidebar.hint", &[("prefix", &prefix)])
}

/// Attribute for muted chrome: faint in mono mode (no color to tell it apart), plain otherwise.
fn dim_if_mono(t: &Theme) -> u8 {
    if t.mono { DIM } else { 0 }
}

/// Where the sidebar draws: text origin and width, plus the theme and the prefix key.
struct Lane<'a> {
    t: &'a Theme,
    tx: u16,
    tw: u16,
    prefix: String,
    lang: Lang,
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
        lang: cx.lang,
    };
    for row in layout_rows(&groups, any_agent(st), st.cfg.hints, tw, h, cx.lang) {
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
    let dim = dim_if_mono(l.t);
    let ell = l.t.g.ellipsis;
    let tw = l.tw as usize;
    match row.kind {
        SidebarKind::Title => {
            g.text(
                l.tx,
                y,
                l.tw,
                tr(l.lang, "render.sidebar.title"),
                l.t.c(l.t.accent),
                Color::Reset,
                BOLD,
            );
        }
        SidebarKind::SessionOpen => {
            if let Some(gr) = group {
                let label = cut(&format!("{} {}", l.t.g.group_open, gr.name), tw, ell);
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
                let m = cut(m, room as usize, ell);
                g.text(l.tx + PANE_PREFIX, y, room, &m, muted, Color::Reset, dim);
            }
        }
        SidebarKind::Empty(i) => {
            if let Some(line) = empty_lines(l.tw, l.t.g.dash, l.lang).get(i) {
                let line = cut(line, tw, ell);
                g.text(l.tx, y, l.tw, &line, muted, Color::Reset, dim);
            }
        }
        SidebarKind::More(n) => {
            let more = trf(l.lang, "render.sidebar.more", &[("ell", &ell), ("n", &n)]);
            let label = cut(&more, tw, ell);
            g.text(l.tx, y, l.tw, &label, muted, Color::Reset, dim);
        }
        SidebarKind::Legend => {
            let text = cut(&legend_text(l.t, l.tw >= LEGEND_SPACED, l.lang), tw, ell);
            g.text(l.tx, y, l.tw, &text, muted, Color::Reset, dim);
        }
        SidebarKind::Hint => {
            let text = cut(&hint_text(&l.prefix, l.lang), tw, ell);
            g.text(l.tx, y, l.tw, &text, muted, Color::Reset, dim);
        }
        SidebarKind::Gap => {}
    }
}

/// `▸ name` with right-aligned counts. Counts go first when the name would get < `MIN_NAME`.
fn draw_closed(g: &mut Grid, l: &Lane, y: u16, gr: &GroupView) {
    let t = l.t;
    let dim = dim_if_mono(t);
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
    let name = cut(&gr.name, name_budget as usize, t.g.ellipsis);
    let label = format!("{} {name}", t.g.group_closed);
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
        x += g.text(x, y, end - x, s, *color, Color::Reset, dim);
    }
}

/// `› ▲ name ..... blocked`: pointer and bold mark focus; the state word is muted and is
/// dropped when the name would get < `MIN_NAME` columns beside it.
fn draw_pane(g: &mut Grid, l: &Lane, y: u16, p: &PaneView) {
    let t = l.t;
    let (glyph, color, _) = t.status(p.status);
    let word = status_word(l.lang, p.status);
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
        let name = cut(&p.name, name_room as usize, t.g.ellipsis);
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
        g.text(
            x,
            y,
            word_w,
            word,
            t.c(t.muted),
            Color::Reset,
            dim_if_mono(t),
        );
    }
}

#[cfg(test)]
mod tests;
