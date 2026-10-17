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

/// Usage text for the API commands (printed by `tmuxlike <group> --help`).
pub const HELP: &str = r#"tmuxlike pane|agent|api|session|server  (herdr-style JSON automation)

Pane ids: p<N> (tmux-style %<N> also accepted). Pane TARGET is one of:
  <id>  |  --pane ID  |  --current   (--current = $TMUXLIKE_PANE_ID inside a pane)
Omitted pane targets use the caller's pane ($TMUXLIKE_PANE_ID) when set, else the focused
pane of the most recently used session.
Output: success prints {"result": ...} on stdout, exit 0; text replies print raw.
Errors print {"error":{"code","message"}} on stderr, exit 1. Usage errors exit 2.
Use -- before text that starts with '-'. Use --opt=value to pass values starting with '--'.

api snapshot
session list [--json]
server stop
pane list [--session NAME]
pane get <id>
pane current [--pane ID | --current]
pane split [TARGET] --direction right|down [--ratio F] [--cwd PATH] [--focus | --no-focus]
pane focus <id>
pane focus --direction left|right|up|down [--pane ID | --current]
pane zoom [TARGET] [--toggle | --on | --off]
pane rename <id> <label>  |  pane rename <id> --clear
pane close <id>
pane read <id> [--source visible|recent] [--lines N]
pane send-text <id> <text>        (no Enter is added)
pane send-keys <id> <key>...      (e.g. Enter C-c Up)
pane run <id> <command>           (command + Enter)
pane wait-output <id> (--match TEXT | --regex PAT) [--source S] [--lines N] [--timeout MS]
pane report-agent <id> --source ID --agent LABEL --state idle|working|blocked|unknown
                  [--message TEXT] [--seq N] [-- RESUME_CMD...]
pane release-agent <id> --source ID --agent LABEL [--seq N]
agent list
agent get <target>
agent read <target> [--source visible|recent] [--lines N]
agent send-keys <target> <key>...
agent focus <target>
agent prompt <target> <text> [--wait [--until S]... [--timeout MS]]
agent wait <target> [--until S]... [--timeout MS]
agent rename <target> <name>  |  agent rename <target> --clear

<target> = pane id or agent name [a-z][a-z0-9_-]{0,31} (a name may not look like a pane id, e.g. p1).
--until S: idle|working|blocked|done|unknown; repeatable (default: idle, done, blocked).
--timeout MS: milliseconds; omitted = wait without limit."#;

/// Parse `args` (starting at the group word, e.g. `["pane", "list"]`).
/// `current` is the caller's pane id from `$TMUXLIKE_PANE_ID`, used by `--current` and as the
/// fallback for omitted pane targets. Err = usage error message (CLI exits with status 2);
/// every help request returns `HELP` exactly.
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
