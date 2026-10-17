//! Modal overlays drawn over the panes: the session/window/pane chooser and the key popup.
//!
//! The chooser is a tree (session > window > pane). Filtering, tree art, labels and kill
//! targets are pure functions over a [`NodeSession`] snapshot, so they are unit-tested without
//! ptys. `Client.chooser` stores the selected row and the filter packed in one `usize`
//! (see [`pack_chooser`]), so the server needs no new field.
use super::{BOLD, Cell, Ctx, Grid, INVERSE};
use crate::api::Status;
use crate::server::{Pane, State};
use crate::theme::{ASCII, Glyphs, Theme};
use crossterm::style::Color;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Chooser row: (session index, window index, pane index). `None` marks a level above the row.
pub type Entry = (usize, Option<usize>, Option<usize>);

/// Which panes the chooser shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Only(Status),
}

impl Filter {
    /// Filter keys: `b` blocked, `w` working, `d` done, `i` idle, `a` all.
    pub fn from_key(c: char) -> Option<Filter> {
        Some(match c {
            'a' => Filter::All,
            'b' => Filter::Only(Status::Blocked),
            'w' => Filter::Only(Status::Working),
            'd' => Filter::Only(Status::Done),
            'i' => Filter::Only(Status::Idle),
            _ => return None,
        })
    }

    fn code(self) -> usize {
        match self {
            Filter::All => 0,
            Filter::Only(Status::Blocked) => 1,
            Filter::Only(Status::Working) => 2,
            Filter::Only(Status::Done) => 3,
            Filter::Only(Status::Idle) => 4,
            Filter::Only(Status::Unknown) => 5,
        }
    }

    fn from_code(n: usize) -> Filter {
        match n {
            1 => Filter::Only(Status::Blocked),
            2 => Filter::Only(Status::Working),
            3 => Filter::Only(Status::Done),
            4 => Filter::Only(Status::Idle),
            5 => Filter::Only(Status::Unknown),
            _ => Filter::All,
        }
    }
}

const SEL_BITS: u32 = 16;
const SEL_MASK: usize = (1 << SEL_BITS) - 1;

/// `Client.chooser` = selected row (low 16 bits) | filter code (bits 16..). A plain selection
/// (code 0) therefore opens with `Filter::All`, which is what `ChooseTree` sets.
pub fn pack_chooser(sel: usize, filter: Filter) -> usize {
    sel.min(SEL_MASK) | (filter.code() << SEL_BITS)
}

/// Inverse of [`pack_chooser`]: (selected row, filter).
pub fn unpack_chooser(raw: usize) -> (usize, Filter) {
    (raw & SEL_MASK, Filter::from_code(raw >> SEL_BITS))
}

/// Plain snapshot of one session for the chooser (no pty handles).
#[derive(Clone, Debug, PartialEq)]
struct NodeSession {
    name: String,
    attached: bool,
    windows: Vec<NodeWindow>,
}

#[derive(Clone, Debug, PartialEq)]
struct NodeWindow {
    name: String,
    active: bool,
    panes: Vec<NodePane>,
}

#[derive(Clone, Debug, PartialEq)]
struct NodePane {
    status: Status,
    /// agent name, else label, else window name
    who: String,
}

