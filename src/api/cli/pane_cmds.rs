//! `pane` group: argv -> `Request::Pane*`. An omitted target falls back to the caller's pane
//! (`current`, from `$TMUXLIKE_PANE_ID`); without a caller pane the request carries `None`.
use super::args::{
    agent_label, check_count, lines_opt, none, one_id, opt_num, pane_id, ratio_opt, read_source,
    regex_arg, report_state, required, resume_cmd, source_id, unknown_sub,
};
use super::scan::{
    Args, READ_OPTS, RELEASE_OPTS, REPORT_OPTS, SPLIT_OPTS, WAIT_OUTPUT_OPTS, flag, no_opts, scan,
    val,
};
use crate::api::{Direction, Pattern, Request, ZoomMode};

const PANE_SUBS: &[&str] = &[
    "list",
    "get",
    "current",
    "split",
    "focus",
    "zoom",
    "rename",
    "close",
    "read",
    "send-text",
    "send-keys",
    "run",
    "wait-output",
    "report-agent",
    "release-agent",
];

pub(super) fn dispatch(
    sub: &str,
    rest: &[String],
    current: Option<usize>,
) -> Result<Request, String> {
    match sub {
        "list" => pane_list(rest),
        "get" => pane_get(rest),
        "current" => pane_current(rest, current),
        "split" => pane_split(rest, current),
        "focus" => pane_focus(rest, current),
        "zoom" => pane_zoom(rest, current),
        "rename" => pane_rename(rest),
        "close" => pane_close(rest),
        "read" => pane_read(rest),
        "send-text" => pane_send_text(rest),
        "send-keys" => pane_send_keys(rest),
        "run" => pane_run(rest),
        "wait-output" => pane_wait_output(rest),
        "report-agent" => pane_report_agent(rest),
        "release-agent" => pane_release_agent(rest),
        _ => Err(unknown_sub("pane", sub, PANE_SUBS)),
    }
}

/// Pane TARGET from an optional positional id plus `--pane` / `--current` (one form at most).
fn target_of(
    a: &Args,
    positional: Option<&str>,
    current: Option<usize>,
) -> Result<Option<usize>, String> {
    let flagged = flag_target(a, current)?;
    match (positional, flagged) {
        (Some(_), Some(_)) => {
            Err("give one pane target: a positional id, --pane ID, or --current".into())
        }
        (Some(p), None) => pane_id(p).map(Some),
        (None, f) => Ok(f),
    }
}

fn flag_target(a: &Args, current: Option<usize>) -> Result<Option<usize>, String> {
    match (a.get("pane"), a.has("current")) {
        (Some(_), true) => Err("use only one of --pane or --current".into()),
        (Some(p), false) => pane_id(p).map(Some),
        (None, true) => current.map(Some).ok_or_else(|| {
            "--current needs TMUXLIKE_PANE_ID (run inside a tmuxlike pane)".to_string()
        }),
        (None, false) => Ok(None),
    }
}

fn split_direction(s: Option<&str>) -> Result<Direction, String> {
    match s {
        Some("right") => Ok(Direction::Right),
        Some("down") => Ok(Direction::Down),
        Some(other) => Err(format!(
            "invalid --direction '{other}' for pane split (right|down)"
        )),
        None => Err("pane split needs --direction right|down".into()),
    }
}

fn any_direction(s: &str) -> Result<Direction, String> {
    match s {
        "left" => Ok(Direction::Left),
        "right" => Ok(Direction::Right),
        "up" => Ok(Direction::Up),
        "down" => Ok(Direction::Down),
        other => Err(format!(
            "invalid --direction '{other}' (left|right|up|down)"
        )),
    }
}

fn pane_list(rest: &[String]) -> Result<Request, String> {
    let a = scan(rest, &[val("session")])?;
    none(&a, "pane list [--session NAME]")?;
    let session = match a.get("session") {
        Some("") => return Err("--session needs a non-empty name".into()),
        other => other.map(str::to_string),
    };
    Ok(Request::PaneList { session })
}

fn pane_get(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    Ok(Request::PaneGet {
        pane: one_id(&words, "pane get <id>")?,
    })
}

fn pane_current(rest: &[String], current: Option<usize>) -> Result<Request, String> {
    let a = scan(rest, &[val("pane"), flag("current")])?;
    none(&a, "pane current [--pane ID | --current]")?;
    let pane = target_of(&a, None, current)?.or(current);
    Ok(Request::PaneCurrent { pane })
}

fn pane_split(rest: &[String], current: Option<usize>) -> Result<Request, String> {
    const USAGE: &str =
        "pane split [TARGET] --direction right|down [--ratio F] [--cwd PATH] [--focus|--no-focus]";
    let a = scan(rest, SPLIT_OPTS)?;
    a.exclusive(&["focus", "no-focus"])?;
    let words = a.words();
    check_count(&words, 0, 1, USAGE)?;
    let pane = target_of(&a, words.first().map(String::as_str), current)?.or(current);
    Ok(Request::PaneSplit {
        pane,
        direction: split_direction(a.get("direction"))?,
        ratio: ratio_opt(&a)?,
        cwd: a.get("cwd").map(str::to_string),
        focus: a.has("focus"),
    })
}

