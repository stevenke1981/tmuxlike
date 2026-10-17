//! Modal overlays drawn over the panes: the session/window/pane chooser and the key popup.
//!
//! The chooser is a tree (session > window > pane). Filtering, tree art, labels and kill
//! targets are pure functions over a [`NodeSession`] snapshot, so they are unit-tested without
//! ptys. `Client.chooser` stores the selected row and the filter packed in one `usize`
//! (see [`pack_chooser`]), so the server needs no new field.
use super::{BOLD, Cell, Ctx, DIM, Grid, INVERSE, cut};
use crate::api::Status;
use crate::i18n::{Lang, tr, trf};
use crate::server::{Pane, State};
use crate::theme::{ASCII, Glyphs, Theme};
use crossterm::style::Color;
use unicode_width::UnicodeWidthStr;

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

/// One chooser line before drawing. `word` is the state word of a pane row, drawn muted after
/// the text.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    status: Status,
    prefix: String,
    text: String,
    word: Option<&'static str>,
    bold: bool,
}

fn chooser_rows(tree: &[NodeSession], filter: Filter, g: &Glyphs, lang: Lang) -> Vec<Row> {
    let entries = filter_entries(tree, filter);
    (0..entries.len())
        .map(|i| {
            let (prefix, text, bold) = row_art(tree, &entries, i, g, lang);
            let status = status_of(tree, entries[i]);
            let word = entries[i].2.map(|_| status_word(lang, status));
            Row {
                status,
                prefix,
                text,
                word,
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
    lang: Lang,
) -> (String, String, bool) {
    let (si, wi, pi) = entries[i];
    let s = &tree[si];
    match (wi, pi) {
        (None, _) => (
            String::new(),
            session_text(lang, &s.name, s.windows.len(), s.attached),
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
                window_text(lang, w, &win.name, win.active, win.panes.len()),
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
                pane_text(w, p, &pane.who),
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

/// Noun counted in a row label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Noun {
    Window,
    Pane,
}

/// `1 window` / `2 windows`, `1 個視窗` (zh-TW has no plural; the English count picks the key).
fn count_text(lang: Lang, n: usize, noun: Noun) -> String {
    let key = match (noun, n == 1) {
        (Noun::Window, true) => "render.count.window",
        (Noun::Window, false) => "render.count.windows",
        (Noun::Pane, true) => "render.count.pane",
        (Noun::Pane, false) => "render.count.panes",
    };
    trf(lang, key, &[("n", &n)])
}

/// `work: 2 windows (attached)`. The name is the last argument so it is never re-substituted.
fn session_text(lang: Lang, name: &str, windows: usize, attached: bool) -> String {
    let count = count_text(lang, windows, Noun::Window);
    let attached = if attached {
        tr(lang, "render.chooser.attached")
    } else {
        ""
    };
    trf(
        lang,
        "render.session.text",
        &[("count", &count), ("attached", &attached), ("name", &name)],
    )
}

/// `0: cmd*  2 panes`. The name is the last argument so it is never re-substituted.
fn window_text(lang: Lang, index: usize, name: &str, active: bool, panes: usize) -> String {
    let count = count_text(lang, panes, Noun::Pane);
    let star = if active { "*" } else { "" };
    trf(
        lang,
        "render.window.text",
        &[
            ("index", &index),
            ("star", &star),
            ("count", &count),
            ("name", &name),
        ],
    )
}

/// Pane row text without its state word (that is drawn muted by [`Row::word`]).
fn pane_text(window: usize, pane: usize, who: &str) -> String {
    format!("{window}.{pane} {who}")
}

/// State word for a status (`working`, `執行中`); shared with the sidebar and pane titles.
pub(super) fn status_word(lang: Lang, s: Status) -> &'static str {
    let key = match s {
        Status::Working => "render.state.working",
        Status::Blocked => "render.state.blocked",
        Status::Done => "render.state.done",
        Status::Idle => "render.state.idle",
        Status::Unknown => "render.state.unknown",
    };
    tr(lang, key)
}

/// What `x` on a chooser row kills, and the y/n question that names it.
#[derive(Debug, Clone, PartialEq)]
pub struct KillTarget {
    pub question: String,
    /// command line, run on `y` (or at once when `confirm-close` is off)
    pub command: String,
}

/// Kill target for a session or window row, in the server's default language. Pane rows have
/// none (pane kill is `C-b x`). Clients in another language use [`kill_target_lang`].
pub fn kill_target(st: &State, e: Entry) -> Option<KillTarget> {
    kill_target_lang(st, e, Lang::resolve(&st.cfg.lang, None))
}

/// [`kill_target`] with the question written in `lang`.
pub fn kill_target_lang(st: &State, e: Entry, lang: Lang) -> Option<KillTarget> {
    kill_target_in(&snapshot(st, ""), e, lang)
}

fn blocked_count<'a>(panes: impl Iterator<Item = &'a NodePane>) -> usize {
    panes.filter(|p| p.status == Status::Blocked).count()
}

fn kill_target_in(tree: &[NodeSession], e: Entry, lang: Lang) -> Option<KillTarget> {
    let s = tree.get(e.0)?;
    match (e.1, e.2) {
        (_, Some(_)) => None,
        (None, _) => {
            let n = blocked_count(s.windows.iter().flat_map(|w| w.panes.iter()));
            let windows = count_text(lang, s.windows.len(), Noun::Window);
            Some(KillTarget {
                question: trf(
                    lang,
                    "render.kill.session",
                    &[("windows", &windows), ("blocked", &n), ("name", &s.name)],
                ),
                command: format!("kill-session -t {}", arg_quote(&s.name)),
            })
        }
        (Some(w), None) => {
            let win = s.windows.get(w)?;
            let n = blocked_count(win.panes.iter());
            let panes = count_text(lang, win.panes.len(), Noun::Pane);
            Some(KillTarget {
                question: trf(
                    lang,
                    "render.kill.window",
                    &[
                        ("index", &w),
                        ("panes", &panes),
                        ("blocked", &n),
                        ("name", &win.name),
                    ],
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
    /// tree art, muted (faint in mono)
    pre: String,
    /// state glyph and its color, drawn before the text
    glyph: Option<Status>,
    text: String,
    /// state word after the text, muted
    word: Option<&'static str>,
    bold: bool,
    muted: bool,
}

impl Line {
    fn plain(text: &str) -> Line {
        Line {
            pre: String::new(),
            glyph: None,
            text: text.to_string(),
            word: None,
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
            word: r.word,
            bold: r.bold,
            muted: false,
        }
    }

    fn width(&self) -> usize {
        self.pre.width()
            + if self.glyph.is_some() { 2 } else { 0 }
            + self.text.width()
            + self.word.map_or(0, |w| 1 + w.width())
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

/// Chooser footer; the ASCII set spells out the arrows and Enter.
fn chooser_hints(g: &Glyphs, lang: Lang) -> &'static str {
    if *g == ASCII {
        tr(lang, "render.chooser.hints_ascii")
    } else {
        tr(lang, "render.chooser.hints")
    }
}

fn chooser_title(lang: Lang, filter: Filter) -> String {
    match filter {
        Filter::All => tr(lang, "render.chooser.title").to_string(),
        Filter::Only(s) => trf(
            lang,
            "render.chooser.title_filter",
            &[("state", &status_word(lang, s))],
        ),
    }
}

fn empty_note(lang: Lang, filter: Filter) -> String {
    match filter {
        Filter::All => tr(lang, "render.chooser.empty_all").to_string(),
        Filter::Only(s) => trf(
            lang,
            "render.chooser.empty_filter",
            &[("state", &status_word(lang, s))],
        ),
    }
}

/// Muted chrome (hints, footers, state words, notes): the `muted` color, or SGR 2 in mono.
/// Returns (fg, attributes).
fn muted_style(t: &Theme) -> (Color, u8) {
    (t.c(t.muted), if t.mono { DIM } else { 0 })
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
        let key = if ascii {
            "render.scroll.up_ascii"
        } else {
            "render.scroll.up"
        };
        parts.push(tr(cx.lang, key));
    }
    if down {
        let key = if ascii {
            "render.scroll.down_ascii"
        } else {
            "render.scroll.down"
        };
        parts.push(tr(cx.lang, key));
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
    let (muted_fg, muted_attr) = muted_style(&cx.theme);
    border_label(
        g,
        cx,
        geom,
        geom.y + geom.h - 1,
        spec.footer,
        muted_fg,
        muted_attr,
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
        let (muted_fg, muted_attr) = muted_style(&cx.theme);
        g.text(mx, y, mw as u16, &shown, muted_fg, Color::Reset, muted_attr);
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
    let (muted_fg, muted_attr) = muted_style(t);
    let mut segs: Vec<(String, Color, u8)> = Vec::new();
    let mut used = 0;
    if !line.pre.is_empty() {
        used += line.pre.width();
        let (fg, attr) = if selected {
            (base_fg, base_attr)
        } else {
            (t.c(t.border), base_attr | muted_attr)
        };
        segs.push((line.pre.clone(), fg, attr));
    }
    if let Some(status) = line.glyph {
        let (glyph, color, _) = t.status(status);
        used += 2;
        let fg = if selected { base_fg } else { t.c(color) };
        let attr = base_attr | if status == Status::Blocked { BOLD } else { 0 };
        segs.push((glyph.to_string(), fg, attr));
        segs.push((" ".to_string(), base_fg, base_attr));
    }
    let word = line.word.map(|w| format!(" {w}")).unwrap_or_default();
    let (text_fg, text_muted) = if line.muted && !selected {
        (muted_fg, muted_attr)
    } else {
        (base_fg, 0)
    };
    let bold = if line.bold { BOLD } else { 0 };
    let text = cut(
        &line.text,
        inner_w.saturating_sub(used + word.width()),
        t.g.ellipsis,
    );
    segs.push((text, text_fg, base_attr | text_muted | bold));
    if !word.is_empty() {
        let (fg, attr) = if selected {
            (base_fg, base_attr)
        } else {
            (muted_fg, base_attr | muted_attr)
        };
        segs.push((word, fg, attr));
    }
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
            title: tr(cx.lang, "render.popup.title").to_string(),
            footer: tr(cx.lang, "render.popup.hint"),
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
    let lang = cx.lang;
    let mut lines: Vec<Line> = chooser_rows(&tree, filter, &cx.theme.g, lang)
        .iter()
        .map(Line::from_row)
        .collect();
    let sel = if lines.is_empty() {
        None
    } else {
        Some(sel.min(lines.len() - 1))
    };
    if lines.is_empty() {
        lines.push(Line::note(&empty_note(lang, filter)));
    }
    let spec = BoxSpec {
        title: chooser_title(lang, filter),
        footer: chooser_hints(&cx.theme.g, lang),
        lines: &lines,
        sel,
        scroll: 0,
    };
    draw_box(g, cx, &spec);
}

#[cfg(test)]
mod tests;
