//! Pane, session and snapshot handlers. OWNER: worker H2.
//!
//! Handlers lock `shared` themselves. Anything that waits (`PaneWaitOutput`, the Enter delay in
//! `submit`) releases the lock between polls, so the server keeps rendering and reading.
use super::{
    ApiError, ApiResult, Direction, Pattern, ReadSource, Reply, Request, ZoomMode, pane_id_str,
    pane_json,
};
use crate::keys::encode;
use crate::layout::{Dir, Move, Node, neighbor};
use crate::server::{Session, Shared, State, Window};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use regex::Regex;
use serde_json::{Value, json};
use std::sync::{MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Rows returned by `PaneRead` with `Recent` when no count is given.
const RECENT_ROWS: usize = 80;
/// Delay between pasting text and sending Enter in `submit` (`PaneRun`).
const RUN_ENTER_DELAY_MS: u64 = 30;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const LABEL_MAX_CHARS: usize = 80;

pub fn handle(shared: &Shared, req: Request) -> ApiResult {
    match req {
        Request::ApiSnapshot
        | Request::SessionList
        | Request::PaneList { .. }
        | Request::PaneGet { .. }
        | Request::PaneCurrent { .. } => query(shared, req),
        Request::PaneSplit { .. }
        | Request::PaneFocus { .. }
        | Request::PaneFocusDirection { .. }
        | Request::PaneZoom { .. }
        | Request::PaneRename { .. }
        | Request::PaneClose { .. } => layout_op(shared, req),
        Request::PaneRead { .. }
        | Request::PaneSendText { .. }
        | Request::PaneSendKeys { .. }
        | Request::PaneRun { .. }
        | Request::PaneWaitOutput { .. } => io_op(shared, req),
        other => Err(unexpected(&other)),
    }
}

fn unexpected(req: &Request) -> ApiError {
    ApiError::new(
        "internal",
        format!("pane handler got an unexpected request: {req:?}"),
    )
}

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

fn sent() -> Reply {
    Reply::Json(json!({"sent": true}))
}

fn ensure_pane(st: &State, pid: usize) -> Result<(), ApiError> {
    if st.panes.contains_key(&pid) {
        Ok(())
    } else {
        Err(ApiError::pane_not_found(pid))
    }
}

/// Resolve an optional request pane. `None` = focused pane of the most recently used session.
pub fn resolve_pane(st: &State, pane: Option<usize>) -> Result<usize, ApiError> {
    match pane {
        Some(pid) => ensure_pane(st, pid).map(|()| pid),
        None => {
            let si = st
                .target_session(None)
                .map_err(|_| ApiError::new("session_not_found", "no sessions"))?;
            Ok(st.active_pane(si))
        }
    }
}

fn pane_reply(st: &State, pid: usize) -> ApiResult {
    let pane = pane_json(st, pid).ok_or_else(|| ApiError::pane_not_found(pid))?;
    Ok(Reply::Json(json!({ "pane": pane })))
}

fn session_missing(name: &str) -> ApiError {
    ApiError::new("session_not_found", format!("session not found: {name}"))
}

// ---------------------------------------------------------------------------
// queries: snapshot, sessions, panes
// ---------------------------------------------------------------------------

fn query(shared: &Shared, req: Request) -> ApiResult {
    let st = lock(shared);
    match req {
        Request::ApiSnapshot => Ok(Reply::Json(snapshot(&st))),
        Request::SessionList => Ok(Reply::Json(session_list(&st))),
        Request::PaneList { session } => pane_list(&st, session.as_deref()),
        Request::PaneGet { pane } => pane_reply(&st, pane),
        Request::PaneCurrent { pane } => {
            let pid = resolve_pane(&st, pane)?;
            pane_reply(&st, pid)
        }
        other => Err(unexpected(&other)),
    }
}

fn attached_count(st: &State, session: &str) -> usize {
    st.clients.values().filter(|c| c.session == session).count()
}

fn panes_json(st: &State, ids: &[usize]) -> Vec<Value> {
    ids.iter().filter_map(|&pid| pane_json(st, pid)).collect()
}

fn session_pane_ids(s: &Session) -> Vec<usize> {
    s.windows.iter().flat_map(Window::panes).collect()
}

fn snapshot(st: &State) -> Value {
    let sessions: Vec<Value> = st
        .sessions
        .iter()
        .map(|s| session_snapshot(st, s))
        .collect();
    json!({"version": env!("CARGO_PKG_VERSION"), "sessions": sessions})
}

fn session_snapshot(st: &State, s: &Session) -> Value {
    let windows: Vec<Value> = s
        .windows
        .iter()
        .enumerate()
        .map(|(wi, w)| window_snapshot(st, wi, w))
        .collect();
    json!({
        "name": s.name,
        "cols": s.cols,
        "rows": s.rows,
        "attached": attached_count(st, &s.name),
        "active_window": s.active,
        "windows": windows,
    })
}

fn window_snapshot(st: &State, wi: usize, w: &Window) -> Value {
    json!({
        "index": wi,
        "name": w.name,
        "active_pane": pane_id_str(w.active),
        "zoomed": w.zoomed,
        "panes": panes_json(st, &w.panes()),
        "layout": tree_json(&w.layout),
    })
}

/// Layout tree for snapshots: leaves are `{"pane": "pN"}`, splits carry direction and ratio.
fn tree_json(node: &Node) -> Value {
    match node {
        Node::Leaf(id) => json!({ "pane": pane_id_str(*id) }),
        Node::Split { dir, ratio, a, b } => json!({
            "split": dir_name(*dir),
            "ratio": round_ratio(*ratio),
            "a": tree_json(a),
            "b": tree_json(b),
        }),
    }
}

fn dir_name(dir: Dir) -> &'static str {
    match dir {
        Dir::H => "right",
        Dir::V => "down",
    }
}

