//! Layout persistence: save sessions/windows/splits/cwd to a state file and restore them
//! when the server starts. OWNER: worker H4.
use crate::server::{Shared, State};

/// Called by the render loop every tick. Writes only when the snapshot changed.
pub fn autosave(st: &State, last: &mut Option<String>) {
    let _ = (st, last);
}

/// Write the current snapshot immediately (used by `server stop`).
pub fn save_now(st: &State) {
    let _ = st;
}

/// Delete the state file (used by `kill-server`).
pub fn clear() {}

/// Restore the saved layout into an empty server. Returns the number of sessions restored.
pub fn restore_on_start(shared: &Shared) -> Result<usize, String> {
    let _ = shared;
    Ok(0)
}
