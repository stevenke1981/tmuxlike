//! Bottom status bar: session chip, window labels, agent counts, key hint, copy and message
//! segments, and the prompt, confirm or which-key bar that takes the whole row.
//!
//! Geometry is pure ([`status_layout`], [`which_key`], [`prompt_view`]). Drawing paints the
//! segments that layout returns, and the window hit ranges come from the same output, so a
//! click lands on the columns that were drawn.
use super::{BOLD, Cell, Ctx, Grid, INVERSE};
use crate::api::{Status, agent};

use crate::server::{Client, Prompt, PromptKind, Session, State, Window};
use crate::theme::Theme;
use crossterm::style::Color;
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

mod text;
use text::*;
use unicode_width::UnicodeWidthStr;

/// Columns between left-side items and between the parts of the right block.
const GAP: u16 = 2;
/// Columns the left side keeps when the right block is cut to make room.
const MIN_LEFT: u16 = 3;
const PREFIX_CHIP: &str = " PREFIX ";
const COPY_CHIP: &str = " COPY ";
const COPY_HINT: &str = "hjkl move  v/V select  y copy  q quit";
const WHICH_KEY_END: &str = " ? all  Esc cancel";
const CONFIRM_SUFFIX: &str = " (y/n) ";
/// Which-key entries in priority order: (label word, commands whose first bound key is shown).
const WHICH_KEY_GROUPS: &[(&str, &[&str])] = &[
    ("new", &["new-window"]),
    ("win", &["next-window", "previous-window"]),
    ("split", &["split-window -h", "split-window -v"]),
    (
        "move",
        &[
            "select-pane -L",
            "select-pane -D",
            "select-pane -U",
            "select-pane -R",
        ],
    ),
    ("zoom", &["resize-pane -Z"]),
    ("kill", &["kill-pane"]),
    ("alert", &["next-attention"]),
    ("tree", &["choose-tree"]),
    ("copy", &["copy-mode"]),
    ("detach", &["detach-client"]),
];

/// Foreground, background and attributes of one styled run.
type Style = (Color, Color, u8);

/// Semantic role of a run of cells; `tone_style` maps it to tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// bar foreground on the bar background
    Bar,
    /// accent chip: session name, active window
    Chip,
    /// warn chip: PREFIX, COPY, prompt row
    Warn,
    /// blocked-token chip: confirm row
    Danger,
    /// hints and markers
    Muted,
    /// attention label: bold warn text
    Attention,
    /// agent glyph or count in its status color
    Status(Status),
    /// ok message token
    Ok,
    /// error message token (bold)
    Err,
}

/// One drawing instruction: text starting at column `x`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Seg {
    pub x: u16,
    pub text: String,
    pub tone: Tone,
}

/// Result of [`status_layout`].
#[derive(Debug, PartialEq)]
pub(crate) struct StatusLayout {
    pub segs: Vec<Seg>,
    /// clickable window labels: (window index, start col, end col exclusive)
    pub hits: Vec<(usize, u16, u16)>,
    pub regions: StatusRegions,
}

/// Column ranges `[start, end)` of the status row's named blocks; `None` when not drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusRegions {
    /// session chip, e.g. ` demo `
    pub session: Option<(u16, u16)>,
    /// agent counts block, e.g. `▲1 ◆1 ●1`
    pub counts: Option<(u16, u16)>,
}

/// One window as the layout needs it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WinItem {
    pub name: String,
    /// rollup glyph and its status; `None` when every pane is idle
    pub glyph: Option<(&'static str, Status)>,
    pub attention: bool,
    pub zoomed: bool,
}

/// One agent count in the right block (zero counts are not passed in).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Count {
    pub glyph: &'static str,
    pub status: Status,
    pub n: usize,
}

/// Key hint shown after the counts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tail {
    /// `C-b ? help`, cut to `C-b` first when narrow
    Help(String),
    /// `C-b` only (`hints = off`)
    Key(String),
}

/// What the right end of the bar shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Right {
    Bar {
        counts: Vec<Count>,
        tail: Tail,
    },
    /// copy mode: COPY chip and the key hints
    Copy,
    /// transient message chip over the counts block
    Message {
        glyph: &'static str,
        text: String,
        error: bool,
    },
}