fn round_ratio(ratio: f32) -> f64 {
    (f64::from(ratio) * 10_000.0).round() / 10_000.0
}

fn session_list(st: &State) -> Value {
    let sessions: Vec<Value> = st
        .sessions
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "windows": s.windows.len(),
                "attached": attached_count(st, &s.name),
                "cols": s.cols,
                "rows": s.rows,
            })
        })
        .collect();
    json!({ "sessions": sessions })
}

fn pane_list(st: &State, session: Option<&str>) -> ApiResult {
    let ids: Vec<usize> = match session {
        Some(name) => {
            let si = st.sidx(name).ok_or_else(|| session_missing(name))?;
            session_pane_ids(&st.sessions[si])
        }
        None => st.sessions.iter().flat_map(session_pane_ids).collect(),
    };
    Ok(Reply::Json(json!({ "panes": panes_json(st, &ids) })))
}

// ---------------------------------------------------------------------------
// layout: split, focus, zoom, rename, close
// ---------------------------------------------------------------------------

fn layout_op(shared: &Shared, req: Request) -> ApiResult {
    match req {
        Request::PaneSplit {
            pane,
            direction,
            ratio,
            cwd,
            focus,
        } => split(shared, pane, direction, ratio, cwd, focus),
        Request::PaneFocus { pane } => focus(shared, pane),
        Request::PaneFocusDirection { pane, direction } => focus_direction(shared, pane, direction),
        Request::PaneZoom { pane, mode } => zoom(shared, pane, mode),
        Request::PaneRename { pane, label } => rename(shared, pane, label),
        Request::PaneClose { pane } => close(shared, pane),
        other => Err(unexpected(&other)),
    }
}

/// herdr splits go right or down only; the new pane is always the second child.
fn split_dir(direction: Direction) -> Result<Dir, ApiError> {
    match direction {
        Direction::Right => Ok(Dir::H),
        Direction::Down => Ok(Dir::V),
        Direction::Left | Direction::Up => {
            Err(ApiError::invalid("split direction must be right or down"))
        }
    }
}

/// herdr `--ratio` is the share of the NEW pane; `split_pane` takes the share of the existing one.
fn existing_share(ratio: Option<f32>) -> Result<Option<f32>, ApiError> {
    match ratio {
        None => Ok(None),
        Some(r) if r > 0.0 && r < 1.0 => Ok(Some(1.0 - r)),
        Some(r) => Err(ApiError::invalid(format!(
            "ratio must be between 0 and 1 exclusive, got {r}"
        ))),
    }
}

