//! argv -> `Request` for the herdr-style CLI (`tmuxlike pane ...`, `tmuxlike agent ...`).
//! OWNER: worker H1. Pure: no I/O, no environment access (caller passes `current`).
use super::{
    Direction, Pattern, ReadSource, Request, Status, Target, WaitSpec, ZoomMode, parse_pane_id,
};
use std::str::FromStr;

/// Top-level words routed to this parser by `main.rs`.
pub const API_COMMANDS: &[&str] = &["pane", "agent", "api", "session", "server"];

/// Usage text for the API commands (printed by `tmuxlike <group> --help`).
pub const HELP: &str = r#"tmuxlike pane|agent|api|session|server  (herdr-style JSON automation)

Pane ids: p<N> (tmux-style %<N> also accepted). Pane TARGET is one of:
  <id>  |  --pane ID  |  --current   (--current = $TMUXLIKE_PANE_ID inside a pane)
Omitted pane targets use the focused pane of the most recently used session.
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

<target> = pane id or agent name [a-z][a-z0-9_-]{0,31}.
--until S: idle|working|blocked|done|unknown; repeatable (default: idle, done, blocked).
--timeout MS: milliseconds; omitted = wait without limit."#;

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
const AGENT_SUBS: &[&str] = &[
    "list",
    "get",
    "read",
    "send-keys",
    "focus",
    "prompt",
    "wait",
    "rename",
];
const AGENT_NAME_MAX: usize = 32;
const LABEL_MAX: usize = 80;
const SOURCE_MAX: usize = 80;

/// Parse `args` (starting at the group word, e.g. `["pane", "list"]`).
/// `current` is the caller's pane id from `$TMUXLIKE_PANE_ID`, used by `--current`.
/// Err = usage error message (CLI exits with status 2).
pub fn parse(args: &[String], current: Option<usize>) -> Result<Request, String> {
    let [group, sub, rest @ ..] = args else {
        return Err(HELP.to_string());
    };
    if is_help_word(sub) || wants_help(rest) {
        return Err(HELP.to_string());
    }
    match group.as_str() {
        "pane" => pane(sub, rest, current),
        "agent" => agent(sub, rest),
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

fn unknown_sub(group: &str, sub: &str, subs: &[&str]) -> String {
    format!(
        "unknown {group} subcommand '{sub}'; expected one of: {}",
        subs.join(", ")
    )
}

// ---------------------------------------------------------------- option scanner

/// One accepted option. `value` = takes an argument; `repeat` = may appear many times.
#[derive(Clone, Copy)]
struct Spec {
    name: &'static str,
    value: bool,
    repeat: bool,
}

const fn flag(name: &'static str) -> Spec {
    Spec {
        name,
        value: false,
        repeat: false,
    }
}

const fn val(name: &'static str) -> Spec {
    Spec {
        name,
        value: true,
        repeat: false,
    }
}

const fn many(name: &'static str) -> Spec {
    Spec {
        name,
        value: true,
        repeat: true,
    }
}

const SPLIT_OPTS: &[Spec] = &[
    val("pane"),
    flag("current"),
    val("direction"),
    val("ratio"),
    val("cwd"),
    flag("focus"),
    flag("no-focus"),
];
const READ_OPTS: &[Spec] = &[val("source"), val("lines")];
const WAIT_OUTPUT_OPTS: &[Spec] = &[
    val("match"),
    val("regex"),
    val("source"),
    val("lines"),
    val("timeout"),
];
const REPORT_OPTS: &[Spec] = &[
    val("source"),
    val("agent"),
    val("state"),
    val("message"),
    val("seq"),
];
const RELEASE_OPTS: &[Spec] = &[val("source"), val("agent"), val("seq")];
const PROMPT_OPTS: &[Spec] = &[flag("wait"), many("until"), val("timeout")];
const WAIT_OPTS: &[Spec] = &[many("until"), val("timeout")];

/// Scanner result. Words are kept exactly as typed; nothing is trimmed or re-split.
#[derive(Default)]
struct Args {
    /// positional words seen before any `--`
    pos: Vec<String>,
    /// accepted options in order: (name, value); flags carry an empty value
    opts: Vec<(&'static str, String)>,
    /// words after a literal `--` (None when `--` was not given)
    tail: Option<Vec<String>>,
}

impl Args {
    fn get(&self, name: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    fn values(&self, name: &str) -> Vec<&str> {
        self.opts
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn has(&self, name: &str) -> bool {
        self.opts.iter().any(|(n, _)| *n == name)
    }

    fn exclusive(&self, names: &[&str]) -> Result<(), String> {
        if names.iter().filter(|n| self.has(n)).count() > 1 {
            let list: Vec<String> = names.iter().map(|n| format!("--{n}")).collect();
            return Err(format!("use only one of: {}", list.join(" ")));
        }
        Ok(())
    }

    /// Positionals followed by the words after `--`.
    fn words(&self) -> Vec<String> {
        let mut words = self.pos.clone();
        words.extend(self.tail.iter().flatten().cloned());
        words
    }
}

/// Scan `argv` against `specs`. Options may appear anywhere before `--`.
fn scan(argv: &[String], specs: &[Spec]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let tok = argv[i].as_str();
        i += 1;
        if tok == "--" {
            a.tail = Some(argv[i..].to_vec());
            break;
        }
        let Some(body) = tok.strip_prefix("--") else {
            if tok.len() > 1 && tok.starts_with('-') {
                return Err(format!("unknown option: {tok}"));
            }
            a.pos.push(tok.to_string());
            continue;
        };
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (body, None),
        };
        let spec = specs
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("unknown option: --{name}"))?;
        if !spec.repeat && a.has(spec.name) {
            return Err(format!("duplicate option: --{name}"));
        }
        let value = option_value(spec, inline, argv, &mut i)?;
        a.opts.push((spec.name, value));
    }
    Ok(a)
}

/// Value for one option: `--x=v` inline, or the next word (which must not look like `--opt`).
fn option_value(
    spec: &Spec,
    inline: Option<String>,
    argv: &[String],
    i: &mut usize,
) -> Result<String, String> {
    match (spec.value, inline) {
        (false, None) => Ok(String::new()),
        (false, Some(_)) => Err(format!("option --{} takes no value", spec.name)),
        (true, Some(v)) => Ok(v),
        (true, None) => match argv.get(*i).map(String::as_str) {
            Some(next) if next != "--" && !next.starts_with("--") => {
                *i += 1;
                Ok(next.to_string())
            }
            _ => Err(format!("missing value for --{}", spec.name)),
        },
    }
}

// ---------------------------------------------------------------- shared helpers

fn check_count(words: &[String], min: usize, max: usize, usage: &str) -> Result<(), String> {
    if (min..=max).contains(&words.len()) {
        Ok(())
    } else {
        Err(format!("usage: tmuxlike {usage}"))
    }
}

/// Exactly one word, returned as typed.
fn single<'a>(words: &'a [String], usage: &str) -> Result<&'a str, String> {
    check_count(words, 1, 1, usage)?;
    Ok(words[0].as_str())
}