/// Everything the normal status row depends on, for one client frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StatusInput {
    pub session: String,
    pub wins: Vec<WinItem>,
    pub active: usize,
    pub ellipsis: &'static str,
    pub right: Right,
}

/// A piece of text with a style role; `win` marks a clickable window label.
#[derive(Clone, Debug, PartialEq)]
struct Unit {
    parts: Vec<(String, Tone)>,
    win: Option<usize>,
}

impl Unit {
    fn new(text: impl Into<String>, tone: Tone) -> Unit {
        Unit {
            parts: vec![(text.into(), tone)],
            win: None,
        }
    }

    fn width(&self) -> u16 {
        self.parts
            .iter()
            .fold(0u16, |a, (t, _)| a.saturating_add(t.width() as u16))
    }
}

/// Right-block alternative; `full` marks the one that still carries the `? help` text.
struct Alt {
    units: Vec<Unit>,
    full: bool,
}

/// Window hit ranges: (window index, start col, end col exclusive).
type HitRanges = Vec<(usize, u16, u16)>;

/// Hit ranges of the last normal status row drawn per session name.
static HITS: Mutex<BTreeMap<String, HitRanges>> = Mutex::new(BTreeMap::new());

/// Window index under column `col` of the status row last drawn for `s`.
///
/// The layout depends on agent state (glyphs, flags, counts), so the ranges are the ones
/// the last frame drew for this session.
pub fn status_hit(s: &Session, col: u16) -> Option<usize> {
    let map = HITS.lock().unwrap_or_else(PoisonError::into_inner);
    hit_at(map.get(&s.name)?, col)
}

fn remember_hits(session: &str, hits: Vec<(usize, u16, u16)>) {
    let mut map = HITS.lock().unwrap_or_else(PoisonError::into_inner);
    map.insert(session.to_string(), hits);
}

fn hit_at(hits: &[(usize, u16, u16)], col: u16) -> Option<usize> {
    hits.iter()
        .find(|(_, a, b)| col >= *a && col < *b)
        .map(|h| h.0)
}

/// Pure status-row layout for `w` columns. Cut order when narrow: the `? help` text, then
/// inactive window names (`…`), then overflow windows (`+N`, `<`), then the session name.
pub(crate) fn status_layout(inp: &StatusInput, w: u16) -> StatusLayout {
    if w == 0 {
        return StatusLayout {
            segs: vec![],
            hits: vec![],
            regions: StatusRegions::default(),
        };
    }
    let name = Some(inp.session.as_str());
    let left_full = chain_w(&left_units(inp, 0, inp.wins.len(), false, name), GAP);
    let (right, gap) = right_block(inp, w, left_full);
    let right_w = chain_w(&right, gap);
    let budget = if right_w > 0 {
        w.saturating_sub(right_w + 1)
    } else {
        w
    };
    let left = fit_left(inp, budget);
    let (mut segs, hits) = place(&left, 0, GAP);
    let rx = w.saturating_sub(right_w);
    let (right_segs, _) = place(&right, rx, gap);
    let regions = StatusRegions {
        session: session_region(&left),
        counts: counts_region(&right, rx),
    };
    segs.extend(right_segs);
    StatusLayout {
        segs,
        hits,
        regions,
    }
}

/// `[0, width)` when the session chip is the first left unit (it is dropped only when the
/// name is too narrow for a chip).
fn session_region(left: &[Unit]) -> Option<(u16, u16)> {
    let first = left.first()?;
    let is_chip = first.win.is_none() && first.parts.first().is_some_and(|(_, t)| *t == Tone::Chip);
    is_chip.then(|| (0, first.width()))
}

/// Start column and width of the counts unit when it leads the right block.
fn counts_region(right: &[Unit], rx: u16) -> Option<(u16, u16)> {
    let first = right.first()?;
    let is_counts = first
        .parts
        .first()
        .is_some_and(|(_, t)| matches!(t, Tone::Status(_)));
    is_counts.then(|| (rx, rx.saturating_add(first.width())))
}