fn split(
    shared: &Shared,
    pane: Option<usize>,
    direction: Direction,
    ratio: Option<f32>,
    cwd: Option<String>,
    focus: bool,
) -> ApiResult {
    let dir = split_dir(direction)?;
    let share = existing_share(ratio)?;
    let mut st = lock(shared);
    let pid = resolve_pane(&st, pane)?;
    let new_id = st
        .split_pane(shared, pid, dir, share, cwd, focus)
        .map_err(|e| ApiError::new("spawn_failed", format!("{e:#}")))?;
    st.mark_dirty();
    pane_reply(&st, new_id)
}

fn focus(shared: &Shared, pane: usize) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    st.focus_pane(pid);
    st.mark_dirty();
    pane_reply(&st, pid)
}

fn move_of(direction: Direction) -> Move {
    match direction {
        Direction::Left => Move::Left,
        Direction::Right => Move::Right,
        Direction::Up => Move::Up,
        Direction::Down => Move::Down,
    }
}

fn neighbor_pane(st: &State, pid: usize, mv: Move) -> Result<usize, ApiError> {
    let (si, wi) = st
        .locate(pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    let s = &st.sessions[si];
    let rects = st.visible_rects(s, &s.windows[wi]);
    neighbor(&rects, pid, mv).ok_or_else(|| {
        ApiError::new(
            "no_neighbor",
            format!("no pane {mv:?} of {}", pane_id_str(pid)),
        )
    })
}

fn focus_direction(shared: &Shared, pane: Option<usize>, direction: Direction) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, pane)?;
    let target = neighbor_pane(&st, pid, move_of(direction))?;
    st.focus_pane(target);
    st.mark_dirty();
    pane_reply(&st, target)
}

/// Apply a zoom request to one window. Zooming a single-pane window is a no-op flag-wise.
fn apply_zoom(w: &mut Window, pid: usize, mode: ZoomMode) {
    let on = match mode {
        ZoomMode::On => true,
        ZoomMode::Off => false,
        ZoomMode::Toggle => !w.zoomed,
    };
    if !on {
        w.zoomed = false;
        return;
    }
    w.active = pid;
    w.zoomed = w.panes().len() > 1;
}

fn zoom(shared: &Shared, pane: Option<usize>, mode: ZoomMode) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, pane)?;
    let (si, wi) = st
        .locate(pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    apply_zoom(&mut st.sessions[si].windows[wi], pid, mode);
    st.relayout(si);
    st.mark_dirty();
    pane_reply(&st, pid)
}

/// Trim, cap length, and treat empty as "no label".
fn normalize_label(label: Option<String>) -> Option<String> {
    let trimmed: String = label?.trim().chars().take(LABEL_MAX_CHARS).collect();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn rename(shared: &Shared, pane: usize, label: Option<String>) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    let p = st
        .panes
        .get_mut(&pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    p.label = normalize_label(label);
    st.mark_dirty();
    pane_reply(&st, pid)
}

fn close(shared: &Shared, pane: usize) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    st.kill_pane(pid);
    st.mark_dirty();
    Ok(Reply::Json(json!({ "closed": pane_id_str(pid) })))
}

// ---------------------------------------------------------------------------
// terminal I/O: read, send text/keys, run, wait
// ---------------------------------------------------------------------------

fn io_op(shared: &Shared, req: Request) -> ApiResult {
    match req {
        Request::PaneRead {
            pane,
            source,
            lines,
        } => read_pane(shared, pane, source, lines),
        Request::PaneSendText { pane, text } => send_text(shared, pane, &text),
        Request::PaneSendKeys { pane, keys } => send_keys_op(shared, pane, &keys),
        Request::PaneRun { pane, command } => {
            submit(shared, pane, &command, RUN_ENTER_DELAY_MS)?;
            Ok(sent())
        }
        Request::PaneWaitOutput {
            pane,
            pattern,
            source,
            lines,
            timeout_ms,
        } => wait_output(shared, pane, &pattern, source, lines, timeout_ms),
        other => Err(unexpected(&other)),
    }
}

fn read_pane(shared: &Shared, pane: usize, source: ReadSource, lines: Option<usize>) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    read_text(&mut st, pid, source, lines).map(Reply::Text)
}

