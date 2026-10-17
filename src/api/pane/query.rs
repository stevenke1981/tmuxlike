//! Read-only pane requests: snapshot, session list, pane list, and the layout tree in snapshots.
use super::{pane_reply, resolve_pane, unexpected};
use crate::api::{ApiError, ApiResult, Reply, Request, lock, pane_id_str, pane_json};
use crate::layout::{Dir, Node};
use crate::server::{Session, Shared, State, Window};
use serde_json::{Value, json};

pub(super) fn query(shared: &Shared, req: Request) -> ApiResult {
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

fn session_missing(name: &str) -> ApiError {
    ApiError::new("session_not_found", format!("session not found: {name}"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