/// Column ranges of the session chip and counts block exactly as the client's frame draws
/// them. Empty while a prompt or the which-key bar owns the row. The mouse code uses these
/// instead of estimating widths.
pub fn status_regions(st: &State, c: &Client, s: &Session) -> StatusRegions {
    if c.prompt.is_some() || c.prefix {
        return StatusRegions::default();
    }
    let theme = Theme::resolve(&st.cfg.theme, &c.env);
    status_layout(&status_input(st, s, c, &theme), s.cols).regions
}

fn chain_w(units: &[Unit], gap: u16) -> u16 {
    let total = units.iter().fold(0u16, |a, u| a.saturating_add(u.width()));
    let gaps = gap.saturating_mul(units.len().saturating_sub(1) as u16);
    total.saturating_add(gaps)
}

fn place(units: &[Unit], x0: u16, gap: u16) -> (Vec<Seg>, Vec<(usize, u16, u16)>) {
    let mut segs = vec![];
    let mut hits = vec![];
    let mut x = x0;
    for (i, u) in units.iter().enumerate() {
        if i > 0 {
            x = x.saturating_add(gap);
        }
        let start = x;
        for (text, tone) in &u.parts {
            segs.push(Seg {
                x,
                text: text.clone(),
                tone: *tone,
            });
            x = x.saturating_add(text.width() as u16);
        }
        if let Some(win) = u.win {
            hits.push((win, start, x));
        }
    }
    (segs, hits)
}

/// Left side: session chip, then windows `lo..hi`, with `<` and `+N` markers for hidden ones.
fn left_units(
    inp: &StatusInput,
    lo: usize,
    hi: usize,
    short: bool,
    chip: Option<&str>,
) -> Vec<Unit> {
    let n = inp.wins.len();
    let mut v = Vec::with_capacity(hi.saturating_sub(lo) + 3);
    if let Some(name) = chip {
        v.push(Unit::new(format!(" {name} "), Tone::Chip));
    }
    if lo > 0 {
        v.push(Unit::new("<", Tone::Muted));
    }
    v.extend((lo..hi).map(|i| window_unit(inp, i, short)));
    if hi < n {
        v.push(Unit::new(format!("+{}", n - hi), Tone::Muted));
    }
    v
}

fn window_unit(inp: &StatusInput, i: usize, short: bool) -> Unit {
    let win = &inp.wins[i];
    let active = i == inp.active;
    let name = if short && !active {
        inp.ellipsis
    } else {
        win.name.as_str()
    };
    let flags: String = [(active, "*"), (win.zoomed, "Z"), (win.attention, "!")]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, f)| *f)
        .collect();
    let tone = if active {
        Tone::Chip
    } else if win.attention {
        Tone::Attention
    } else {
        Tone::Bar
    };
    let mut parts = vec![(format!("{i}:{name}{flags}"), tone)];
    if let Some((glyph, st)) = win.glyph {
        let glyph_tone = if active { Tone::Chip } else { Tone::Status(st) };
        parts.push((glyph.to_string(), glyph_tone));
    }
    Unit {
        parts,
        win: Some(i),
    }
}

/// Pick the left side for `budget` columns, applying the cut order.
fn fit_left(inp: &StatusInput, budget: u16) -> Vec<Unit> {
    let n = inp.wins.len();
    let name = Some(inp.session.as_str());
    if n == 0 {
        return left_units(inp, 0, 0, false, name);
    }
    let full = left_units(inp, 0, n, false, name);
    if chain_w(&full, GAP) <= budget {
        return full;
    }
    let all_short = left_units(inp, 0, n, true, name);
    if chain_w(&all_short, GAP) <= budget {
        return all_short;
    }
    let (lo, hi) = pick_range(inp, budget);
    let ranged = left_units(inp, lo, hi, true, name);
    if chain_w(&ranged, GAP) <= budget {
        return ranged;
    }
    session_cut(inp, lo, hi, budget)
}

/// Widest window range around the active window that fits; grows right before left.
fn pick_range(inp: &StatusInput, budget: u16) -> (usize, usize) {
    let n = inp.wins.len();
    let fits = |lo: usize, hi: usize| {
        let units = left_units(inp, lo, hi, true, Some(inp.session.as_str()));
        chain_w(&units, GAP) <= budget
    };
    let active = inp.active.min(n - 1);
    let mut range = (active, active + 1);
    loop {
        let (lo, hi) = range;
        if hi < n && fits(lo, hi + 1) {
            range.1 += 1;
        } else if lo > 0 && fits(lo - 1, hi) {
            range.0 -= 1;
        } else {
            return range;
        }
    }
}