fn send_text(shared: &Shared, pane: usize, text: &str) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    st.write_pane(pid, text.as_bytes());
    st.mark_dirty();
    Ok(sent())
}

fn send_keys_op(shared: &Shared, pane: usize, keys: &[String]) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    send_keys(&mut st, pid, keys)?;
    st.mark_dirty();
    Ok(sent())
}

/// Text of a pane as lines joined by `\n`, with a trailing newline when non-empty.
/// Lines are trimmed on the right, trailing blank lines are dropped, then `keep` keeps the last N.
fn finish_text(rows: impl IntoIterator<Item = String>, keep: Option<usize>) -> String {
    let mut lines: Vec<String> = rows.into_iter().map(|r| r.trim_end().to_string()).collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if let Some(n) = keep {
        let skip = lines.len().saturating_sub(n);
        lines.drain(..skip);
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The last `want` rows across scrollback and screen, oldest first, as raw rows (untrimmed).
/// Scrollback is restored to its previous offset, so an active copy-mode view is undisturbed.
/// Fetches `h` extra rows so that trailing blank screen rows do not eat into `want`.
fn recent_rows(screen: &mut vt100::Screen, want: usize) -> Vec<String> {
    let (rows, width) = screen.size();
    let h = usize::from(rows);
    if h == 0 {
        return Vec::new();
    }
    let saved = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let total = history + h;
    let mut p = total.saturating_sub(want.saturating_add(h));
    let mut out = Vec::new();
    while p < history {
        // at offset `history - p` the first rows on screen are absolute lines p, p+1, ...
        let take = (history - p).min(h);
        screen.set_scrollback(history - p);
        out.extend(screen.rows(0, width).take(take));
        p += take;
    }
    screen.set_scrollback(0);
    out.extend(screen.rows(0, width).skip(p - history));
    screen.set_scrollback(saved);
    out
}

/// Read a pane's text. Visible = the screen as currently displayed (same view as `capture-pane`).
/// Recent = last N rows (default 80) across scrollback and screen, regardless of the view offset.
pub fn read_text(
    st: &mut State,
    pid: usize,
    source: ReadSource,
    lines: Option<usize>,
) -> Result<String, ApiError> {
    let p = st
        .panes
        .get_mut(&pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    let screen = p.parser.screen_mut();
    Ok(match source {
        ReadSource::Visible => {
            let width = screen.size().1;
            finish_text(screen.rows(0, width), lines)
        }
        ReadSource::Recent => {
            let want = lines.unwrap_or(RECENT_ROWS);
            finish_text(recent_rows(screen, want), Some(want))
        }
    })
}

/// Map a key spec (herdr syntax or legacy tmux names) to the bytes a pane should receive.
pub fn key_bytes(spec: &str, app_cursor: bool) -> Result<Vec<u8>, String> {
    let unknown = || format!("unknown key: {spec}");
    let (mods, base) = split_modifiers(spec);
    let (code, mods) = base_key(base, mods).ok_or_else(unknown)?;
    let ctrl_unmapped = matches!(
        code,
        KeyCode::Char(c) if mods.contains(KeyModifiers::CONTROL) && !ctrl_maps(c)
    );
    if ctrl_unmapped {
        return Err(unknown());
    }
    let bytes = encode(&KeyEvent::new(code, mods), app_cursor);
    if bytes.is_empty() {
        return Err(unknown());
    }
    Ok(bytes)
}

const MODIFIER_PREFIXES: &[(&str, KeyModifiers)] = &[
    ("ctrl+", KeyModifiers::CONTROL),
    ("control+", KeyModifiers::CONTROL),
    ("alt+", KeyModifiers::ALT),
    ("shift+", KeyModifiers::SHIFT),
    ("c-", KeyModifiers::CONTROL),
    ("m-", KeyModifiers::ALT),
    ("s-", KeyModifiers::SHIFT),
    ("^", KeyModifiers::CONTROL),
];

/// Strip any stacked modifier prefixes (case-insensitive). A prefix is only taken when
/// something follows it, so a lone "-" or "^" stays a plain key.
fn split_modifiers(spec: &str) -> (KeyModifiers, &str) {
    let mut mods = KeyModifiers::NONE;
    let mut rest = spec;
    loop {
        let lower = rest.to_ascii_lowercase();
        let hit = MODIFIER_PREFIXES
            .iter()
            .find(|(p, _)| lower.starts_with(p) && rest.len() > p.len());
        match hit {
            Some((p, m)) => {
                mods |= *m;
                rest = &rest[p.len()..];
            }
            None => return (mods, rest),
        }
    }
}

/// Characters that `encode` turns into control bytes when Ctrl is held.
fn ctrl_maps(c: char) -> bool {
    matches!(
        c.to_ascii_lowercase(),
        'a'..='z'
            | '@'
            | ' '
            | '2'
            | '['
            | '3'
            | '\\'
            | '4'
            | ']'
            | '5'
            | '^'
            | '6'
            | '_'
            | '7'
            | '/'
            | '-'
            | '8'
            | '?'
    )
}

fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "btab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" | "bspace" => KeyCode::Backspace,
        "space" => KeyCode::Char(' '),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" | "ppage" => KeyCode::PageUp,
        "pagedown" | "pgdn" | "npage" => KeyCode::PageDown,
        "insert" | "ic" => KeyCode::Insert,
        "delete" | "dc" => KeyCode::Delete,
        "minus" => KeyCode::Char('-'),
        "plus" => KeyCode::Char('+'),
        "backtick" => KeyCode::Char('`'),
        "equals" => KeyCode::Char('='),
        "comma" => KeyCode::Char(','),
        "period" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "backslash" => KeyCode::Char('\\'),
        "semicolon" => KeyCode::Char(';'),
        "quote" => KeyCode::Char('\''),
        "lbracket" => KeyCode::Char('['),
        "rbracket" => KeyCode::Char(']'),
        f => return function_key(f),
    })
}

