//! Validators shared by the pane and agent groups: word counts, pane ids, numbers, sources,
//! labels, agent targets and names, waits and the `-- RESUME` tail. Errors are usage messages.
use super::scan::Args;
use crate::api::{LABEL_MAX, ReadSource, Status, Target, WaitSpec, is_agent_name, parse_pane_id};
use std::str::FromStr;

/// Max chars of a reporter id (`--source`).
const SOURCE_MAX: usize = 80;

pub(super) fn unknown_sub(group: &str, sub: &str, subs: &[&str]) -> String {
    format!(
        "unknown {group} subcommand '{sub}'; expected one of: {}",
        subs.join(", ")
    )
}

pub(super) fn check_count(
    words: &[String],
    min: usize,
    max: usize,
    usage: &str,
) -> Result<(), String> {
    if (min..=max).contains(&words.len()) {
        Ok(())
    } else {
        Err(format!("usage: tmuxlike {usage}"))
    }
}

/// Exactly one word, returned as typed.
pub(super) fn single<'a>(words: &'a [String], usage: &str) -> Result<&'a str, String> {
    check_count(words, 1, 1, usage)?;
    Ok(words[0].as_str())
}

/// Options and words must be absent.
pub(super) fn none(a: &Args, usage: &str) -> Result<(), String> {
    check_count(&a.words(), 0, 0, usage)
}

pub(super) fn pane_id(s: &str) -> Result<usize, String> {
    parse_pane_id(s).ok_or_else(|| format!("invalid pane id '{s}' (expected p<N> or %<N>)"))
}

pub(super) fn one_id(words: &[String], usage: &str) -> Result<usize, String> {
    pane_id(single(words, usage)?)
}

pub(super) fn required<'a>(a: &'a Args, name: &str) -> Result<&'a str, String> {
    a.get(name)
        .ok_or_else(|| format!("missing required --{name}"))
}

pub(super) fn opt_num<T: FromStr>(a: &Args, name: &str) -> Result<Option<T>, String> {
    a.get(name)
        .map(|v| {
            v.parse::<T>()
                .map_err(|_| format!("--{name} must be a number, got '{v}'"))
        })
        .transpose()
}

pub(super) fn lines_opt(a: &Args) -> Result<Option<usize>, String> {
    match opt_num::<usize>(a, "lines")? {
        Some(0) => Err("--lines must be at least 1".into()),
        n => Ok(n),
    }
}

pub(super) fn ratio_opt(a: &Args) -> Result<Option<f32>, String> {
    match opt_num::<f32>(a, "ratio")? {
        Some(r) if r > 0.0 && r < 1.0 => Ok(Some(r)),
        Some(_) => Err("--ratio must be strictly between 0 and 1".into()),
        None => Ok(None),
    }
}

pub(super) fn read_source(a: &Args) -> Result<ReadSource, String> {
    match a.get("source") {
        None | Some("recent") => Ok(ReadSource::Recent),
        Some("visible") => Ok(ReadSource::Visible),
        Some(other) => Err(format!("invalid --source '{other}' (visible|recent)")),
    }
}

/// Agent target: a pane id is tried first, then a live agent name (`is_agent_name` rejects
/// pane-shaped names, so the two forms never overlap).
pub(super) fn agent_target(s: &str) -> Result<Target, String> {
    if let Some(id) = parse_pane_id(s) {
        return Ok(Target::Pane(id));
    }
    if is_agent_name(s) {
        return Ok(Target::Name(s.to_string()));
    }
    Err(format!(
        "invalid agent target '{s}': use a pane id (p<N>) or a name matching [a-z][a-z0-9_-]{{0,31}}"
    ))
}

fn is_source_id(s: &str) -> bool {
    (1..=SOURCE_MAX).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b':' | b'.' | b'_' | b'-'))
}

pub(super) fn source_id(s: &str) -> Result<String, String> {
    if is_source_id(s) {
        Ok(s.to_string())
    } else {
        Err(format!("invalid --source '{s}': 1-80 of A-Za-z0-9 : . _ -"))
    }
}

pub(super) fn agent_label(s: &str) -> Result<String, String> {
    if (1..=LABEL_MAX).contains(&s.chars().count()) {
        Ok(s.to_string())
    } else {
        Err("--agent LABEL must be 1-80 characters".into())
    }
}

pub(super) fn report_state(s: &str) -> Result<Status, String> {
    match Status::parse(s) {
        Some(Status::Done) | None => Err(format!(
            "invalid --state '{s}' (idle|working|blocked|unknown)"
        )),
        Some(st) => Ok(st),
    }
}

fn status_arg(s: &str) -> Result<Status, String> {
    Status::parse(s)
        .ok_or_else(|| format!("invalid --until '{s}' (idle|working|blocked|done|unknown)"))
}

pub(super) fn wait_spec(a: &Args) -> Result<WaitSpec, String> {
    let until = a
        .values("until")
        .into_iter()
        .map(status_arg)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WaitSpec {
        until,
        timeout_ms: opt_num(a, "timeout")?,
    })
}

pub(super) fn regex_arg(pat: &str) -> Result<String, String> {
    regex::Regex::new(pat).map_err(|e| format!("invalid --regex: {e}"))?;
    Ok(pat.to_string())
}

/// `-- CMD...` after report-agent: must be present with at least one word when given.
pub(super) fn resume_cmd(a: &Args) -> Result<Vec<String>, String> {
    match &a.tail {
        None => Ok(Vec::new()),
        Some(words) if words.is_empty() => {
            Err("'--' must be followed by the resume command".into())
        }
        Some(words) => Ok(words.clone()),
    }
}
