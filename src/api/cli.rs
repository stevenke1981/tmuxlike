//! argv -> `Request` for the herdr-style CLI (`tmuxlike pane ...`, `tmuxlike agent ...`).
//! OWNER: worker H1. Pure: no I/O, no environment access (caller passes `current`).
use super::Request;

/// Top-level words routed to this parser by `main.rs`.
pub const API_COMMANDS: &[&str] = &["pane", "agent", "api", "session", "server"];

/// Usage text for the API commands (printed by `tmuxlike <group> --help`).
pub const HELP: &str = "";

/// Parse `args` (starting at the group word, e.g. `["pane", "list"]`).
/// `current` is the caller's pane id from `$TMUXLIKE_PANE_ID`, used by `--current`.
/// Err = usage error message (CLI exits with status 2).
pub fn parse(args: &[String], current: Option<usize>) -> Result<Request, String> {
    let _ = (args, current);
    Err("not implemented".into())
}