fn function_key(name: &str) -> Option<KeyCode> {
    let n: u8 = name.strip_prefix('f')?.parse().ok()?;
    (1..=12).contains(&n).then_some(KeyCode::F(n))
}

/// Resolve the base key name (after modifiers) to a code, folding Shift into the code where
/// `encode` would otherwise lose it.
fn base_key(base: &str, mods: KeyModifiers) -> Option<(KeyCode, KeyModifiers)> {
    let mut chars = base.chars();
    let code = match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() => KeyCode::Char(c),
        _ => named_key(&base.to_ascii_lowercase())?,
    };
    Some(match code {
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
            (KeyCode::BackTab, mods - KeyModifiers::SHIFT)
        }
        KeyCode::Char(c) if mods.contains(KeyModifiers::SHIFT) => {
            (KeyCode::Char(c.to_ascii_uppercase()), mods)
        }
        code => (code, mods),
    })
}

fn keys_to_bytes(keys: &[String], app_cursor: bool) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    for k in keys {
        out.extend(key_bytes(k, app_cursor).map_err(ApiError::invalid)?);
    }
    Ok(out)
}

/// Validate every key first, then write all bytes in one go. Nothing is sent if any key is bad.
pub fn send_keys(st: &mut State, pid: usize, keys: &[String]) -> Result<(), ApiError> {
    ensure_pane(st, pid)?;
    let app_cursor = st.panes[&pid].parser.screen().application_cursor();
    let bytes = keys_to_bytes(keys, app_cursor)?;
    st.write_pane(pid, &bytes);
    Ok(())
}

/// Paste `text` (bracketed-paste aware), wait `enter_delay_ms` without holding the lock, then Enter.
pub fn submit(
    shared: &Shared,
    pid: usize,
    text: &str,
    enter_delay_ms: u64,
) -> Result<(), ApiError> {
    {
        let mut st = lock(shared);
        ensure_pane(&st, pid)?;
        st.paste(pid, text);
        st.mark_dirty();
    }
    std::thread::sleep(Duration::from_millis(enter_delay_ms));
    let mut st = lock(shared);
    ensure_pane(&st, pid)?;
    st.write_pane(pid, b"\r");
    st.mark_dirty();
    Ok(())
}

enum Matcher {
    Literal(String),
    Re(Regex),
}

impl Matcher {
    fn new(pattern: &Pattern) -> Result<Self, ApiError> {
        match pattern {
            Pattern::Match(s) => Ok(Matcher::Literal(s.clone())),
            Pattern::Regex(s) => Regex::new(s)
                .map(Matcher::Re)
                .map_err(|e| ApiError::invalid(format!("invalid regex: {e}"))),
        }
    }