/// Last cut: shorten the session name in the chip to whatever room is left.
fn session_cut(inp: &StatusInput, lo: usize, hi: usize, budget: u16) -> Vec<Unit> {
    let rest = left_units(inp, lo, hi, true, None);
    let room = budget
        .saturating_sub(chain_w(&rest, GAP))
        .saturating_sub(GAP);
    if room < 3 {
        return rest;
    }
    let chip = format!(" {} ", cut(&inp.session, room - 2, inp.ellipsis));
    let mut v = vec![Unit::new(chip, Tone::Chip)];
    v.extend(rest);
    v
}

fn right_block(inp: &StatusInput, w: u16, left_w: u16) -> (Vec<Unit>, u16) {
    match &inp.right {
        Right::Bar { counts, tail } => bar_right(counts, tail, w, left_w),
        Right::Copy => copy_right(w, left_w, inp.ellipsis),
        Right::Message { glyph, text, error } => {
            message_right(glyph, text, *error, w, left_w, inp.ellipsis)
        }
    }
}

/// True when the left side (`left_w`) and these right units fit side by side in `w`.
fn fits_uncut(left_w: u16, units: &[Unit], gap: u16, w: u16) -> bool {
    let rw = chain_w(units, gap);
    if rw == 0 {
        return left_w <= w;
    }
    left_w.saturating_add(1).saturating_add(rw) <= w
}

/// True when the right units leave room for the minimal left side.
fn cut_ok(units: &[Unit], gap: u16, w: u16) -> bool {
    let rw = chain_w(units, gap);
    rw == 0 || rw.saturating_add(1).saturating_add(MIN_LEFT) <= w
}

/// Right block for the bar. The first alternative that fits with the left side uncut wins;
/// otherwise the left side is cut, keeping the first alternative without the help text.
/// The empty alternative is only used when no key hint fits beside the minimal left side.
fn bar_right(counts: &[Count], tail: &Tail, w: u16, left_w: u16) -> (Vec<Unit>, u16) {
    let alts = bar_alts(counts_unit(counts), tail);
    let shown = || alts.iter().filter(|a| !a.units.is_empty());
    if let Some(a) = shown().find(|a| fits_uncut(left_w, &a.units, GAP, w)) {
        return (a.units.clone(), GAP);
    }
    if let Some(a) = shown().find(|a| !a.full && cut_ok(&a.units, GAP, w)) {
        return (a.units.clone(), GAP);
    }
    (vec![], GAP)
}

/// Right-block alternatives in the order they are dropped: help text, counts, key hint.
fn bar_alts(counts: Option<Unit>, tail: &Tail) -> Vec<Alt> {
    let options: Vec<(String, bool)> = match tail {
        Tail::Help(k) => vec![(format!(" {k} ? help"), true), (format!(" {k}"), false)],
        Tail::Key(k) => vec![(format!(" {k}"), false)],
    };
    let key_only = options.last().map(|(t, _)| t.clone()).unwrap_or_default();
    let mut alts: Vec<Alt> = options
        .into_iter()
        .map(|(t, full)| Alt {
            units: counts
                .clone()
                .into_iter()
                .chain(std::iter::once(Unit::new(t, Tone::Muted)))
                .collect(),
            full,
        })
        .collect();
    alts.push(Alt {
        units: vec![Unit::new(key_only, Tone::Muted)],
        full: false,
    });
    alts.push(Alt {
        units: vec![],
        full: false,
    });
    alts
}

fn counts_unit(counts: &[Count]) -> Option<Unit> {
    if counts.is_empty() {
        return None;
    }
    let mut parts = vec![];
    for (k, c) in counts.iter().enumerate() {
        if k > 0 {
            parts.push((" ".to_string(), Tone::Bar));
        }
        parts.push((format!("{}{}", c.glyph, c.n), Tone::Status(c.status)));
    }
    Some(Unit { parts, win: None })
}