fn pane_focus(rest: &[String], current: Option<usize>) -> Result<Request, String> {
    const ID_USAGE: &str = "pane focus <id>";
    const DIR_USAGE: &str = "pane focus --direction left|right|up|down [--pane ID | --current]";
    let a = scan(rest, &[val("direction"), val("pane"), flag("current")])?;
    let Some(dir) = a.get("direction") else {
        if a.has("pane") || a.has("current") {
            return Err(format!("usage: tmuxlike {DIR_USAGE}"));
        }
        return Ok(Request::PaneFocus {
            pane: one_id(&a.words(), ID_USAGE)?,
        });
    };
    let direction = any_direction(dir)?;
    check_count(&a.words(), 0, 0, DIR_USAGE)?;
    Ok(Request::PaneFocusDirection {
        pane: target_of(&a, None, current)?.or(current),
        direction,
    })
}

fn pane_zoom(rest: &[String], current: Option<usize>) -> Result<Request, String> {
    const USAGE: &str = "pane zoom [TARGET] [--toggle|--on|--off]";
    let a = scan(
        rest,
        &[
            val("pane"),
            flag("current"),
            flag("toggle"),
            flag("on"),
            flag("off"),
        ],
    )?;
    a.exclusive(&["toggle", "on", "off"])?;
    let words = a.words();
    check_count(&words, 0, 1, USAGE)?;
    let pane = target_of(&a, words.first().map(String::as_str), current)?.or(current);
    let mode = match (a.has("on"), a.has("off")) {
        (true, _) => ZoomMode::On,
        (_, true) => ZoomMode::Off,
        _ => ZoomMode::Toggle,
    };
    Ok(Request::PaneZoom { pane, mode })
}

fn pane_rename(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "pane rename <id> <label> | pane rename <id> --clear";
    let a = scan(rest, &[flag("clear")])?;
    let words = a.words();
    if a.has("clear") {
        check_count(&words, 1, 1, USAGE)?;
        return Ok(Request::PaneRename {
            pane: pane_id(&words[0])?,
            label: None,
        });
    }
    check_count(&words, 2, 2, USAGE)?;
    if words[1].is_empty() {
        return Err("pane label must not be empty (use --clear to remove it)".into());
    }
    Ok(Request::PaneRename {
        pane: pane_id(&words[0])?,
        label: Some(words[1].clone()),
    })
}

fn pane_close(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    Ok(Request::PaneClose {
        pane: one_id(&words, "pane close <id>")?,
    })
}

fn pane_read(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "pane read <id> [--source visible|recent] [--lines N]";
    let a = scan(rest, READ_OPTS)?;
    Ok(Request::PaneRead {
        pane: one_id(&a.words(), USAGE)?,
        source: read_source(&a)?,
        lines: lines_opt(&a)?,
    })
}

fn pane_send_text(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    check_count(&words, 2, 2, "pane send-text <id> <text>")?;
    Ok(Request::PaneSendText {
        pane: pane_id(&words[0])?,
        text: words[1].clone(),
    })
}

fn pane_send_keys(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "pane send-keys <id> <key> [key...]";
    let words = no_opts(rest)?;
    check_count(&words, 2, usize::MAX, USAGE)?;
    Ok(Request::PaneSendKeys {
        pane: pane_id(&words[0])?,
        keys: words[1..].to_vec(),
    })
}

fn pane_run(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    check_count(&words, 2, 2, "pane run <id> <command>")?;
    Ok(Request::PaneRun {
        pane: pane_id(&words[0])?,
        command: words[1].clone(),
    })
}

fn pane_wait_output(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "pane wait-output <id> (--match TEXT | --regex PAT) [--source S] [--lines N] [--timeout MS]";
    let a = scan(rest, WAIT_OUTPUT_OPTS)?;
    a.exclusive(&["match", "regex"])?;
    let pattern = match (a.get("match"), a.get("regex")) {
        (Some(text), _) => Pattern::Match(text.to_string()),
        (None, Some(pat)) => Pattern::Regex(regex_arg(pat)?),
        (None, None) => return Err(format!("usage: tmuxlike {USAGE}")),
    };
    Ok(Request::PaneWaitOutput {
        pane: one_id(&a.words(), USAGE)?,
        pattern,
        source: read_source(&a)?,
        lines: lines_opt(&a)?,
        timeout_ms: opt_num(&a, "timeout")?,
    })
}

fn pane_report_agent(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "pane report-agent <id> --source ID --agent LABEL --state idle|working|blocked|unknown [--message TEXT] [--seq N] [-- RESUME...]";
    let a = scan(rest, REPORT_OPTS)?;
    check_count(&a.pos, 1, 1, USAGE)?;
    Ok(Request::PaneReportAgent {
        pane: pane_id(&a.pos[0])?,
        source: source_id(required(&a, "source")?)?,
        agent: agent_label(required(&a, "agent")?)?,
        state: report_state(required(&a, "state")?)?,
        message: a.get("message").map(str::to_string),
        seq: opt_num(&a, "seq")?,
        resume: resume_cmd(&a)?,
    })
}

fn pane_release_agent(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "pane release-agent <id> --source ID --agent LABEL [--seq N]";
    let a = scan(rest, RELEASE_OPTS)?;
    Ok(Request::PaneReleaseAgent {
        pane: one_id(&a.words(), USAGE)?,
        source: source_id(required(&a, "source")?)?,
        agent: agent_label(required(&a, "agent")?)?,
        seq: opt_num(&a, "seq")?,
    })
}
