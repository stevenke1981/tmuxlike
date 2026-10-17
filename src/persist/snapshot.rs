//! State-file format (version 1): data types, validation, and conversion between live layout
//! trees and their saved form.
use crate::api;
use crate::layout::{Dir, Node};
use crate::server::{Pane, Session, State, Window};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(super) const VERSION: u32 = 1;
/// Maximum tree depth (root leaf = 1). Deeper snapshots are rejected.
pub(super) const MAX_DEPTH: usize = 64;
const RATIO_MIN: f32 = 0.05;
const RATIO_MAX: f32 = 0.95;

/// Whole state file. `version` must be 1.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub version: u32,
    pub sessions: Vec<SessionSnap>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionSnap {
    pub name: String,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub sidebar: bool,
    pub active_window: usize,
    pub windows: Vec<WindowSnap>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WindowSnap {
    pub name: String,
    pub manual_name: bool,
    pub zoomed: bool,
    /// index of the focused pane in leaf order
    pub active: usize,
    pub layout: LayoutSnap,
}

/// Per-pane data kept for a leaf. Serialized as the `Leaf` variant's body.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct LeafSnap {
    pub cwd: Option<String>,
    pub label: Option<String>,
    /// agent kind label (`AgentInfo::agent`)
    pub agent: Option<String>,
    /// unique live agent name (`AgentInfo::name`)
    pub agent_name: Option<String>,
    /// command that resumes the agent after a restart
    #[serde(default)]
    pub resume: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum LayoutSnap {
    Leaf(LeafSnap),
    Split {
        dir: Dir,
        ratio: f32,
        a: Box<LayoutSnap>,
        b: Box<LayoutSnap>,
    },
}

pub(super) fn serialize(snap: &Snapshot) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(snap)
}

/// Decode raw state-file bytes. Non-UTF-8 content is an error, as is anything `parse_snapshot`
/// rejects.
pub(super) fn decode_snapshot(bytes: &[u8]) -> Result<Snapshot, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8: {e}"))?;
    parse_snapshot(text)
}

/// Parse and validate a state file. Pure.
pub(super) fn parse_snapshot(text: &str) -> Result<Snapshot, String> {
    let snap: Snapshot = serde_json::from_str(text).map_err(|e| format!("parse: {e}"))?;
    validate(&snap)?;
    Ok(snap)
}

/// Structural checks on a whole snapshot. Ratios must be finite; out-of-range finite values are
/// clamped later by `rebuild`.
pub(super) fn validate(snap: &Snapshot) -> Result<(), String> {
    if snap.version != VERSION {
        return Err(format!("unsupported version {}", snap.version));
    }
    let mut names: Vec<&str> = Vec::with_capacity(snap.sessions.len());
    for s in &snap.sessions {
        if s.name.is_empty() {
            return Err("empty session name".into());
        }
        if names.contains(&s.name.as_str()) {
            return Err(format!("duplicate session name {:?}", s.name));
        }
        names.push(&s.name);
        if s.windows.is_empty() {
            return Err(format!("session {:?} has no windows", s.name));
        }
        if s.active_window >= s.windows.len() {
            return Err(format!("session {:?}: active_window out of range", s.name));
        }
        for w in &s.windows {
            let mut leaves = 0;
            check_layout(&w.layout, 1, &mut leaves)?;
            if w.active >= leaves {
                return Err(format!("window {:?}: active pane out of range", w.name));
            }
        }
    }
    Ok(())
}

fn check_ratio(ratio: f32) -> Result<(), String> {
    if ratio.is_finite() {
        Ok(())
    } else {
        Err(format!("non-finite split ratio {ratio}"))
    }
}

fn check_layout(layout: &LayoutSnap, depth: usize, leaves: &mut usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("layout deeper than {MAX_DEPTH}"));
    }
    match layout {
        LayoutSnap::Leaf(_) => {
            *leaves += 1;
            Ok(())
        }
        LayoutSnap::Split { ratio, a, b, .. } => {
            check_ratio(*ratio)?;
            check_layout(a, depth + 1, leaves)?;
            check_layout(b, depth + 1, leaves)
        }
    }
}

/// Build a `Node` tree from a snapshot tree. `leaf` is called once per leaf, in leaf order,
/// and returns the new pane id. The first error aborts the build.
pub(super) fn rebuild(
    layout: &LayoutSnap,
    leaf: &mut impl FnMut(&LeafSnap) -> Result<usize, String>,
) -> Result<Node, String> {
    rebuild_at(layout, leaf, 1)
}