fn copy_right(w: u16, left_w: u16, ell: &str) -> (Vec<Unit>, u16) {
    let chip = Unit::new(COPY_CHIP, Tone::Warn);
    let hint = format!(" {COPY_HINT}");
    let full = vec![chip.clone(), Unit::new(hint.clone(), Tone::Muted)];
    if fits_uncut(left_w, &full, 0, w) {
        return (full, 0);
    }
    let room = w
        .saturating_sub(left_w)
        .saturating_sub(1)
        .saturating_sub(chip.width());
    if room >= 4 {
        return (vec![chip, Unit::new(cut(&hint, room, ell), Tone::Muted)], 0);
    }
    (vec![chip], 0)
}

fn message_right(
    glyph: &str,
    text: &str,
    error: bool,
    w: u16,
    left_w: u16,
    ell: &str,
) -> (Vec<Unit>, u16) {
    let tone = if error { Tone::Err } else { Tone::Ok };
    let full_text = format!(" {glyph} {text}");
    let full = vec![Unit::new(full_text.clone(), tone)];
    if fits_uncut(left_w, &full, 0, w) {
        return (full, 0);
    }
    let cap = (w / 2).max(w.saturating_sub(left_w.saturating_add(1)));
    if cap < 2 {
        return (vec![], 0);
    }
    (vec![Unit::new(cut(&full_text, cap, ell), tone)], 0)
}

fn tone_style(t: &Theme, tone: Tone) -> Style {
    let bar = (t.c(t.bar_fg), t.c(t.bar_bg), 0);
    match tone {
        Tone::Bar => bar,
        Tone::Chip => solid(t, t.accent, BOLD),
        Tone::Warn => solid(t, t.warn, BOLD),
        Tone::Danger => solid(t, t.blocked, BOLD),
        Tone::Muted => (t.c(t.muted), t.c(t.bar_bg), 0),
        Tone::Attention => (t.c(t.warn), t.c(t.bar_bg), BOLD),
        Tone::Status(s) => {
            let (_, color, _) = t.status(s);
            let bold = if s == Status::Blocked { BOLD } else { 0 };
            (t.c(color), t.c(t.bar_bg), bold)
        }
        Tone::Ok => (t.c(t.done), t.c(t.bar_bg), 0),
        Tone::Err => (t.c(t.blocked), t.c(t.bar_bg), BOLD),
    }
}

/// Black on a token color; INVERSE in mono so the chip still stands out.
fn solid(t: &Theme, color: Color, attrs: u8) -> Style {
    if t.mono {
        (Color::Reset, Color::Reset, attrs | INVERSE)
    } else {
        (Color::Black, t.c(color), attrs)
    }
}

fn paint(g: &mut Grid, y: u16, segs: &[Seg], t: &Theme) {
    for seg in segs {
        let (fg, bg, attrs) = tone_style(t, seg.tone);
        let max = g.w.saturating_sub(seg.x);
        g.text(seg.x, y, max, &seg.text, fg, bg, attrs);
    }
}

fn fill_row(g: &mut Grid, y: u16, w: u16, (fg, bg, attrs): Style) {
    for x in 0..w {
        g.put(
            x,
            y,
            Cell {
                fg,
                bg,
                attrs,
                ..Cell::default()
            },
        );
    }
}

/// Draw the status bar on the last row. Returns the cursor position when a command
/// prompt is open (the prompt owns the cursor), otherwise `None`.
pub(super) fn draw_status(g: &mut Grid, cx: &Ctx) -> Option<(u16, u16)> {
    let sy = cx.h.saturating_sub(1);
    let t = &cx.theme;
    if let Some(p) = &cx.c.prompt {
        remember_hits(&cx.s.name, Vec::new());
        return draw_prompt(g, cx, sy, p);
    }
    fill_row(g, sy, cx.w, tone_style(t, Tone::Bar));
    if cx.c.prefix {
        remember_hits(&cx.s.name, Vec::new());
        let segs = [
            Seg {
                x: 0,
                text: which_key(&cx.st.bindings, cx.w),
                tone: Tone::Bar,
            },
            Seg {
                x: 0,
                text: PREFIX_CHIP.to_string(),
                tone: Tone::Warn,
            },
        ];
        paint(g, sy, &segs, t);
        return None;
    }
    let layout = status_layout(&status_input(cx.st, cx.s, cx.c, &cx.theme), cx.w);
    paint(g, sy, &layout.segs, t);
    remember_hits(&cx.s.name, layout.hits);
    None
}