/// Options must be absent; returns the words (positionals + after `--`).
fn no_opts(rest: &[String]) -> Result<Vec<String>, String> {
    Ok(scan(rest, &[])?.words())
}

fn none(a: &Args, usage: &str) -> Result<(), String> {
    check_count(&a.words(), 0, 0, usage)
}

fn pane_id(s: &str) -> Result<usize, String> {
    parse_pane_id(s).ok_or_else(|| format!("invalid pane id '{s}' (expected p<N> or %<N>)"))
}

fn one_id(words: &[String], usage: &str) -> Result<usize, String> {
    pane_id(single(words, usage)?)
}

fn required<'a>(a: &'a Args, name: &str) -> Result<&'a str, String> {
    a.get(name)
        .ok_or_else(|| format!("missing required --{name}"))
}

fn opt_num<T: FromStr>(a: &Args, name: &str) -> Result<Option<T>, String> {
    a.get(name)
        .map(|v| {
            v.parse::<T>()
                .map_err(|_| format!("--{name} must be a number, got '{v}'"))
        })
        .transpose()
}

fn lines_opt(a: &Args) -> Result<Option<usize>, String> {
    match opt_num::<usize>(a, "lines")? {
        Some(0) => Err("--lines must be at least 1".into()),
        n => Ok(n),
    }
}

fn ratio_opt(a: &Args) -> Result<Option<f32>, String> {
    match opt_num::<f32>(a, "ratio")? {
        Some(r) if r > 0.0 && r < 1.0 => Ok(Some(r)),
        Some(_) => Err("--ratio must be strictly between 0 and 1".into()),
        None => Ok(None),
    }
}

fn read_source(a: &Args) -> Result<ReadSource, String> {
    match a.get("source") {
        None | Some("recent") => Ok(ReadSource::Recent),
        Some("visible") => Ok(ReadSource::Visible),
        Some(other) => Err(format!("invalid --source '{other}' (visible|recent)")),
    }
}

fn is_agent_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    let starts_ok = matches!(bytes.next(), Some(c) if c.is_ascii_lowercase());
    starts_ok
        && s.len() <= AGENT_NAME_MAX
        && bytes.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