fn rebuild_at(
    layout: &LayoutSnap,
    leaf: &mut impl FnMut(&LeafSnap) -> Result<usize, String>,
    depth: usize,
) -> Result<Node, String> {
    if depth > MAX_DEPTH {
        return Err(format!("layout deeper than {MAX_DEPTH}"));
    }
    match layout {
        LayoutSnap::Leaf(s) => Ok(Node::Leaf(leaf(s)?)),
        LayoutSnap::Split { dir, ratio, a, b } => {
            check_ratio(*ratio)?;
            let a = rebuild_at(a, leaf, depth + 1)?;
            let b = rebuild_at(b, leaf, depth + 1)?;
            Ok(Node::Split {
                dir: *dir,
                ratio: ratio.clamp(RATIO_MIN, RATIO_MAX),
                a: Box::new(a),
                b: Box::new(b),
            })
        }
    }
}

/// Keep a restored agent name only when it is a valid live name and not already claimed.
/// Kept names are recorded in `taken`. A rejected name is logged and becomes `None`.
pub(super) fn keep_agent_name(raw: Option<&str>, taken: &mut HashSet<String>) -> Option<String> {
    let name = raw?;
    if !api::is_agent_name(name) {
        crate::log!("restore: agent name {name:?} is invalid; dropped");
        return None;
    }
    if !taken.insert(name.to_string()) {
        crate::log!("restore: agent name {name:?} is already taken; dropped");
        return None;
    }
    Some(name.to_string())
}

/// Snapshot of the given sessions. `leaf` supplies the per-pane data for each pane id.
/// Sessions without windows are skipped.
pub(super) fn snapshot_of(sessions: &[Session], leaf: &impl Fn(usize) -> LeafSnap) -> Snapshot {
    let sessions = sessions
        .iter()
        .filter(|s| !s.windows.is_empty())
        .map(|s| SessionSnap {
            name: s.name.clone(),
            cwd: s.cwd.clone(),
            cols: s.cols,
            rows: s.rows,
            sidebar: s.sidebar,
            active_window: s.active.min(s.windows.len() - 1),
            windows: s.windows.iter().map(|w| window_snap(w, leaf)).collect(),
        })
        .collect();
    Snapshot {
        version: VERSION,
        sessions,
    }
}

fn window_snap(w: &Window, leaf: &impl Fn(usize) -> LeafSnap) -> WindowSnap {
    let mut ids = vec![];
    w.layout.leaves(&mut ids);
    WindowSnap {
        name: w.name.clone(),
        manual_name: w.manual_name,
        zoomed: w.zoomed,
        active: ids.iter().position(|&id| id == w.active).unwrap_or(0),
        layout: layout_snap(&w.layout, leaf),
    }
}

/// Convert a live `Node` tree to its snapshot form.
fn layout_snap(node: &Node, leaf: &impl Fn(usize) -> LeafSnap) -> LayoutSnap {
    match node {
        Node::Leaf(id) => LayoutSnap::Leaf(leaf(*id)),
        Node::Split { dir, ratio, a, b } => LayoutSnap::Split {
            dir: *dir,
            ratio: *ratio,
            a: Box::new(layout_snap(a, leaf)),
            b: Box::new(layout_snap(b, leaf)),
        },
    }
}

pub(super) fn current_snapshot(st: &State) -> Snapshot {
    snapshot_of(&st.sessions, &|id| leaf_of(st.panes.get(&id)))
}

fn leaf_of(pane: Option<&Pane>) -> LeafSnap {
    let Some(p) = pane else {
        return LeafSnap::default();
    };
    let agent = p.agent.as_ref();
    LeafSnap {
        cwd: live_cwd(p),
        label: p.label.clone(),
        agent: agent.map(|a| a.agent.clone()),
        agent_name: agent.and_then(|a| a.name.clone()),
        resume: agent.map(|a| a.resume.clone()).unwrap_or_default(),
    }
}

/// Current directory of the pane's foreground process (Linux, via `/proc`). Falls back to the
/// process pid, then to the directory the pane was started in.
#[cfg(target_os = "linux")]
fn live_cwd(p: &Pane) -> Option<String> {
    let leader = p
        .master
        .process_group_leader()
        .and_then(|v| u32::try_from(v).ok());
    [leader, p.child.process_id()]
        .into_iter()
        .flatten()
        .find_map(|pid| std::fs::read_link(format!("/proc/{pid}/cwd")).ok())
        .map(|d| d.to_string_lossy().into_owned())
        .or_else(|| p.cwd.clone())
}

/// Other platforms: the directory the pane was started in.
#[cfg(not(target_os = "linux"))]
fn live_cwd(p: &Pane) -> Option<String> {
    p.cwd.clone()
}
