//! herdr-style automation API: typed requests sent by the CLI, handled by the server.
//!
//! Contract owned by the coordinator. Submodules:
//! - `cli`   argv -> `Request` (pure, no I/O)
//! - `pane`  pane/session/snapshot handlers and terminal I/O helpers
//! - `agent` agent state model, reports, waits and prompts
use crate::server::{Shared, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub mod agent;
pub mod cli;
pub mod pane;

/// Agent/pane status. `Done` = idle but not yet seen by a viewer.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Working => "working",
            Status::Blocked => "blocked",
            Status::Done => "done",
            Status::Unknown => "unknown",
        }
    }
    pub fn parse(s: &str) -> Option<Status> {
        Some(match s {
            "idle" => Status::Idle,
            "working" => Status::Working,
            "blocked" => Status::Blocked,
            "done" => Status::Done,
            "unknown" => Status::Unknown,
            _ => return None,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReadSource {
    /// the visible screen
    Visible,
    /// the last N rows (default 80) including scrollback
    Recent,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ZoomMode {
    Toggle,
    On,
    Off,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// literal substring on one line
    Match(String),
    /// Rust regex, matched one line at a time
    Regex(String),
}

/// Agent target: pane id or unique live agent name.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Pane(usize),
    Name(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WaitSpec {
    /// empty = default set (idle, done, blocked)
    pub until: Vec<Status>,
    /// None = wait indefinitely
    pub timeout_ms: Option<u64>,
}

/// Every API request. Pane ids are the numeric part of `p<N>`.
/// `None` pane targets mean "the focused pane of the most recently used session".
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Request {
    ApiSnapshot,
    SessionList,
    ServerStop,
    PaneList {
        session: Option<String>,
    },
    PaneGet {
        pane: usize,
    },
    PaneCurrent {
        pane: Option<usize>,
    },
    PaneSplit {
        pane: Option<usize>,
        direction: Direction,
        ratio: Option<f32>,
        cwd: Option<String>,
        focus: bool,
    },
    PaneFocus {
        pane: usize,
    },
    PaneFocusDirection {
        pane: Option<usize>,
        direction: Direction,
    },
    PaneZoom {
        pane: Option<usize>,
        mode: ZoomMode,
    },
    PaneRename {
        pane: usize,
        label: Option<String>,
    },
    PaneClose {
        pane: usize,
    },
    PaneRead {
        pane: usize,
        source: ReadSource,
        lines: Option<usize>,
    },
    PaneSendText {
        pane: usize,
        text: String,
    },
    PaneSendKeys {
        pane: usize,
        keys: Vec<String>,
    },
    PaneRun {
        pane: usize,
        command: String,
    },
    PaneWaitOutput {
        pane: usize,
        pattern: Pattern,
        source: ReadSource,
        lines: Option<usize>,
        timeout_ms: Option<u64>,
    },
    PaneReportAgent {
        pane: usize,
        source: String,
        agent: String,
        state: Status,
        message: Option<String>,
        seq: Option<u64>,
        resume: Vec<String>,
    },
    PaneReleaseAgent {
        pane: usize,
        source: String,
        agent: String,
        seq: Option<u64>,
    },
    AgentList,
    AgentGet {
        target: Target,
    },
    AgentRead {
        target: Target,
        source: ReadSource,
        lines: Option<usize>,
    },
    AgentSendKeys {
        target: Target,
        keys: Vec<String>,
    },
    AgentPrompt {
        target: Target,
        text: String,
        wait: Option<WaitSpec>,
    },
    AgentWait {
        target: Target,
        wait: WaitSpec,
    },
    AgentRename {
        target: Target,
        name: Option<String>,
    },
    AgentFocus {
        target: Target,
    },
}

/// Reported agent state stored on a pane (`Pane::agent`).
#[derive(Debug, Clone, PartialEq)]
pub struct AgentInfo {
    /// reporter id (`--source`)
    pub source: String,
    /// agent kind label (`--agent`, e.g. "claude")
    pub agent: String,
    /// last reported state (idle|working|blocked|unknown)
    pub state: Status,
    pub message: Option<String>,
    pub seq: Option<u64>,
    /// unique live agent name (`agent rename`)
    pub name: Option<String>,
    /// command to resume this agent after a server restart (`-- CMD...`)
    pub resume: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ApiError {
    /// stable machine code: pane_not_found, agent_not_found, invalid_argument, timeout,
    /// agent_blocked, agent_prompt_stalled, spawn_failed, not_implemented, internal
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        ApiError {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn pane_not_found(id: usize) -> Self {
        Self::new(
            "pane_not_found",
            format!("pane not found: {}", pane_id_str(id)),
        )
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_argument", message)
    }
}

/// Successful reply: JSON is printed as `{"result": ...}`; Text is printed verbatim.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Reply {
    Json(Value),
    Text(String),
}

pub type ApiResult = Result<Reply, ApiError>;

/// Poll period for every wait; the server lock is never held while sleeping.
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// `AgentInfo::source` of agents rebuilt from the state file after a server restart.
pub const RESTORE_SOURCE: &str = "restore";

/// Max chars for labels, agent labels, messages and other display text.
pub const LABEL_MAX: usize = 80;

/// Lock the server state; a poisoned lock is recovered so socket input never panics a handler.
pub fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// Trim, drop control characters and cap at `max` chars. Empty result -> None.
pub fn clean_text(s: &str, max: usize) -> Option<String> {
    let t: String = s
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect();
    let t = t.trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// Valid live agent name: `[a-z][a-z0-9_-]{0,31}` and not shaped like a pane id (`p12`),
/// so a name can never shadow a pane target.
pub fn is_agent_name(s: &str) -> bool {
    let mut cs = s.chars();
    let first_ok = cs.next().is_some_and(|c| c.is_ascii_lowercase());
    first_ok
        && s.len() <= 32
        && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && parse_pane_id(s).is_none()
}

/// `p12` -> 12. Also accepts tmux-style `%12`. Rejects 0, leading zeros, signs and junk.
pub fn parse_pane_id(s: &str) -> Option<usize> {
    let digits = s.strip_prefix('p').or_else(|| s.strip_prefix('%'))?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub fn pane_id_str(id: usize) -> String {
    format!("p{id}")
}

/// Canonical JSON for one pane. Shared by `pane list/get`, `agent list/get` and snapshots.
/// Fields: pane_id, session, window, window_name, focused, zoomed, label, cwd, cols, rows,
/// status, agent (null or {source, agent, state, message, name}).
pub fn pane_json(st: &State, pid: usize) -> Option<Value> {
    let p = st.panes.get(&pid)?;
    let (si, wi) = st.locate(pid)?;
    let s = &st.sessions[si];
    let w = &s.windows[wi];
    let agent = p.agent.as_ref().map(|a| {
        json!({
            "source": a.source,
            "agent": a.agent,
            "state": a.state.as_str(),
            "message": a.message,
            "name": a.name,
        })
    });
    Some(json!({
        "pane_id": pane_id_str(pid),
        "session": s.name,
        "window": wi,
        "window_name": w.name,
        "focused": s.active == wi && w.active == pid,
        "zoomed": w.zoomed && w.active == pid,
        "label": p.label,
        "cwd": p.cwd,
        "cols": p.rect.w,
        "rows": p.rect.h,
        "status": agent::status(p, &st.cfg).as_str(),
        "agent": agent,
    }))
}

/// Server entry point for every API request. Long waits must not hold the lock.
pub fn handle(shared: &Shared, req: Request) -> ApiResult {
    match req {
        Request::ServerStop => {
            // keep persisted state so the next server start restores the layout;
            // the connection handler exits the process after sending this reply
            let st = lock(shared);
            if st.cfg.persist {
                crate::persist::save_now(&st);
            }
            Ok(Reply::Json(json!({ "stopped": true })))
        }
        Request::PaneReportAgent { .. }
        | Request::PaneReleaseAgent { .. }
        | Request::AgentList
        | Request::AgentGet { .. }
        | Request::AgentRead { .. }
        | Request::AgentSendKeys { .. }
        | Request::AgentPrompt { .. }
        | Request::AgentWait { .. }
        | Request::AgentRename { .. }
        | Request::AgentFocus { .. } => agent::handle(shared, req),
        other => pane::handle(shared, other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pane_ids() {
        assert_eq!(parse_pane_id("p42"), Some(42));
        assert_eq!(parse_pane_id("%7"), Some(7));
        for bad in ["42", "p0", "p01", "p", "p+1", "p1.2", "w1:p1", "p１"] {
            assert_eq!(parse_pane_id(bad), None, "{bad}");
        }
        assert_eq!(pane_id_str(3), "p3");
    }
    #[test]
    fn agent_names() {
        for ok in ["a", "reviewer", "claude-2", "x_y", "pa", "p1x"] {
            assert!(is_agent_name(ok), "{ok}");
        }
        for bad in ["", "P1", "1a", "p1", "p42", "a b", "a.b", &"a".repeat(33)] {
            assert!(!is_agent_name(bad), "{bad}");
        }
    }
    #[test]
    fn clean_text_strips_controls() {
        assert_eq!(clean_text("  a\u{1b}b\n ", 80).as_deref(), Some("ab"));
        assert_eq!(clean_text(" \t ", 80), None);
        assert_eq!(clean_text("中文字", 2).as_deref(), Some("中文"));
    }
}