fn agent_target(s: &str) -> Result<Target, String> {
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

fn source_id(s: &str) -> Result<String, String> {
    if is_source_id(s) {
        Ok(s.to_string())
    } else {
        Err(format!("invalid --source '{s}': 1-80 of A-Za-z0-9 : . _ -"))
    }
}

fn agent_label(s: &str) -> Result<String, String> {
    if (1..=LABEL_MAX).contains(&s.chars().count()) {
        Ok(s.to_string())
    } else {
        Err("--agent LABEL must be 1-80 characters".into())
    }
}

fn report_state(s: &str) -> Result<Status, String> {
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

fn wait_spec(a: &Args) -> Result<WaitSpec, String> {
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

fn regex_arg(pat: &str) -> Result<String, String> {
    regex::Regex::new(pat).map_err(|e| format!("invalid --regex: {e}"))?;
    Ok(pat.to_string())
}

/// `-- CMD...` after report-agent: must be present with at least one word when given.
fn resume_cmd(a: &Args) -> Result<Vec<String>, String> {
    match &a.tail {
        None => Ok(Vec::new()),
        Some(words) if words.is_empty() => {
            Err("'--' must be followed by the resume command".into())
        }
        Some(words) => Ok(words.clone()),
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

// ---------------------------------------------------------------- group dispatch

fn pane(sub: &str, rest: &[String], current: Option<usize>) -> Result<Request, String> {
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

fn agent(sub: &str, rest: &[String]) -> Result<Request, String> {
    match sub {
        "list" => agent_list(rest),
        "get" => agent_get(rest),
        "read" => agent_read(rest),
        "send-keys" => agent_send_keys(rest),
        "focus" => agent_focus(rest),
        "prompt" => agent_prompt(rest),
        "wait" => agent_wait(rest),
        "rename" => agent_rename(rest),
        _ => Err(unknown_sub("agent", sub, AGENT_SUBS)),
    }
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

// ---------------------------------------------------------------- pane commands

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
        pane: target_of(&a, None, current)?,
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
    let pane = target_of(&a, words.first().map(String::as_str), current)?;
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

// ---------------------------------------------------------------- agent commands

fn agent_list(rest: &[String]) -> Result<Request, String> {
    check_count(&no_opts(rest)?, 0, 0, "agent list")?;
    Ok(Request::AgentList)
}

fn agent_get(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    Ok(Request::AgentGet {
        target: agent_target(single(&words, "agent get <target>")?)?,
    })
}

fn agent_read(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent read <target> [--source visible|recent] [--lines N]";
    let a = scan(rest, READ_OPTS)?;
    let words = a.words();
    Ok(Request::AgentRead {
        target: agent_target(single(&words, USAGE)?)?,
        source: read_source(&a)?,
        lines: lines_opt(&a)?,
    })
}

fn agent_send_keys(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent send-keys <target> <key>...";
    let words = no_opts(rest)?;
    check_count(&words, 2, usize::MAX, USAGE)?;
    Ok(Request::AgentSendKeys {
        target: agent_target(&words[0])?,
        keys: words[1..].to_vec(),
    })
}

fn agent_focus(rest: &[String]) -> Result<Request, String> {
    let words = no_opts(rest)?;
    Ok(Request::AgentFocus {
        target: agent_target(single(&words, "agent focus <target>")?)?,
    })
}

fn agent_prompt(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent prompt <target> <text> [--wait [--until S]... [--timeout MS]]";
    let a = scan(rest, PROMPT_OPTS)?;
    let words = a.words();
    check_count(&words, 2, 2, USAGE)?;
    let wait = match (a.has("wait"), a.has("until") || a.has("timeout")) {
        (true, _) => Some(wait_spec(&a)?),
        (false, true) => return Err("--until and --timeout need --wait".into()),
        (false, false) => None,
    };
    Ok(Request::AgentPrompt {
        target: agent_target(&words[0])?,
        text: words[1].clone(),
        wait,
    })
}

fn agent_wait(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent wait <target> [--until S]... [--timeout MS]";
    let a = scan(rest, WAIT_OPTS)?;
    let words = a.words();
    Ok(Request::AgentWait {
        target: agent_target(single(&words, USAGE)?)?,
        wait: wait_spec(&a)?,
    })
}

fn agent_rename(rest: &[String]) -> Result<Request, String> {
    const USAGE: &str = "agent rename <target> <name> | agent rename <target> --clear";
    let a = scan(rest, &[flag("clear")])?;
    let words = a.words();
    if a.has("clear") {
        check_count(&words, 1, 1, USAGE)?;
        return Ok(Request::AgentRename {
            target: agent_target(&words[0])?,
            name: None,
        });
    }
    check_count(&words, 2, 2, USAGE)?;
    if !is_agent_name(&words[1]) {
        return Err(format!(
            "invalid agent name '{}' (matching [a-z][a-z0-9_-]{{0,31}})",
            words[1]
        ));
    }
    Ok(Request::AgentRename {
        target: agent_target(&words[0])?,
        name: Some(words[1].clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    fn run(xs: &[&str]) -> Result<Request, String> {
        parse(&v(xs), None)
    }

    fn run_at(xs: &[&str], current: Option<usize>) -> Result<Request, String> {
        parse(&v(xs), current)
    }

    fn is_err(xs: &[&str]) -> bool {
        run(xs).is_err()
    }

    /// `pane report-agent p4 --source s --agent a` plus caller-supplied extras.
    fn report(extra: &[&str]) -> Result<Request, String> {
        let mut xs = vec![
            "pane",
            "report-agent",
            "p4",
            "--source",
            "s",
            "--agent",
            "a",
        ];
        xs.extend_from_slice(extra);
        run(&xs)
    }

    #[test]
    fn help_and_missing_subcommand_return_help_text() {
        assert_eq!(run(&["pane"]), Err(HELP.to_string()));
        assert_eq!(run(&["agent", "--help"]), Err(HELP.to_string()));
        assert_eq!(run(&["server", "-h"]), Err(HELP.to_string()));
        assert_eq!(run(&["session", "help"]), Err(HELP.to_string()));
        assert_eq!(run(&["pane", "list", "--help"]), Err(HELP.to_string()));
    }

    #[test]
    fn unknown_group_and_subcommand_are_usage_errors() {
        assert!(run(&["nope", "list"]).unwrap_err().contains("group"));
        let e = run(&["pane", "frob"]).unwrap_err();
        assert!(e.contains("frob") && e.contains("report-agent"), "{e}");
        assert!(run(&["agent", "frob"]).unwrap_err().contains("prompt"));
        assert!(is_err(&["api", "stop"]));
        assert!(is_err(&["server", "start"]));
    }

    #[test]
    fn option_free_commands_parse() {
        assert_eq!(run(&["api", "snapshot"]), Ok(Request::ApiSnapshot));
        assert_eq!(run(&["session", "list"]), Ok(Request::SessionList));
        assert_eq!(
            run(&["session", "list", "--json"]),
            Ok(Request::SessionList)
        );
        assert_eq!(run(&["server", "stop"]), Ok(Request::ServerStop));
        assert_eq!(run(&["agent", "list"]), Ok(Request::AgentList));
        assert!(is_err(&["server", "stop", "now"]));
        assert!(is_err(&["api", "snapshot", "--json"]));
        assert!(is_err(&["agent", "list", "x"]));
    }

    #[test]
    fn pane_list_session_option() {
        assert_eq!(
            run(&["pane", "list"]),
            Ok(Request::PaneList { session: None })
        );
        assert_eq!(
            run(&["pane", "list", "--session", "work"]),
            Ok(Request::PaneList {
                session: Some("work".into())
            })
        );
        assert_eq!(
            run(&["pane", "list", "--session=a=b=c"]),
            Ok(Request::PaneList {
                session: Some("a=b=c".into())
            })
        );
        assert!(is_err(&["pane", "list", "--session="]));
        assert!(is_err(&["pane", "list", "extra"]));
    }

    #[test]
    fn single_id_commands_accept_p_and_percent_forms() {
        assert_eq!(
            run(&["pane", "get", "p12"]),
            Ok(Request::PaneGet { pane: 12 })
        );
        assert_eq!(
            run(&["pane", "get", "%7"]),
            Ok(Request::PaneGet { pane: 7 })
        );
        assert_eq!(
            run(&["pane", "close", "p3"]),
            Ok(Request::PaneClose { pane: 3 })
        );
        assert_eq!(
            run(&["pane", "focus", "p4"]),
            Ok(Request::PaneFocus { pane: 4 })
        );
    }

    #[test]
    fn bad_pane_ids_and_extra_words_are_rejected() {
        for bad in ["p0", "w1:p1", "42", "p01", "p"] {
            assert!(is_err(&["pane", "get", bad]), "{bad}");
        }
        assert!(is_err(&["pane", "get"]));
        assert!(is_err(&["pane", "get", "p1", "p2"]));
        assert!(is_err(&["pane", "get", "p1", "--", "x"]));
        assert!(is_err(&["pane", "close", "p1", "--force"]));
    }

    #[test]
    fn current_flag_uses_caller_pane_or_fails() {
        assert_eq!(
            run_at(&["pane", "current", "--current"], Some(4)),
            Ok(Request::PaneCurrent { pane: Some(4) })
        );
        assert!(run_at(&["pane", "current", "--current"], None).is_err());
        assert_eq!(
            run_at(&["pane", "current"], Some(4)),
            Ok(Request::PaneCurrent { pane: Some(4) })
        );
        assert_eq!(
            run_at(&["pane", "current"], None),
            Ok(Request::PaneCurrent { pane: None })
        );
        assert_eq!(
            run_at(&["pane", "current", "--pane", "p2"], None),
            Ok(Request::PaneCurrent { pane: Some(2) })
        );
        assert!(run_at(&["pane", "current", "--pane", "p2", "--current"], Some(4)).is_err());
    }

    #[test]
    fn split_defaults_and_all_options() {
        assert_eq!(
            run(&["pane", "split", "--direction", "right"]),
            Ok(Request::PaneSplit {
                pane: None,
                direction: Direction::Right,
                ratio: None,
                cwd: None,
                focus: false,
            })
        );
        assert_eq!(
            run_at(&["pane", "split", "--direction", "down"], Some(5)),
            Ok(Request::PaneSplit {
                pane: Some(5),
                direction: Direction::Down,
                ratio: None,
                cwd: None,
                focus: false,
            })
        );
        assert_eq!(
            run(&[
                "pane",
                "split",
                "p9",
                "--direction=down",
                "--ratio=0.3",
                "--cwd",
                "/tmp/x y",
                "--focus"
            ]),
            Ok(Request::PaneSplit {
                pane: Some(9),
                direction: Direction::Down,
                ratio: Some(0.3),
                cwd: Some("/tmp/x y".into()),
                focus: true,
            })
        );
        assert_eq!(
            run(&[
                "pane",
                "split",
                "--pane",
                "p2",
                "--direction",
                "right",
                "--no-focus"
            ]),
            Ok(Request::PaneSplit {
                pane: Some(2),
                direction: Direction::Right,
                ratio: None,
                cwd: None,
                focus: false,
            })
        );
    }

    #[test]
    fn split_rejects_left_up_missing_direction_and_double_targets() {
        assert!(is_err(&["pane", "split"]));
        assert!(is_err(&["pane", "split", "--direction", "left"]));
        assert!(is_err(&["pane", "split", "--direction", "up"]));
        assert!(is_err(&["pane", "split", "--direction", "sideways"]));
        assert!(is_err(&[
            "pane",
            "split",
            "p1",
            "p2",
            "--direction",
            "right"
        ]));
        assert!(is_err(&[
            "pane",
            "split",
            "p1",
            "--pane",
            "p2",
            "--direction",
            "right"
        ]));
        assert!(is_err(&[
            "pane",
            "split",
            "--direction",
            "right",
            "--focus",
            "--no-focus"
        ]));
        assert!(is_err(&[
            "pane",
            "split",
            "--direction",
            "right",
            "--direction",
            "down"
        ]));
        assert!(
            run_at(
                &["pane", "split", "--direction", "right", "--current"],
                None
            )
            .is_err()
        );
    }

    #[test]
    fn ratio_must_be_strictly_between_zero_and_one() {
        for good in ["0.001", "0.5", "0.999"] {
            assert!(
                run(&["pane", "split", "--direction", "right", "--ratio", good]).is_ok(),
                "{good}"
            );
        }
        for bad in ["0", "1", "-0.1", "1.5", "NaN", "inf", "half"] {
            assert!(
                is_err(&["pane", "split", "--direction", "right", "--ratio", bad]),
                "{bad}"
            );
        }
    }

    #[test]
    fn zoom_modes_and_targets() {
        let zoom = |pane, mode| Ok(Request::PaneZoom { pane, mode });
        assert_eq!(run(&["pane", "zoom"]), zoom(None, ZoomMode::Toggle));
        assert_eq!(
            run(&["pane", "zoom", "p3", "--on"]),
            zoom(Some(3), ZoomMode::On)
        );
        assert_eq!(
            run(&["pane", "zoom", "--off", "--pane", "p5"]),
            zoom(Some(5), ZoomMode::Off)
        );
        assert_eq!(
            run(&["pane", "zoom", "--toggle"]),
            zoom(None, ZoomMode::Toggle)
        );
        assert_eq!(
            run_at(&["pane", "zoom", "--current", "--on"], Some(6)),
            zoom(Some(6), ZoomMode::On)
        );
        assert!(is_err(&["pane", "zoom", "--on", "--off"]));
        assert!(run_at(&["pane", "zoom", "p1", "--current"], Some(3)).is_err());
        assert!(run_at(&["pane", "zoom", "--current"], None).is_err());
    }

    #[test]
    fn focus_by_direction_and_by_id() {
        assert_eq!(
            run(&["pane", "focus", "--direction", "left"]),
            Ok(Request::PaneFocusDirection {
                pane: None,
                direction: Direction::Left
            })
        );
        assert_eq!(
            run_at(
                &["pane", "focus", "--direction", "up", "--current"],
                Some(2)
            ),
            Ok(Request::PaneFocusDirection {
                pane: Some(2),
                direction: Direction::Up
            })
        );
        assert_eq!(
            run(&["pane", "focus", "--direction", "down", "--pane", "p5"]),
            Ok(Request::PaneFocusDirection {
                pane: Some(5),
                direction: Direction::Down
            })
        );
        assert!(is_err(&["pane", "focus", "--direction", "diagonal"]));
        assert!(is_err(&[
            "pane",
            "focus",
            "--direction",
            "left",
            "--current"
        ]));
        assert!(is_err(&["pane", "focus", "--pane", "p1"]));
        assert!(is_err(&["pane", "focus", "p1", "--direction", "left"]));
    }

    #[test]
    fn rename_sets_label_or_clears_it() {
        assert_eq!(
            run(&["pane", "rename", "p1", "工作 區 my label"]),
            Ok(Request::PaneRename {
                pane: 1,
                label: Some("工作 區 my label".into())
            })
        );
        assert_eq!(
            run(&["pane", "rename", "p1", "--clear"]),
            Ok(Request::PaneRename {
                pane: 1,
                label: None
            })
        );
        assert!(is_err(&["pane", "rename", "p1"]));
        assert!(is_err(&["pane", "rename", "p1", "--clear", "x"]));
        assert!(is_err(&["pane", "rename", "p1", ""]));
        assert!(is_err(&["pane", "rename", "p1", "a", "b"]));
    }

    #[test]
    fn read_defaults_source_and_lines() {
        assert_eq!(
            run(&["pane", "read", "p1"]),
            Ok(Request::PaneRead {
                pane: 1,
                source: ReadSource::Recent,
                lines: None
            })
        );
        assert_eq!(
            run(&["pane", "read", "p1", "--source", "visible", "--lines", "5"]),
            Ok(Request::PaneRead {
                pane: 1,
                source: ReadSource::Visible,
                lines: Some(5)
            })
        );
        assert_eq!(
            run(&["agent", "read", "claude-1", "--lines=3"]),
            Ok(Request::AgentRead {
                target: Target::Name("claude-1".into()),
                source: ReadSource::Recent,
                lines: Some(3),
            })
        );
        assert!(is_err(&["pane", "read", "p1", "--lines", "0"]));
        assert!(is_err(&["pane", "read", "p1", "--lines=abc"]));
        assert!(is_err(&["pane", "read", "p1", "--source", "bogus"]));
        assert!(is_err(&[
            "pane", "read", "p1", "--source", "recent", "--source", "visible"
        ]));
    }

    #[test]
    fn send_text_and_run_keep_text_byte_for_byte() {
        let text = "  say \"hi\" \\n\\t  \nline2\r\n 你好 世界  ";
        assert_eq!(
            run(&["pane", "send-text", "p1", text]),
            Ok(Request::PaneSendText {
                pane: 1,
                text: text.into()
            })
        );
        assert_eq!(
            run(&["pane", "send-text", "p1", "--", "-rf"]),
            Ok(Request::PaneSendText {
                pane: 1,
                text: "-rf".into()
            })
        );
        assert_eq!(
            run(&["pane", "send-text", "p1", "--", "--text=x"]),
            Ok(Request::PaneSendText {
                pane: 1,
                text: "--text=x".into()
            })
        );
        assert_eq!(
            run(&["pane", "run", "p2", "--", "echo \"a\" 'b' \\ c"]),
            Ok(Request::PaneRun {
                pane: 2,
                command: "echo \"a\" 'b' \\ c".into()
            })
        );
        assert!(is_err(&["pane", "send-text", "p1"]));
        assert!(is_err(&["pane", "send-text", "p1", "a", "b"]));
        assert!(is_err(&["pane", "send-text", "p1", "-rf"]));
        assert!(is_err(&["pane", "run", "p1"]));
    }

    #[test]
    fn send_keys_needs_at_least_one_key() {
        assert_eq!(
            run(&["pane", "send-keys", "p1", "Enter", "C-c"]),
            Ok(Request::PaneSendKeys {
                pane: 1,
                keys: vec!["Enter".into(), "C-c".into()]
            })
        );
        assert_eq!(
            run(&["agent", "send-keys", "claude", "Up"]),
            Ok(Request::AgentSendKeys {
                target: Target::Name("claude".into()),
                keys: vec!["Up".into()],
            })
        );
        assert_eq!(
            run(&["agent", "send-keys", "p3", "-", "Esc"]),
            Ok(Request::AgentSendKeys {
                target: Target::Pane(3),
                keys: vec!["-".into(), "Esc".into()],
            })
        );
        assert!(is_err(&["pane", "send-keys", "p1"]));
        assert!(is_err(&["agent", "send-keys", "claude"]));
    }

    #[test]
    fn wait_output_needs_exactly_one_pattern() {
        assert_eq!(
            run(&["pane", "wait-output", "p1", "--match", "$ "]),
            Ok(Request::PaneWaitOutput {
                pane: 1,
                pattern: Pattern::Match("$ ".into()),
                source: ReadSource::Recent,
                lines: None,
                timeout_ms: None,
            })
        );
        assert_eq!(
            run(&[
                "pane",
                "wait-output",
                "p1",
                "--regex",
                "^ok\\d+$",
                "--source",
                "visible",
                "--lines",
                "20",
                "--timeout",
                "1500"
            ]),
            Ok(Request::PaneWaitOutput {
                pane: 1,
                pattern: Pattern::Regex("^ok\\d+$".into()),
                source: ReadSource::Visible,
                lines: Some(20),
                timeout_ms: Some(1500),
            })
        );
        assert!(is_err(&["pane", "wait-output", "p1"]));
        assert!(is_err(&[
            "pane",
            "wait-output",
            "p1",
            "--match",
            "a",
            "--regex",
            "b"
        ]));
        assert!(is_err(&["pane", "wait-output", "p1", "--regex", "("]));
        assert!(is_err(&[
            "pane",
            "wait-output",
            "p1",
            "--match",
            "a",
            "--timeout",
            "soon"
        ]));
    }

    #[test]
    fn report_agent_parses_message_seq_and_resume() {
        assert_eq!(
            report(&["--state", "working"]),
            Ok(Request::PaneReportAgent {
                pane: 4,
                source: "s".into(),
                agent: "a".into(),
                state: Status::Working,
                message: None,
                seq: None,
                resume: vec![],
            })
        );
        let text = "  Waiting: ok -- sure\n";
        assert_eq!(
            report(&[
                "--state",
                "blocked",
                "--message",
                text,
                "--seq",
                "7",
                "--",
                "claude",
                "--resume",
                "--last"
            ]),
            Ok(Request::PaneReportAgent {
                pane: 4,
                source: "s".into(),
                agent: "a".into(),
                state: Status::Blocked,
                message: Some(text.into()),
                seq: Some(7),
                resume: vec!["claude".into(), "--resume".into(), "--last".into()],
            })
        );
        assert!(report(&["--state", "idle", "--message=--x"]).is_ok());
    }

    #[test]
    fn report_agent_rejects_bad_fields() {
        assert!(report(&["--state", "idle", "--"]).is_err());
        assert!(report(&["--state", "done"]).is_err());
        assert!(report(&["--state", "sleeping"]).is_err());
        assert!(report(&["--seq", "x", "--state", "idle"]).is_err());
        assert!(is_err(&[
            "pane",
            "report-agent",
            "p4",
            "--source",
            "bad source",
            "--agent",
            "a",
            "--state",
            "idle"
        ]));
        assert!(is_err(&[
            "pane",
            "report-agent",
            "p4",
            "--source",
            "",
            "--agent",
            "a",
            "--state",
            "idle"
        ]));
        assert!(is_err(&[
            "pane",
            "report-agent",
            "p4",
            "--agent",
            "a",
            "--state",
            "idle"
        ]));
        assert!(is_err(&[
            "pane",
            "report-agent",
            "p4",
            "--source",
            "s",
            "--agent",
            "",
            "--state",
            "idle"
        ]));
        let long = "x".repeat(81);
        assert!(is_err(&[
            "pane",
            "report-agent",
            "p4",
            "--source",
            "s",
            "--agent",
            long.as_str(),
            "--state",
            "idle"
        ]));
        let edge = "x".repeat(80);
        assert!(
            run(&[
                "pane",
                "report-agent",
                "p4",
                "--source",
                "s",
                "--agent",
                edge.as_str(),
                "--state",
                "idle"
            ])
            .is_ok()
        );
    }

    #[test]
    fn release_agent_parses() {
        assert_eq!(
            run(&[
                "pane",
                "release-agent",
                "p4",
                "--source",
                "hook:claude",
                "--agent",
                "claude"
            ]),
            Ok(Request::PaneReleaseAgent {
                pane: 4,
                source: "hook:claude".into(),
                agent: "claude".into(),
                seq: None,
            })
        );
        assert_eq!(
            run(&[
                "pane",
                "release-agent",
                "p4",
                "--source",
                "s",
                "--agent",
                "a",
                "--seq=9"
            ]),
            Ok(Request::PaneReleaseAgent {
                pane: 4,
                source: "s".into(),
                agent: "a".into(),
                seq: Some(9)
            })
        );
        assert!(is_err(&["pane", "release-agent", "p4", "--source", "s"]));
        assert!(is_err(&[
            "pane",
            "release-agent",
            "p4",
            "--source",
            "s",
            "--agent",
            "a",
            "--",
            "x"
        ]));
    }

    #[test]
    fn agent_target_is_pane_id_or_name() {
        assert_eq!(
            run(&["agent", "get", "p3"]),
            Ok(Request::AgentGet {
                target: Target::Pane(3)
            })
        );
        assert_eq!(
            run(&["agent", "get", "%8"]),
            Ok(Request::AgentGet {
                target: Target::Pane(8)
            })
        );
        assert_eq!(
            run(&["agent", "get", "claude-1_b"]),
            Ok(Request::AgentGet {
                target: Target::Name("claude-1_b".into())
            })
        );
        assert_eq!(
            run(&["agent", "get", "p0"]),
            Ok(Request::AgentGet {
                target: Target::Name("p0".into())
            })
        );
        let max = "a".repeat(32);
        assert!(run(&["agent", "get", max.as_str()]).is_ok());
        let too_long = "a".repeat(33);
        assert!(is_err(&["agent", "get", too_long.as_str()]));
        assert!(is_err(&["agent", "get", "42"]));
        assert!(is_err(&["agent", "get", "Claude"]));
        assert!(is_err(&["agent", "get", "1abc"]));
        assert!(is_err(&["agent", "get"]));
        assert!(is_err(&["agent", "get", "claude", "x"]));
    }

    #[test]
    fn agent_read_and_focus() {
        assert_eq!(
            run(&["agent", "read", "p2", "--source", "visible"]),
            Ok(Request::AgentRead {
                target: Target::Pane(2),
                source: ReadSource::Visible,
                lines: None
            })
        );
        assert_eq!(
            run(&["agent", "focus", "reviewer"]),
            Ok(Request::AgentFocus {
                target: Target::Name("reviewer".into())
            })
        );
        assert!(is_err(&["agent", "focus", "p1", "p2"]));
    }

    #[test]
    fn agent_prompt_wait_options() {
        assert_eq!(
            run(&["agent", "prompt", "claude", "hello world"]),
            Ok(Request::AgentPrompt {
                target: Target::Name("claude".into()),
                text: "hello world".into(),
                wait: None,
            })
        );
        assert_eq!(
            run(&["agent", "prompt", "p2", "hi", "--wait"]),
            Ok(Request::AgentPrompt {
                target: Target::Pane(2),
                text: "hi".into(),
                wait: Some(WaitSpec {
                    until: vec![],
                    timeout_ms: None
                }),
            })
        );
        assert_eq!(
            run(&[
                "agent",
                "prompt",
                "p2",
                "hi",
                "--wait",
                "--until",
                "idle",
                "--until=done",
                "--timeout",
                "5000"
            ]),
            Ok(Request::AgentPrompt {
                target: Target::Pane(2),
                text: "hi".into(),
                wait: Some(WaitSpec {
                    until: vec![Status::Idle, Status::Done],
                    timeout_ms: Some(5000),
                }),
            })
        );
        assert_eq!(
            run(&["agent", "prompt", "claude", "--", "--help me"]),
            Ok(Request::AgentPrompt {
                target: Target::Name("claude".into()),
                text: "--help me".into(),
                wait: None,
            })
        );
        assert!(is_err(&[
            "agent", "prompt", "claude", "hi", "--until", "idle"
        ]));
        assert!(is_err(&[
            "agent",
            "prompt",
            "claude",
            "hi",
            "--timeout",
            "10"
        ]));
        assert!(is_err(&[
            "agent", "prompt", "claude", "hi", "--wait", "--until", "sleeping"
        ]));
        assert!(is_err(&[
            "agent",
            "prompt",
            "claude",
            "hi",
            "--wait",
            "--timeout",
            "-5"
        ]));
    }

    #[test]
    fn agent_wait_defaults_to_empty_until() {
        assert_eq!(
            run(&["agent", "wait", "claude"]),
            Ok(Request::AgentWait {
                target: Target::Name("claude".into()),
                wait: WaitSpec {
                    until: vec![],
                    timeout_ms: None
                },
            })
        );
        assert_eq!(
            run(&[
                "agent",
                "wait",
                "p7",
                "--until",
                "working",
                "--timeout",
                "250"
            ]),
            Ok(Request::AgentWait {
                target: Target::Pane(7),
                wait: WaitSpec {
                    until: vec![Status::Working],
                    timeout_ms: Some(250)
                },
            })
        );
        assert!(is_err(&["agent", "wait"]));
        assert!(is_err(&["agent", "wait", "claude", "--until", "nope"]));
    }

    #[test]
    fn agent_rename_sets_or_clears_name() {
        assert_eq!(
            run(&["agent", "rename", "p3", "reviewer"]),
            Ok(Request::AgentRename {
                target: Target::Pane(3),
                name: Some("reviewer".into())
            })
        );
        assert_eq!(
            run(&["agent", "rename", "reviewer", "--clear"]),
            Ok(Request::AgentRename {
                target: Target::Name("reviewer".into()),
                name: None
            })
        );
        assert!(is_err(&["agent", "rename", "p3"]));
        assert!(is_err(&["agent", "rename", "p3", "Bad Name"]));
        assert!(is_err(&["agent", "rename", "p3", "x", "--clear"]));
    }

    #[test]
    fn unknown_and_duplicate_options_are_rejected() {
        assert!(is_err(&["pane", "list", "--bogus"]));
        assert!(is_err(&["pane", "list", "-s"]));
        assert!(is_err(&["pane", "list", "--session"]));
        assert!(is_err(&["pane", "list", "--session", "--other"]));
        assert!(is_err(&[
            "pane",
            "list",
            "--session",
            "a",
            "--session",
            "b"
        ]));
        assert!(is_err(&["pane", "get", "p1", "--json"]));
        assert!(is_err(&["session", "list", "--json=1"]));
        assert!(run(&["agent", "wait", "p1", "--until", "idle", "--until", "done"]).is_ok());
    }
}