fn draw_prompt(g: &mut Grid, cx: &Ctx, sy: u16, p: &Prompt) -> Option<(u16, u16)> {
    let t = &cx.theme;
    let ell = t.g.ellipsis;
    if p.kind == PromptKind::Confirm {
        let text = confirm_text(&p.buf, cx.w, ell);
        let style = tone_style(t, Tone::Danger);
        fill_row(g, sy, cx.w, style);
        g.text(0, sy, cx.w, &text, style.0, style.1, style.2);
        return None;
    }
    let label = match p.kind {
        PromptKind::RenameWindow => "(rename-window) ",
        PromptKind::RenameSession => "(rename-session) ",
        _ => ":",
    };
    let (text, col) = prompt_view(label, &p.buf, p.cur, cx.w, ell);
    let style = tone_style(t, Tone::Warn);
    fill_row(g, sy, cx.w, style);
    g.text(0, sy, cx.w, &text, style.0, style.1, style.2);
    Some((col, sy))
}

/// Everything the normal status row shows, read from the client's state.
fn status_input(st: &State, s: &Session, c: &Client, theme: &Theme) -> StatusInput {
    let wins = s
        .windows
        .iter()
        .map(|win| WinItem {
            name: win.name.clone(),
            glyph: window_rollup(st, win).map(|r| (theme.status(r).0, r)),
            attention: win
                .panes()
                .iter()
                .any(|id| st.panes.get(id).is_some_and(|p| p.attention)),
            zoomed: win.zoomed,
        })
        .collect();
    StatusInput {
        session: s.name.clone(),
        wins,
        active: s.active,
        ellipsis: theme.g.ellipsis,
        right: right_of(st, s, c, theme),
    }
}

fn right_of(st: &State, s: &Session, c: &Client, theme: &Theme) -> Right {
    if let Some(m) = &c.message {
        let glyph = if m.error { theme.g.err } else { theme.g.ok };
        return Right::Message {
            glyph,
            text: m.text.clone(),
            error: m.error,
        };
    }
    if in_copy_mode(st, s) {
        return Right::Copy;
    }
    let key = st.cfg.prefix.clone();
    let tail = if st.cfg.hints {
        Tail::Help(key)
    } else {
        Tail::Key(key)
    };
    Right::Bar {
        counts: agent_counts(st, theme),
        tail,
    }
}

fn in_copy_mode(st: &State, s: &Session) -> bool {
    s.windows
        .get(s.active)
        .and_then(|w| st.panes.get(&w.active))
        .is_some_and(|p| p.copy.is_some())
}

/// Blocked, done and working counts over every session; zero counts are omitted.
fn agent_counts(st: &State, theme: &Theme) -> Vec<Count> {
    let mut n = [0usize; 3];
    for sess in &st.sessions {
        for win in &sess.windows {
            for id in win.panes() {
                let Some(p) = st.panes.get(&id) else {
                    continue;
                };
                if let Some(slot) = count_slot(agent::status(p, &st.cfg)) {
                    n[slot] += 1;
                }
            }
        }
    }
    [Status::Blocked, Status::Done, Status::Working]
        .into_iter()
        .zip(n)
        .filter(|(_, n)| *n > 0)
        .map(|(status, n)| Count {
            glyph: theme.status(status).0,
            status,
            n,
        })
        .collect()
}

fn count_slot(s: Status) -> Option<usize> {
    match s {
        Status::Blocked => Some(0),
        Status::Done => Some(1),
        Status::Working => Some(2),
        _ => None,
    }
}

/// Highest-precedence status among the window's panes; `None` when all are idle.
fn window_rollup(st: &State, win: &Window) -> Option<Status> {
    win.panes()
        .iter()
        .filter_map(|id| st.panes.get(id))
        .map(|p| agent::status(p, &st.cfg))
        .min_by_key(|s| rollup_rank(*s))
        .filter(|s| *s != Status::Idle)
}

fn rollup_rank(s: Status) -> u8 {
    match s {
        Status::Blocked => 0,
        Status::Done => 1,
        Status::Working => 2,
        Status::Unknown => 3,
        Status::Idle => 4,
    }
}

#[cfg(test)]
mod tests;
