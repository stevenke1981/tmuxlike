//! Pane, session and snapshot handlers.
//!
//! Handlers lock `shared` themselves. Anything that waits (`PaneWaitOutput`, the Enter delay in
//! `submit`) releases the lock between polls, so the server keeps rendering and reading.
//! Submodules: `query` (snapshot, sessions, pane list), `layout` (split, focus, zoom, rename,
//! close), `keys` (key specs to bytes, send, submit), `read` (pane text and the wait matcher).
use super::{
    ApiError, ApiResult, POLL_INTERVAL, Pattern, ReadSource, Reply, Request, lock, pane_id_str,
    pane_json,
};
use crate::server::{Shared, State};
use serde_json::json;
use std::time::{Duration, Instant};

mod keys;
mod layout;
mod query;
mod read;

// Kept public path for `super::pane::key_bytes`; no caller outside this module uses it yet.
#[allow(unused_imports)]
pub use keys::key_bytes;
pub use keys::{send_keys, submit};
pub use read::read_text;

use read::{Matcher, find_line};

/// Delay between pasting text and sending Enter in `submit` (`PaneRun`).
const RUN_ENTER_DELAY_MS: u64 = 30;

pub fn handle(shared: &Shared, req: Request) -> ApiResult {
    match req {
        Request::ApiSnapshot
        | Request::SessionList
        | Request::PaneList { .. }
        | Request::PaneGet { .. }
        | Request::PaneCurrent { .. } => query::query(shared, req),
        Request::PaneSplit { .. }
        | Request::PaneFocus { .. }
        | Request::PaneFocusDirection { .. }
        | Request::PaneZoom { .. }
        | Request::PaneRename { .. }
        | Request::PaneClose { .. } => layout::layout_op(shared, req),
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

/// Poll the pane's logical lines every `POLL_INTERVAL` until one matches or the deadline passes.
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
