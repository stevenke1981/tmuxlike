//! Layout persistence: save sessions/windows/splits/cwd to a state file and restore them
//! when the server starts. Processes do not survive a restart; only the layout does.
//! OWNER: worker H4.
mod resume;
mod snapshot;
#[cfg(test)]
mod tests;

use crate::api::{AgentInfo, Status};
use crate::ipc;
use crate::server::{Session, Shared, State, Window};
use snapshot::{LeafSnap, SessionSnap};
use std::collections::HashSet;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Same minimum sizes as `State::new_session`.
const MIN_COLS: u16 = 10;
const MIN_ROWS: u16 = 3;

/// Path of the state file: `$TMUXLIKE_STATE`, else a per-socket file in the local data dir.
fn state_path() -> Option<PathBuf> {
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
    let snap = snapshot::current_snapshot(st);
    if snap.sessions.is_empty() {
        if last.as_deref() != Some("") {
            remove_state_file(&path);
            *last = Some(String::new());
        }
        return;
    }
    let json = match snapshot::serialize(&snap) {
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
    let snap = snapshot::current_snapshot(st);
    if snap.sessions.is_empty() {
        return;
    }
    let Some(path) = state_path() else { return };
    match snapshot::serialize(&snap)
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
/// The whole file is read as bytes, then parsed and validated before anything is spawned. Any
/// read or parse failure moves the file to `<file>.bad`, so later autosaves cannot delete it.
/// When only some sessions come back, the original is kept as `<file>.partial.bad`.
pub fn restore_on_start(shared: &Shared) -> Result<usize, String> {
    let Some(path) = state_path() else {
        return Ok(0);
    };
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(quarantine(&path, &format!("read failed: {e}"))),
    };
    let snap = snapshot::decode_snapshot(&bytes).map_err(|e| quarantine(&path, &e))?;
    let mut guard = shared.lock().map_err(|e| format!("state lock: {e}"))?;
    let st: &mut State = &mut guard;
    let mut names = live_agent_names(st);
    let mut attempted = 0;
    let mut restored = 0;
    for s in &snap.sessions {
        if st.sidx(&s.name).is_some() {
            crate::log!("restore: session {} already exists, skipped", s.name);
            continue;
        }
        attempted += 1;
        match restore_session(st, shared, s, &mut names) {
            Ok(()) => restored += 1,
            Err(e) => crate::log!("restore: session {} failed: {e}", s.name),
        }
    }
    if attempted > 0 && restored == 0 {
        // Keep the snapshot: an empty server would make autosave delete it.
        return Err(quarantine(&path, "no saved session could be restored"));
    }
    if is_partial(attempted, restored) {
        keep_partial_copy(&path);
    }
    st.mark_dirty();
    Ok(restored)
}

/// True when some, but not all, of the attempted sessions came back.
fn is_partial(attempted: usize, restored: usize) -> bool {
    restored > 0 && restored < attempted
}

/// Copy the original state file to `<file>.partial.bad` before the next autosave overwrites it.
/// If the copy fails, the original is moved to `<file>.bad` instead, so it is not lost.
fn keep_partial_copy(path: &Path) {
    let bad = with_suffix(path, ".partial.bad");
    match fs::copy(path, &bad) {
        Ok(_) => crate::log!(
            "restore: partial restore; original kept at {}",
            bad.display()
        ),
        Err(e) => {
            crate::log!("restore: copy to {} failed: {e}", bad.display());
            let _ = quarantine(path, "partial restore; backup copy failed");
        }
    }
}

/// Agent names of panes that are already live. Restored names must not collide with them.
fn live_agent_names(st: &State) -> HashSet<String> {
    st.panes
        .values()
        .filter_map(|p| p.agent.as_ref()?.name.clone())
        .collect()
}

/// Rebuild one session. Panes spawned for it are killed again if any step fails, and the agent
/// names it claimed are released. `names` gains the names of a session that restores.
fn restore_session(
    st: &mut State,
    shared: &Shared,
    s: &SessionSnap,
    names: &mut HashSet<String>,
) -> Result<(), String> {
    let cols = s.cols.max(MIN_COLS);
    let rows = s.rows.max(MIN_ROWS);
    let mut spawned = Vec::new();
    let mut session_names = names.clone();
    let windows = match build_windows(st, shared, s, cols, rows, &mut spawned, &mut session_names) {
        Ok(w) => w,
        Err(e) => {
            for pid in spawned {
                st.kill_pane(pid);
            }
            return Err(e);
        }
    };
    *names = session_names;
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
    names: &mut HashSet<String>,
) -> Result<Vec<Window>, String> {
    let mut out = Vec::with_capacity(s.windows.len());
    for w in &s.windows {
        let mut spawn_leaf = |leaf: &LeafSnap| -> Result<usize, String> {
            let id = st
                .spawn_pane(shared, rows, cols, leaf.cwd.as_deref())
                .map_err(|e| format!("spawn pane: {e}"))?;
            spawned.push(id);
            apply_leaf(st, id, leaf, names);
            Ok(id)
        };
        let layout = snapshot::rebuild(&w.layout, &mut spawn_leaf)?;
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
fn apply_leaf(st: &mut State, id: usize, leaf: &LeafSnap, names: &mut HashSet<String>) {
    let Some(p) = st.panes.get_mut(&id) else {
        return;
    };
    p.label = leaf.label.clone();
    let Some(kind) = leaf.agent.clone() else {
        return;
    };
    p.agent = Some(AgentInfo {
        source: crate::api::RESTORE_SOURCE.into(),
        agent: kind,
        state: Status::Unknown,
        message: None,
        seq: None,
        name: snapshot::keep_agent_name(leaf.agent_name.as_deref(), names),
        resume: leaf.resume.clone(),
    });
    if leaf.resume.is_empty() {
        return;
    }
    match resume::quote_resume(&leaf.resume) {
        Some(line) => st.write_pane(id, format!("{line}\r").as_bytes()),
        None => crate::log!("restore: pane {id}: resume command rejected"),
    }
}

/// Write to `<path>.tmp`, fsync it, then rename over `path`.
fn write_atomic(path: &Path, data: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = with_suffix(path, ".tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
    }
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