    fn is_match(&self, line: &str) -> bool {
        match self {
            Matcher::Literal(s) => line.contains(s.as_str()),
            Matcher::Re(re) => re.is_match(line),
        }
    }
}

/// First line of the pane text matching `matcher`, read under a short lock.
fn find_line(
    shared: &Shared,
    pid: usize,
    matcher: &Matcher,
    source: ReadSource,
    lines: Option<usize>,
) -> Result<Option<String>, ApiError> {
    let mut st = lock(shared);
    let text = read_text(&mut st, pid, source, lines)?;
    Ok(text
        .lines()
        .find(|line| matcher.is_match(line))
        .map(str::to_owned))
}

/// Poll the pane text every 100 ms until a line matches or the deadline passes.
/// `timeout_ms: None` (or a timeout too large to represent) waits indefinitely.
fn wait_output(
    shared: &Shared,
    pane: usize,
    pattern: &Pattern,
    source: ReadSource,
    lines: Option<usize>,
    timeout_ms: Option<u64>,
) -> ApiResult {
    let matcher = Matcher::new(pattern)?;
    let deadline = timeout_ms.and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
    loop {
        if let Some(line) = find_line(shared, pane, &matcher, source, lines)? {
            return Ok(Reply::Json(json!({"matched": true, "line": line})));
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(ApiError::new(
                "timeout",
                format!("timed out waiting for output in {}", pane_id_str(pane)),
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(spec: &str) -> Vec<u8> {
        key_bytes(spec, false).unwrap_or_else(|e| panic!("{spec}: {e}"))
    }

    #[test]
    fn key_bytes_named_and_modified_keys() {
        assert_eq!(bytes("enter"), b"\r");
        assert_eq!(bytes("ctrl+c"), b"\x03");
        assert_eq!(bytes("alt+x"), b"\x1bx");
        assert_eq!(bytes("ctrl+alt+x"), b"\x1b\x18");
        assert_eq!(bytes("shift+tab"), b"\x1b[Z");
        assert_eq!(bytes("f5"), b"\x1b[15~");
        assert_eq!(bytes("minus"), b"-");
        assert_eq!(bytes("BSpace"), b"\x7f");
        assert_eq!(bytes("ctrl+space"), b"\x00");
        assert_eq!(bytes("ctrl+minus"), b"\x1f");
        assert_eq!(bytes("shift+a"), b"A");
        assert_eq!(bytes("x"), b"x");
    }

    #[test]
    fn key_bytes_legacy_tmux_names() {
        assert_eq!(bytes("C-c"), b"\x03");
        assert_eq!(bytes("c-c"), b"\x03");
        assert_eq!(bytes("M-x"), b"\x1bx");
        assert_eq!(bytes("Enter"), b"\r");
        assert_eq!(bytes("^A"), b"\x01");
        assert_eq!(bytes("C--"), b"\x1f");
    }

    #[test]
    fn key_bytes_cursor_mode_and_arrows() {
        assert_eq!(key_bytes("up", false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes("up", true).unwrap(), b"\x1bOA");
        assert_eq!(bytes("shift+up"), b"\x1b[1;2A");
    }

    #[test]
    fn key_bytes_rejects_unknown_keys() {
        for bad in ["bogus", "", "ctrl+", "ctrl+1", "f13", "C-"] {
            let err = key_bytes(bad, false).unwrap_err();
            assert!(err.starts_with("unknown key: "), "{bad}: {err}");
        }
    }

    #[test]
    fn tree_json_matches_contract() {
        let node = Node::Split {
            dir: Dir::H,
            ratio: 0.5,
            a: Box::new(Node::Leaf(1)),
            b: Box::new(Node::Split {
                dir: Dir::V,
                ratio: 0.25,
                a: Box::new(Node::Leaf(2)),
                b: Box::new(Node::Leaf(3)),
            }),
        };
        assert_eq!(
            tree_json(&node),
            json!({
                "split": "right",
                "ratio": 0.5,
                "a": {"pane": "p1"},
                "b": {"split": "down", "ratio": 0.25, "a": {"pane": "p2"}, "b": {"pane": "p3"}},
            })
        );
    }

    #[test]
    fn ratio_is_rounded_for_output() {
        assert_eq!(round_ratio(0.3), 0.3);
        assert_eq!(round_ratio(0.123_456), 0.1235);
    }

    #[test]
    fn finish_text_trims_and_keeps_last_lines() {
        let rows = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let all = rows(&["a   ", "b", "", "c", "  ", ""]);
        assert_eq!(finish_text(all.clone(), None), "a\nb\n\nc\n");
        assert_eq!(finish_text(all, Some(1)), "c\n");
        assert_eq!(finish_text(rows(&["", " "]), None), "");
        assert_eq!(finish_text(rows(&["x"]), Some(0)), "");
    }

    #[test]
    fn visible_read_uses_screen_rows() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"x\r\ny");
        let screen = parser.screen_mut();
        let width = screen.size().1;
        assert_eq!(finish_text(screen.rows(0, width), None), "x\ny\n");
        assert_eq!(finish_text(screen.rows(0, width), Some(1)), "y\n");
    }

    #[test]
    fn recent_read_spans_scrollback_and_restores_offset() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"a\r\nb\r\nc\r\nd\r\ne\r\n");
        let screen = parser.screen_mut();
        screen.set_scrollback(1);
        let all = finish_text(recent_rows(screen, 80), Some(80));
        assert_eq!(all, "a\nb\nc\nd\ne\n");
        assert_eq!(screen.scrollback(), 1);
        let last2 = finish_text(recent_rows(screen, 2), Some(2));
        assert_eq!(last2, "d\ne\n");
        assert_eq!(screen.scrollback(), 1);
    }

    #[test]
    fn recent_read_on_empty_pane_is_empty() {
        let mut parser = vt100::Parser::new(4, 10, 100);
        let screen = parser.screen_mut();
        assert_eq!(finish_text(recent_rows(screen, 80), Some(80)), "");
        assert_eq!(finish_text(recent_rows(screen, usize::MAX), Some(3)), "");
    }

    #[test]
    fn label_is_trimmed_capped_and_empty_clears() {
        assert_eq!(normalize_label(Some("  api  ".into())), Some("api".into()));
        assert_eq!(normalize_label(Some("   ".into())), None);
        assert_eq!(normalize_label(None), None);
        let long = "x".repeat(200);
        assert_eq!(normalize_label(Some(long)).unwrap().chars().count(), 80);
    }

    #[test]
    fn split_direction_and_ratio_mapping() {
        assert_eq!(split_dir(Direction::Right).unwrap(), Dir::H);
        assert_eq!(split_dir(Direction::Down).unwrap(), Dir::V);
        assert_eq!(
            split_dir(Direction::Left).unwrap_err().code,
            "invalid_argument"
        );
        assert_eq!(existing_share(None).unwrap(), None);
        assert_eq!(existing_share(Some(0.25)).unwrap(), Some(0.75));
        for bad in [0.0, 1.0, -0.5, 1.5, f32::NAN] {
            let err = existing_share(Some(bad)).unwrap_err();
            assert_eq!(err.code, "invalid_argument");
        }
    }

    #[test]
    fn zoom_on_toggle_off() {
        let mut w = Window {
            name: String::new(),
            manual_name: false,
            layout: Node::Split {
                dir: Dir::H,
                ratio: 0.5,
                a: Box::new(Node::Leaf(1)),
                b: Box::new(Node::Leaf(2)),
            },
            active: 1,
            zoomed: false,
        };
        apply_zoom(&mut w, 2, ZoomMode::On);
        assert!(w.zoomed);
        assert_eq!(w.active, 2);
        apply_zoom(&mut w, 2, ZoomMode::Toggle);
        assert!(!w.zoomed);
        apply_zoom(&mut w, 1, ZoomMode::On);
        apply_zoom(&mut w, 1, ZoomMode::Off);
        assert!(!w.zoomed);
    }

    #[test]
    fn zoom_single_pane_window_stays_unzoomed() {
        let mut w = Window {
            name: String::new(),
            manual_name: false,
            layout: Node::Leaf(7),
            active: 7,
            zoomed: false,
        };
        apply_zoom(&mut w, 7, ZoomMode::On);
        assert!(!w.zoomed);
    }
}
