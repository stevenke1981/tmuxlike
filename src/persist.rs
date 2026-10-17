//! Layout persistence: save sessions/windows/splits/cwd to a state file and restore them
//! when the server starts. Processes do not survive a restart; only the layout does.
//! OWNER: worker H4.
use crate::api::{AgentInfo, Status};
use crate::ipc;
use crate::layout::{Dir, Node};
use crate::server::{Pane, Session, Shared, State, Window};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;

const VERSION: u32 = 1;
/// Maximum tree depth (root leaf = 1). Deeper snapshots are rejected.
const MAX_DEPTH: usize = 64;
/// Same minimum sizes as `State::new_session`.
const MIN_COLS: u16 = 10;
const MIN_ROWS: u16 = 3;
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

/// Path of the state file: `$TMUXLIKE_STATE`, else a per-socket file in the local data dir.
pub fn state_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("TMUXLIKE_STATE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let file = format!("state-{}.json", sanitize_file_part(&ipc::base_name()));
    Some(dirs::data_local_dir()?.join("tmuxlike").join(file))
}

/// Keep only `[A-Za-z0-9._-]`; every other char becomes `_`.
fn sanitize_file_part(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Called by the render loop every tick. Writes only when the snapshot changed.
pub fn autosave(st: &State, last: &mut Option<String>) {
    let Some(path) = state_path() else { return };
    let snap = current_snapshot(st);
    if snap.sessions.is_empty() {
        if last.as_deref() != Some("") {
            remove_state_file(&path);
            *last = Some(String::new());
        }
        return;
    }
    let json = match serialize(&snap) {
        Ok(j) => j,
        Err(e) => {
            crate::log!("autosave: serialize failed: {e}");
            return;
        }
    };
    if last.as_deref() == Some(json.as_str()) {
        return;
    }
    match write_atomic(&path, &json) {
        Ok(()) => *last = Some(json),
        Err(e) => crate::log!("autosave: write {} failed: {e}", path.display()),
    }
}

/// Write the current snapshot immediately (used by `server stop`).
pub fn save_now(st: &State) {
    let snap = current_snapshot(st);
    if snap.sessions.is_empty() {
        return;
    }
    let Some(path) = state_path() else { return };
    match serialize(&snap)
        .map_err(|e| e.to_string())
        .and_then(|j| write_atomic(&path, &j).map_err(|e| e.to_string()))
    {
        Ok(()) => crate::log!("state saved to {}", path.display()),
        Err(e) => crate::log!("save_now: {} failed: {e}", path.display()),
    }
}

/// Delete the state file (used by `kill-server`).
pub fn clear() {
    if let Some(path) = state_path() {
        remove_state_file(&path);
    }
}

/// Restore the saved layout into an empty server. Returns the number of sessions restored.
///
/// The whole file is parsed and validated before anything is spawned. An invalid file is moved
/// to `<file>.bad`, so later autosaves cannot delete it.
pub fn restore_on_start(shared: &Shared) -> Result<usize, String> {
    let Some(path) = state_path() else {
        return Ok(0);
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let snap = parse_snapshot(&text).map_err(|e| quarantine(&path, &e))?;
    let mut guard = shared.lock().map_err(|e| format!("state lock: {e}"))?;
    let st: &mut State = &mut guard;
    let mut attempted = 0;
    let mut restored = 0;
    for s in &snap.sessions {
        if st.sidx(&s.name).is_some() {
            crate::log!("restore: session {} already exists, skipped", s.name);
            continue;
        }
        attempted += 1;
        match restore_session(st, shared, s) {
            Ok(()) => restored += 1,
            Err(e) => crate::log!("restore: session {} failed: {e}", s.name),
        }
    }
    if attempted > 0 && restored == 0 {
        // Keep the snapshot: an empty server would make autosave delete it.
        return Err(quarantine(&path, "no saved session could be restored"));
    }
    st.mark_dirty();
    Ok(restored)
}

/// Parse and validate a state file. Pure.
pub fn parse_snapshot(text: &str) -> Result<Snapshot, String> {
    let snap: Snapshot = serde_json::from_str(text).map_err(|e| format!("parse: {e}"))?;
    validate(&snap)?;
    Ok(snap)
}

/// Structural checks on a whole snapshot. Ratios must be finite; out-of-range finite values are
/// clamped later by `rebuild`.
pub fn validate(snap: &Snapshot) -> Result<(), String> {
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
pub fn rebuild(
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

/// Shell command line that resumes an agent, or `None` when it must not be typed.
/// Each argument is single-quoted unless it is plain. Rejected: an empty or path-like first
/// argument, any apostrophe (including PowerShell's typographic ones), and control characters.
pub fn quote_resume(args: &[String]) -> Option<String> {
    let first = args.first()?;
    if first.is_empty() || first.contains(['/', '\\']) {
        return None;
    }
    if args
        .iter()
        .any(|a| a.contains(APOSTROPHES) || a.chars().any(char::is_control))
    {
        return None;
    }
    Some(
        args.iter()
            .map(|a| quote_arg(a))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Apostrophe-like characters. PowerShell treats the typographic ones as quotes too.
const APOSTROPHES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];

fn quote_arg(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=,@+".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{arg}'")
    }
}

/// Snapshot of the given sessions. `leaf` supplies the per-pane data for each pane id.
/// Sessions without windows are skipped.
pub fn snapshot_of(sessions: &[Session], leaf: &impl Fn(usize) -> LeafSnap) -> Snapshot {
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
pub fn layout_snap(node: &Node, leaf: &impl Fn(usize) -> LeafSnap) -> LayoutSnap {
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

fn current_snapshot(st: &State) -> Snapshot {
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
        .find_map(|pid| fs::read_link(format!("/proc/{pid}/cwd")).ok())
        .map(|d| d.to_string_lossy().into_owned())
        .or_else(|| p.cwd.clone())
}

/// Other platforms: the directory the pane was started in.
#[cfg(not(target_os = "linux"))]
fn live_cwd(p: &Pane) -> Option<String> {
    p.cwd.clone()
}

/// Rebuild one session. Panes spawned for it are killed again if any step fails.
fn restore_session(st: &mut State, shared: &Shared, s: &SessionSnap) -> Result<(), String> {
    let cols = s.cols.max(MIN_COLS);
    let rows = s.rows.max(MIN_ROWS);
    let mut spawned = Vec::new();
    let windows = match build_windows(st, shared, s, cols, rows, &mut spawned) {
        Ok(w) => w,
        Err(e) => {
            for pid in spawned {
                st.kill_pane(pid);
            }
            return Err(e);
        }
    };
    st.sessions.push(Session {
        name: s.name.clone(),
        windows,
        active: s.active_window,
        cols,
        rows,
        sidebar: s.sidebar,
        cwd: s.cwd.clone(),
        last_used: Instant::now(),
    });
    let si = st.sessions.len() - 1;
    st.relayout(si);
    Ok(())
}

fn build_windows(
    st: &mut State,
    shared: &Shared,
    s: &SessionSnap,
    cols: u16,
    rows: u16,
    spawned: &mut Vec<usize>,
) -> Result<Vec<Window>, String> {
    let mut out = Vec::with_capacity(s.windows.len());
    for w in &s.windows {
        let mut spawn_leaf = |leaf: &LeafSnap| -> Result<usize, String> {
            let id = st
                .spawn_pane(shared, rows, cols, leaf.cwd.as_deref())
                .map_err(|e| format!("spawn pane: {e}"))?;
            spawned.push(id);
            apply_leaf(st, id, leaf);
            Ok(id)
        };
        let layout = rebuild(&w.layout, &mut spawn_leaf)?;
        let mut leaves = vec![];
        layout.leaves(&mut leaves);
        let active = *leaves
            .get(w.active)
            .ok_or_else(|| format!("window {:?}: active pane out of range", w.name))?;
        out.push(Window {
            name: w.name.clone(),
            manual_name: w.manual_name,
            layout,
            active,
            zoomed: w.zoomed,
        });
    }
    if out.is_empty() {
        return Err(format!("session {:?} has no windows", s.name));
    }
    Ok(out)
}

/// Apply saved label and agent data to a freshly spawned pane, and type its resume command.
fn apply_leaf(st: &mut State, id: usize, leaf: &LeafSnap) {
    let Some(p) = st.panes.get_mut(&id) else {
        return;
    };
    p.label = leaf.label.clone();
    let Some(kind) = leaf.agent.clone() else {
        return;
    };
    p.agent = Some(AgentInfo {
        source: "restore".into(),
        agent: kind,
        state: Status::Unknown,
        message: None,
        seq: None,
        name: leaf.agent_name.clone(),
        resume: leaf.resume.clone(),
    });
    if leaf.resume.is_empty() {
        return;
    }
    match quote_resume(&leaf.resume) {
        Some(line) => st.write_pane(id, format!("{line}\r").as_bytes()),
        None => crate::log!("restore: pane {id}: resume command rejected"),
    }
}

fn serialize(snap: &Snapshot) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(snap)
}

/// Write to `<path>.tmp`, then rename over `path`.
fn write_atomic(path: &Path, data: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = with_suffix(path, ".tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

/// Move an invalid snapshot to `<path>.bad`. Returns the error message to report.
fn quarantine(path: &Path, reason: &str) -> String {
    let bad = with_suffix(path, ".bad");
    match fs::rename(path, &bad) {
        Ok(()) => {
            crate::log!("restore: {reason}; moved to {}", bad.display());
            format!("{reason}; moved to {}", bad.display())
        }
        Err(e) => {
            crate::log!("restore: {reason}; move to {} failed: {e}", bad.display());
            format!("{reason}; could not move to {}: {e}", bad.display())
        }
    }
}

fn remove_state_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => crate::log!("remove {} failed: {e}", path.display()),
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_leaf(id: usize) -> LeafSnap {
        LeafSnap {
            cwd: Some(format!("/work/{id}")),
            ..LeafSnap::default()
        }
    }

    /// Pane 2 is an agent with a label and a resume command.
    fn lookup(id: usize) -> LeafSnap {
        if id == 2 {
            LeafSnap {
                cwd: Some("/work/api".into()),
                label: Some("api".into()),
                agent: Some("claude".into()),
                agent_name: Some("reviewer".into()),
                resume: vec!["claude".into(), "--resume".into(), "abc123".into()],
            }
        } else {
            plain_leaf(id)
        }
    }

    fn three_pane_session() -> Session {
        let layout = Node::Split {
            dir: Dir::H,
            ratio: 0.4,
            a: Box::new(Node::Leaf(1)),
            b: Box::new(Node::Split {
                dir: Dir::V,
                ratio: 0.5,
                a: Box::new(Node::Leaf(2)),
                b: Box::new(Node::Leaf(3)),
            }),
        };
        Session {
            name: "main".into(),
            windows: vec![Window {
                name: "sh".into(),
                manual_name: false,
                layout,
                active: 3,
                zoomed: true,
            }],
            active: 0,
            cols: 120,
            rows: 40,
            sidebar: true,
            cwd: Some("/work".into()),
            last_used: Instant::now(),
        }
    }

    fn sample() -> Snapshot {
        snapshot_of(&[three_pane_session()], &lookup)
    }

    fn set_first_window(snap: &mut Snapshot, f: impl FnOnce(&mut WindowSnap)) {
        f(&mut snap.sessions[0].windows[0]);
    }

    fn chain(splits: usize) -> LayoutSnap {
        let mut node = LayoutSnap::Leaf(LeafSnap::default());
        for _ in 0..splits {
            node = LayoutSnap::Split {
                dir: Dir::H,
                ratio: 0.5,
                a: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
                b: Box::new(node),
            };
        }
        node
    }

    #[test]
    fn json_roundtrip_of_three_pane_tree() {
        let snap = sample();
        let text = serialize(&snap).unwrap();
        let back = parse_snapshot(&text).unwrap();
        assert_eq!(back, snap);
        let w = &back.sessions[0].windows[0];
        assert_eq!(w.active, 2, "pane 3 is leaf index 2");
        assert!(w.zoomed);
        assert_eq!(
            back.sessions[0].windows[0].layout,
            snap.sessions[0].windows[0].layout
        );
    }

    #[test]
    fn validate_rejects_bad_version() {
        let mut snap = sample();
        snap.version = 2;
        assert!(validate(&snap).is_err());
    }

    #[test]
    fn validate_rejects_empty_windows() {
        let mut snap = sample();
        snap.sessions[0].windows.clear();
        assert!(validate(&snap).is_err());
    }

    #[test]
    fn validate_rejects_out_of_range_indices() {
        let mut snap = sample();
        set_first_window(&mut snap, |w| w.active = 3);
        assert!(
            validate(&snap).is_err(),
            "3 leaves, index 3 is out of range"
        );

        let mut snap = sample();
        snap.sessions[0].active_window = 1;
        assert!(validate(&snap).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_and_empty_names() {
        let mut snap = sample();
        let dup = snap.sessions[0].clone();
        snap.sessions.push(dup);
        assert!(validate(&snap).is_err());

        let mut snap = sample();
        snap.sessions[0].name.clear();
        assert!(validate(&snap).is_err());
    }

    #[test]
    fn validate_rejects_non_finite_ratio() {
        let mut snap = sample();
        set_first_window(&mut snap, |w| {
            w.layout = LayoutSnap::Split {
                dir: Dir::V,
                ratio: f32::NAN,
                a: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
                b: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
            };
        });
        assert!(validate(&snap).is_err());
    }

    #[test]
    fn validate_limits_depth_to_64() {
        let ok = |n| {
            let mut snap = sample();
            set_first_window(&mut snap, |w| {
                w.layout = chain(n);
                w.active = 0;
            });
            validate(&snap)
        };
        assert!(ok(63).is_ok(), "64 levels");
        assert!(ok(64).is_err(), "65 levels");
    }

    #[test]
    fn rebuild_assigns_leaf_ids_in_order() {
        let mut next = 9;
        let node = rebuild(
            &sample().sessions[0].windows[0].layout,
            &mut |_: &LeafSnap| {
                next += 1;
                Ok(next)
            },
        )
        .unwrap();
        let mut leaves = vec![];
        node.leaves(&mut leaves);
        assert_eq!(leaves, vec![10, 11, 12]);
    }

    #[test]
    fn rebuild_aborts_on_first_closure_error() {
        let mut calls = 0;
        let layout = &sample().sessions[0].windows[0].layout;
        let result = rebuild(layout, &mut |_: &LeafSnap| {
            calls += 1;
            if calls == 2 {
                Err("boom".to_string())
            } else {
                Ok(calls)
            }
        });
        assert_eq!(result.unwrap_err(), "boom");
        assert_eq!(calls, 2, "no leaves after the failing one");
    }

    #[test]
    fn rebuild_clamps_finite_ratios() {
        let split = |ratio: f32| LayoutSnap::Split {
            dir: Dir::H,
            ratio,
            a: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
            b: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
        };
        let mut id = 0;
        let mut next = |_: &LeafSnap| {
            id += 1;
            Ok(id)
        };
        let high = rebuild(&split(2.0), &mut next).unwrap();
        let low = rebuild(&split(-1.0), &mut next).unwrap();
        assert!(matches!(high, Node::Split { ratio, .. } if ratio == 0.95));
        assert!(matches!(low, Node::Split { ratio, .. } if ratio == 0.05));
        assert!(rebuild(&split(f32::INFINITY), &mut next).is_err());
    }

    #[test]
    fn quote_resume_cases() {
        let args = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
        assert_eq!(
            quote_resume(&args(&["claude", "--resume", "abc123"])).as_deref(),
            Some("claude --resume abc123")
        );
        assert_eq!(
            quote_resume(&args(&["claude", "my file"])).as_deref(),
            Some("claude 'my file'")
        );
        assert_eq!(
            quote_resume(&args(&["tool", "a;rm -rf x", ""])).as_deref(),
            Some("tool 'a;rm -rf x' ''")
        );
        assert_eq!(quote_resume(&args(&[])), None);
        assert_eq!(quote_resume(&args(&["", "x"])), None);
        assert_eq!(quote_resume(&args(&["./claude"])), None);
        assert_eq!(quote_resume(&args(&["/usr/bin/claude"])), None);
        assert_eq!(quote_resume(&args(&["C:\\tools\\claude.exe"])), None);
        assert_eq!(quote_resume(&args(&["claude", "it's"])), None);
        assert_eq!(quote_resume(&args(&["claude", "\u{2019}x"])), None);
        assert_eq!(quote_resume(&args(&["claude", "a\nb"])), None);
    }

    #[test]
    fn sanitize_keeps_only_safe_chars() {
        assert_eq!(
            sanitize_file_part("tmuxlike-user.sock"),
            "tmuxlike-user.sock"
        );
        assert_eq!(sanitize_file_part("a b/c\\d:e"), "a_b_c_d_e");
        assert_eq!(sanitize_file_part("..\\x"), ".._x");
        assert_eq!(sanitize_file_part("ü"), "_");
    }

    #[test]
    fn snapshot_maps_focus_and_skips_empty_sessions() {
        let mut empty = three_pane_session();
        empty.name = "empty".into();
        empty.windows.clear();
        let snap = snapshot_of(&[empty, three_pane_session()], &lookup);
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions[0].windows[0].active, 2);
        assert!(validate(&snap).is_ok());
    }

    #[test]
    fn parse_rejects_invalid_json() {
        assert!(parse_snapshot("{not json").is_err());
    }

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir =
            std::env::temp_dir().join(format!("tmuxlike-persist-test-{}", std::process::id()));
        let path = dir.join("state.json");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!with_suffix(&path, ".tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