fn snapshot(st: &State, attached: &str) -> Vec<NodeSession> {
    st.sessions
        .iter()
        .map(|s| NodeSession {
            name: s.name.clone(),
            attached: s.name == attached,
            windows: s
                .windows
                .iter()
                .enumerate()
                .map(|(wi, w)| NodeWindow {
                    name: w.name.clone(),
                    active: wi == s.active,
                    panes: w
                        .panes()
                        .into_iter()
                        .map(|pid| match st.panes.get(&pid) {
                            Some(p) => NodePane {
                                status: crate::api::agent::status(p, &st.cfg),
                                who: pane_who(p, &w.name),
                            },
                            None => NodePane {
                                status: Status::Unknown,
                                who: w.name.clone(),
                            },
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

fn pane_who(p: &Pane, window: &str) -> String {
    p.agent
        .as_ref()
        .and_then(|a| a.name.as_deref().or(Some(a.agent.as_str())))
        .or(p.label.as_deref())
        .unwrap_or(window)
        .to_string()
}

/// Every chooser row, unfiltered, in tree order: session, its windows, each window's panes.
pub fn chooser_entries(st: &State) -> Vec<Entry> {
    entries_of(&snapshot(st, ""))
}

/// The rows the chooser shows for `filter`; rendering and key handling both use this list.
pub fn visible_entries(st: &State, filter: Filter) -> Vec<Entry> {
    filter_entries(&snapshot(st, ""), filter)
}

fn entries_of(tree: &[NodeSession]) -> Vec<Entry> {
    let mut v = vec![];
    for (si, s) in tree.iter().enumerate() {
        v.push((si, None, None));
        for (wi, w) in s.windows.iter().enumerate() {
            v.push((si, Some(wi), None));
            for pi in 0..w.panes.len() {
                v.push((si, Some(wi), Some(pi)));
            }
        }
    }
    v
}

/// Rollup precedence: blocked > done > working > unknown > idle (ui-spec §2).
fn rank(s: Status) -> u8 {
    match s {
        Status::Blocked => 4,
        Status::Done => 3,
        Status::Working => 2,
        Status::Unknown => 1,
        Status::Idle => 0,
    }
}

fn rollup(statuses: impl Iterator<Item = Status>) -> Status {
    statuses.max_by_key(|s| rank(*s)).unwrap_or(Status::Idle)
}

/// Status shown on a row: a pane's own state, or the rollup of the panes below it.
fn status_of(tree: &[NodeSession], e: Entry) -> Status {
    let Some(s) = tree.get(e.0) else {
        return Status::Unknown;
    };
    match (e.1, e.2) {
        (None, _) => rollup(
            s.windows
                .iter()
                .flat_map(|w| w.panes.iter().map(|p| p.status)),
        ),
        (Some(w), None) => s.windows.get(w).map_or(Status::Unknown, |w| {
            rollup(w.panes.iter().map(|p| p.status))
        }),
        (Some(w), Some(p)) => s
            .windows
            .get(w)
            .and_then(|w| w.panes.get(p))
            .map_or(Status::Unknown, |p| p.status),
    }
}

/// Keep the panes in `filter`, plus each window and session that holds one of them.
fn filter_entries(tree: &[NodeSession], filter: Filter) -> Vec<Entry> {
    let all = entries_of(tree);
    let Filter::Only(want) = filter else {
        return all;
    };
    let panes: Vec<Entry> = all
        .iter()
        .copied()
        .filter(|e| e.2.is_some() && status_of(tree, *e) == want)
        .collect();
    all.into_iter()
        .filter(|e| match (e.1, e.2) {
            (_, Some(_)) => panes.contains(e),
            (Some(w), None) => panes.iter().any(|p| p.0 == e.0 && p.1 == Some(w)),
            (None, _) => panes.iter().any(|p| p.0 == e.0),
        })
        .collect()
}

/// One chooser line before drawing.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    status: Status,
    prefix: String,
    text: String,
    bold: bool,
}

fn chooser_rows(tree: &[NodeSession], filter: Filter, g: &Glyphs) -> Vec<Row> {
    let entries = filter_entries(tree, filter);
    (0..entries.len())
        .map(|i| {
            let (prefix, text, bold) = row_art(tree, &entries, i, g);
            Row {
                status: status_of(tree, entries[i]),
                prefix,
                text,
                bold,
            }
        })
        .collect()
}

/// Tree art, label and boldness for `entries[i]`. Connectors are computed over the visible
/// rows, so the last visible child gets the `tree_last` corner.
fn row_art(
    tree: &[NodeSession],
    entries: &[Entry],
    i: usize,
    g: &Glyphs,
) -> (String, String, bool) {
    let (si, wi, pi) = entries[i];
    let s = &tree[si];
    match (wi, pi) {
        (None, _) => (
            String::new(),
            session_text(&s.name, s.windows.len(), s.attached),
            true,
        ),
        (Some(w), None) => {
            let win = &s.windows[w];
            let branch = if sibling_follows(entries, i) {
                g.tree_mid
            } else {
                g.tree_last
            };
            (
                format!("{branch} "),
                window_text(w, &win.name, win.active, win.panes.len()),
                false,
            )
        }
        (Some(w), Some(p)) => {
            let parent_last = entries[..i]
                .iter()
                .rposition(|e| e.0 == si && e.1 == Some(w) && e.2.is_none())
                .is_none_or(|j| !sibling_follows(entries, j));
            let cont = if parent_last {
                "  ".to_string()
            } else {
                format!("{} ", g.vline)
            };
            let branch = if pane_follows(entries, i) {
                g.tree_mid
            } else {
                g.tree_last
            };
            let pane = &s.windows[w].panes[p];
            (
                format!("{cont}{branch} "),
                pane_text(w, p, &pane.who, pane.status),
                false,
            )
        }
    }
}

/// A later window of the same session exists in `entries`.
fn sibling_follows(entries: &[Entry], i: usize) -> bool {
    let e = entries[i];
    for n in &entries[i + 1..] {
        if n.0 != e.0 {
            return false;
        }
        if n.1.is_some() && n.2.is_none() {
            return true;
        }
    }
    false
}

/// A later pane of the same window exists in `entries`.
fn pane_follows(entries: &[Entry], i: usize) -> bool {
    let e = entries[i];
    for n in &entries[i + 1..] {
        if n.0 != e.0 || n.1 != e.1 {
            return false;
        }
        if n.2.is_some() {
            return true;
        }
    }
    false
}

/// `1 window` / `2 windows`.
fn count_noun(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn session_text(name: &str, windows: usize, attached: bool) -> String {
    format!(
        "{name}: {}{}",
        count_noun(windows, "window", "windows"),
        if attached { " (attached)" } else { "" }
    )
}

fn window_text(index: usize, name: &str, active: bool, panes: usize) -> String {
    format!(
        "{index}: {name}{}  {}",
        if active { "*" } else { "" },
        count_noun(panes, "pane", "panes")
    )
}

fn pane_text(window: usize, pane: usize, who: &str, status: Status) -> String {
    format!("{window}.{pane} {who} {}", status_word(status))
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Working => "working",
        Status::Blocked => "blocked",
        Status::Done => "done",
        Status::Idle => "idle",
        Status::Unknown => "unknown",
    }
}

/// What `x` on a chooser row kills, and the y/n question that names it.
#[derive(Debug, Clone, PartialEq)]
pub struct KillTarget {
    pub question: String,
    /// command line, run on `y` (or at once when `confirm-close` is off)
    pub command: String,
}

/// Kill target for a session or window row. Pane rows have none (pane kill is `C-b x`).
pub fn kill_target(st: &State, e: Entry) -> Option<KillTarget> {
    kill_target_in(&snapshot(st, ""), e)
}

fn blocked_count<'a>(panes: impl Iterator<Item = &'a NodePane>) -> usize {
    panes.filter(|p| p.status == Status::Blocked).count()
}

fn kill_target_in(tree: &[NodeSession], e: Entry) -> Option<KillTarget> {
    let s = tree.get(e.0)?;
    match (e.1, e.2) {
        (_, Some(_)) => None,
        (None, _) => {
            let n = blocked_count(s.windows.iter().flat_map(|w| w.panes.iter()));
            Some(KillTarget {
                question: format!(
                    "kill session {} ({}, {n} blocked)?",
                    s.name,
                    count_noun(s.windows.len(), "window", "windows")
                ),
                command: format!("kill-session -t {}", arg_quote(&s.name)),
            })
        }
        (Some(w), None) => {
            let win = s.windows.get(w)?;
            let n = blocked_count(win.panes.iter());
            Some(KillTarget {
                question: format!(
                    "kill window {w} {} ({}, {n} blocked)?",
                    win.name,
                    count_noun(win.panes.len(), "pane", "panes")
                ),
                command: format!("kill-window -t {}", arg_quote(&format!("{}:{w}", s.name))),
            })
        }
    }
}

/// One command argument for `commands::tokenize`: always double-quoted, `\` and `"` escaped.
fn arg_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One drawable line inside an overlay box.
struct Line {
    /// tree art, dim
    pre: String,
    /// state glyph and its color, drawn before the text
    glyph: Option<Status>,
    text: String,
    bold: bool,
    muted: bool,
}

impl Line {
    fn plain(text: &str) -> Line {
        Line {
            pre: String::new(),
            glyph: None,
            text: text.to_string(),
            bold: false,
            muted: false,
        }
    }

    fn note(text: &str) -> Line {
        Line {
            muted: true,
            ..Line::plain(text)
        }
    }

    fn from_row(r: &Row) -> Line {
        Line {
            pre: r.prefix.clone(),
            glyph: Some(r.status),
            text: r.text.clone(),
            bold: r.bold,
            muted: false,
        }
    }

    fn width(&self) -> usize {
        self.pre.width() + if self.glyph.is_some() { 2 } else { 0 } + self.text.width()
    }
}

/// Box geometry in cells.
#[derive(Clone, Copy)]
struct Geom {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

struct BoxSpec<'a> {
    title: String,
    footer: &'a str,
    lines: &'a [Line],
    sel: Option<usize>,
    scroll: usize,
}

const POPUP_HINT: &str = "j/k scroll  esc close";
const POPUP_TITLE: &str = "keys";

/// Chooser footer; the ASCII set spells out the arrows and Enter.
fn chooser_hints(g: &Glyphs) -> &'static str {
    if *g == ASCII {
        "up/dn move  enter open  x kill  b/w/d/i filter  esc close"
    } else {
        "↑↓ move  ⏎ open  x kill  b/w/d/i filter  esc close"
    }
}

fn chooser_title(filter: Filter) -> String {
    match filter {
        Filter::All => "Sessions".to_string(),
        Filter::Only(s) => format!("Sessions [{}]", status_word(s)),
    }
}

fn empty_note(filter: Filter) -> String {
    match filter {
        Filter::All => "no sessions".to_string(),
        Filter::Only(s) => format!("no {} panes", status_word(s)),
    }
}

/// Cut `s` to `max` display columns, ending with `ell` when shortened. Never splits a wide char.
fn cut(s: &str, max: usize, ell: &str) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max.saturating_sub(ell.width());
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > budget {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push_str(ell);
    out
}

/// Selected-row style: selection color with the bar foreground, or inverse video in mono.
fn sel_style(t: &Theme) -> (Color, Color, u8) {
    if t.mono {
        (Color::Reset, Color::Reset, INVERSE)
    } else {
        (t.bar_bg, t.selection, 0)
    }
}

/// Scroll markers for the top border: `▲ more` when lines are above, `▼ more` when below.
/// Both sit on the top border so the bottom border keeps its hints.
fn scroll_marker(cx: &Ctx, up: bool, down: bool) -> Option<String> {
    let ascii = cx.theme.g == ASCII;
    let mut parts = vec![];
    if up {
        parts.push(if ascii { "^ more" } else { "▲ more" });
    }
    if down {
        parts.push(if ascii { "v more" } else { "▼ more" });
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// First visible line. A selection stays visible and roughly centred; without one, `scroll` is
/// clamped to the list.
fn window_start(len: usize, inner: usize, sel: Option<usize>, scroll: usize) -> usize {
    let max_start = len.saturating_sub(inner);
    match sel {
        Some(s) => s.saturating_sub(inner / 2).min(max_start),
        None => scroll.min(max_start),
    }
}

/// Draw a bordered box: title and scroll markers on the top border, footer hints on the bottom
/// border, one line per row.
fn draw_box(g: &mut Grid, cx: &Ctx, spec: &BoxSpec) {
    let (w, h) = (cx.w, cx.h);
    let want = spec
        .lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(spec.title.width() + 2)
        .max(spec.footer.width() + 2);
    let bw = (want + 4).min(usize::from(w.saturating_sub(2))).max(10) as u16;
    let maxh = usize::from(h.saturating_sub(3).max(3));
    let bh = (spec.lines.len().max(1) + 2).min(maxh) as u16;
    let geom = Geom {
        x: w.saturating_sub(bw) / 2,
        y: h.saturating_sub(1).saturating_sub(bh) / 2,
        w: bw,
        h: bh,
    };
    let inner_h = usize::from(bh - 2);
    let inner_w = usize::from(bw - 4);
    let start = window_start(spec.lines.len(), inner_h, spec.sel, spec.scroll);
    let up = start > 0;
    let down = start + inner_h < spec.lines.len();

    draw_frame(g, cx, geom);
    border_label(
        g,
        cx,
        geom,
        geom.y,
        &spec.title,
        cx.theme.c(cx.theme.accent),
        BOLD,
        scroll_marker(cx, up, down).as_deref(),
    );
    border_label(
        g,
        cx,
        geom,
        geom.y + geom.h - 1,
        spec.footer,
        cx.theme.c(cx.theme.muted),
        0,
        None,
    );
    for (k, line) in spec.lines.iter().enumerate().skip(start).take(inner_h) {
        let y = geom.y + 1 + (k - start) as u16;
        draw_row(g, cx, geom.x + 2, y, inner_w, line, spec.sel == Some(k));
    }
}

fn draw_frame(g: &mut Grid, cx: &Ctx, b: Geom) {
    let (tl, tr, bl, br) = if cx.theme.g == ASCII {
        ("+", "+", "+", "+")
    } else {
        ("┌", "┐", "└", "┘")
    };
    let color = cx.theme.c(cx.theme.accent);
    for yy in 0..b.h {
        for xx in 0..b.w {
            let (top, bottom) = (yy == 0, yy == b.h - 1);
            let (left, right) = (xx == 0, xx == b.w - 1);
            let s = match (top, bottom, left, right) {
                (true, _, true, _) => tl,
                (true, _, _, true) => tr,
                (_, true, true, _) => bl,
                (_, true, _, true) => br,
                (true, _, _, _) | (_, true, _, _) => cx.theme.g.hline,
                (_, _, true, _) | (_, _, _, true) => cx.theme.g.vline,
                _ => " ",
            };
            g.put(
                b.x + xx,
                b.y + yy,
                Cell {
                    s: s.into(),
                    fg: color,
                    ..Cell::default()
                },
            );
        }
    }
}

/// Text on a border row as ` text `, with an optional right-aligned marker. Never overlaps it.
#[allow(clippy::too_many_arguments)]
fn border_label(
    g: &mut Grid,
    cx: &Ctx,
    b: Geom,
    y: u16,
    text: &str,
    fg: Color,
    attr: u8,
    marker: Option<&str>,
) {
    let mut room = usize::from(b.w.saturating_sub(4));
    if let Some(m) = marker {
        let mw = m.width() + 2;
        room = room.saturating_sub(mw);
        let mx = (b.x + b.w).saturating_sub(2 + mw as u16);
        let shown = format!(" {m} ");
        g.text(
            mx,
            y,
            mw as u16,
            &shown,
            cx.theme.c(cx.theme.muted),
            Color::Reset,
            0,
        );
    }
    if room < 3 {
        return;
    }
    let label = format!(" {} ", cut(text, room - 2, cx.theme.g.ellipsis));
    g.text(b.x + 2, y, room as u16, &label, fg, Color::Reset, attr);
}

/// One content row: tree art, glyph, text, cut to `inner_w`. A selected row fills the full
/// width (inverse video in mono).
fn draw_row(g: &mut Grid, cx: &Ctx, x0: u16, y: u16, inner_w: usize, line: &Line, selected: bool) {
    let t = &cx.theme;
    let (base_fg, bg, base_attr) = if selected {
        sel_style(t)
    } else {
        (Color::Reset, Color::Reset, 0)
    };
    let end = x0 + inner_w as u16;
    if selected {
        for xx in x0.saturating_sub(1)..=end {
            g.put(
                xx,
                y,
                Cell {
                    s: " ".into(),
                    fg: base_fg,
                    bg,
                    attrs: base_attr,
                    ..Cell::default()
                },
            );
        }
    }
    let mut segs: Vec<(String, Color, u8)> = Vec::new();
    let mut used = 0;
    if !line.pre.is_empty() {
        used += line.pre.width();
        let fg = if selected { base_fg } else { t.c(t.border) };
        segs.push((line.pre.clone(), fg, base_attr));
    }
    if let Some(status) = line.glyph {
        let (glyph, color, _) = t.status(status);
        used += 2;
        let fg = if selected { base_fg } else { t.c(color) };
        let attr = base_attr | if status == Status::Blocked { BOLD } else { 0 };
        segs.push((glyph.to_string(), fg, attr));
        segs.push((" ".to_string(), base_fg, base_attr));
    }
    let text_fg = if line.muted && !selected {
        t.c(t.muted)
    } else {
        base_fg
    };
    let text = cut(&line.text, inner_w.saturating_sub(used), t.g.ellipsis);
    segs.push((text, text_fg, base_attr | if line.bold { BOLD } else { 0 }));
    let mut x = x0;
    for (s, fg, attr) in segs {
        let room = end.saturating_sub(x);
        x += g.text(x, y, room, &s, fg, bg, attr);
    }
}

/// Draw the chooser or key popup if open. Returns `true` when an overlay took the cursor,
/// so the caller hides it.
pub(super) fn draw_overlays(g: &mut Grid, cx: &Ctx) -> bool {
    let c = cx.c;
    if let Some(raw) = c.chooser {
        draw_chooser(g, cx, raw);
        true
    } else if let Some((text, scroll)) = &c.popup {
        let lines: Vec<Line> = text.iter().map(|l| Line::plain(l)).collect();
        let spec = BoxSpec {
            title: POPUP_TITLE.to_string(),
            footer: POPUP_HINT,
            lines: &lines,
            sel: None,
            scroll: *scroll,
        };
        draw_box(g, cx, &spec);
        true
    } else {
        false
    }
}

fn draw_chooser(g: &mut Grid, cx: &Ctx, raw: usize) {
    let (sel, filter) = unpack_chooser(raw);
    let tree = snapshot(cx.st, &cx.s.name);
    let mut lines: Vec<Line> = chooser_rows(&tree, filter, &cx.theme.g)
        .iter()
        .map(Line::from_row)
        .collect();
    let sel = if lines.is_empty() {
        None
    } else {
        Some(sel.min(lines.len() - 1))
    };
    if lines.is_empty() {
        lines.push(Line::note(&empty_note(filter)));
    }
    let spec = BoxSpec {
        title: chooser_title(filter),
        footer: chooser_hints(&cx.theme.g),
        lines: &lines,
        sel,
        scroll: 0,
    };
    draw_box(g, cx, &spec);
}

#[cfg(test)]
mod tests;
