//! argv -> `Request` for the herdr-style CLI (`tmuxlike pane ...`, `tmuxlike agent ...`).
//! Pure: no I/O, no environment access (the caller passes `current`).
//! Public API: `API_COMMANDS`, `HELP`, `parse`. Submodules: `scan` (option scanner and option
//! specs), `args` (validators), `pane_cmds`, `agent_cmds`, `tests`.
mod agent_cmds;
mod args;
mod pane_cmds;
mod scan;
#[cfg(test)]
mod tests;

use crate::api::Request;
use args::{check_count, none, unknown_sub};
use scan::{flag, no_opts, scan};

/// Top-level words routed to this parser by `main.rs`.
pub const API_COMMANDS: &[&str] = &["pane", "agent", "api", "session", "server"];

/// Sentinel error returned for every help request (`pane --help`, `pane` alone, ...). The
/// usage text itself is the `cli.api_help` catalog entry; `main.rs` prints it in the CLI language.
pub const HELP: &str = "__help__";

/// Parse `args` (starting at the group word, e.g. `["pane", "list"]`).
/// `current` is the caller's pane id from `$TMUXLIKE_PANE_ID`, used by `--current` and as the
/// fallback for omitted pane targets. Err = usage error message (CLI exits with status 2), or
/// `HELP` exactly for every help request.
pub fn parse(args: &[String], current: Option<usize>) -> Result<Request, String> {
    let [group, sub, rest @ ..] = args else {
        return Err(HELP.to_string());
    };
    if is_help_word(sub) || wants_help(rest) {
        return Err(HELP.to_string());
    }
    match group.as_str() {
        "pane" => pane_cmds::dispatch(sub, rest, current),
        "agent" => agent_cmds::dispatch(sub, rest),
        "api" => api(sub, rest),
        "session" => session(sub, rest),
        "server" => server(sub, rest),
        other => Err(format!(
            "unknown command group '{other}'; expected one of: {}",
            API_COMMANDS.join(", ")
        )),
    }
}

fn is_help_flag(s: &str) -> bool {
    matches!(s, "--help" | "-h")
}

fn is_help_word(s: &str) -> bool {
    is_help_flag(s) || s == "help"
}

fn wants_help(rest: &[String]) -> bool {
    rest.iter()
        .take_while(|s| s.as_str() != "--")
        .any(|s| is_help_flag(s))
}

fn api(sub: &str, rest: &[String]) -> Result<Request, String> {
    match sub {
        "snapshot" => {
            check_count(&no_opts(rest)?, 0, 0, "api snapshot")?;
            Ok(Request::ApiSnapshot)
        }
        _ => Err(unknown_sub("api", sub, &["snapshot"])),
    }
}

fn session(sub: &str, rest: &[String]) -> Result<Request, String> {
    match sub {
        "list" => {
            let a = scan(rest, &[flag("json")])?;
            none(&a, "session list [--json]")?;
            Ok(Request::SessionList)
        }
        _ => Err(unknown_sub("session", sub, &["list"])),
    }
}

fn server(sub: &str, rest: &[String]) -> Result<Request, String> {
    match sub {
        "stop" => {
            check_count(&no_opts(rest)?, 0, 0, "server stop")?;
            Ok(Request::ServerStop)
        }
        _ => Err(unknown_sub("server", sub, &["stop"])),
    }
}
