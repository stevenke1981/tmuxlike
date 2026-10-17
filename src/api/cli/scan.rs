//! Option scanner: positional words, accepted `--opt` / `--opt=value` options and the words
//! after a literal `--`. Pure; nothing is trimmed or re-split.

/// One accepted option. `value` = takes an argument; `repeat` = may appear many times.
#[derive(Clone, Copy)]
pub(super) struct Spec {
    name: &'static str,
    value: bool,
    repeat: bool,
}

pub(super) const fn flag(name: &'static str) -> Spec {
    Spec {
        name,
        value: false,
        repeat: false,
    }
}

pub(super) const fn val(name: &'static str) -> Spec {
    Spec {
        name,
        value: true,
        repeat: false,
    }
}

pub(super) const fn many(name: &'static str) -> Spec {
    Spec {
        name,
        value: true,
        repeat: true,
    }
}

pub(super) const SPLIT_OPTS: &[Spec] = &[
    val("pane"),
    flag("current"),
    val("direction"),
    val("ratio"),
    val("cwd"),
    flag("focus"),
    flag("no-focus"),
];
pub(super) const READ_OPTS: &[Spec] = &[val("source"), val("lines")];
pub(super) const WAIT_OUTPUT_OPTS: &[Spec] = &[
    val("match"),
    val("regex"),
    val("source"),
    val("lines"),
    val("timeout"),
];
pub(super) const REPORT_OPTS: &[Spec] = &[
    val("source"),
    val("agent"),
    val("state"),
    val("message"),
    val("seq"),
];
pub(super) const RELEASE_OPTS: &[Spec] = &[val("source"), val("agent"), val("seq")];
pub(super) const PROMPT_OPTS: &[Spec] = &[flag("wait"), many("until"), val("timeout")];
pub(super) const WAIT_OPTS: &[Spec] = &[many("until"), val("timeout")];

/// Scanner result. Words are kept exactly as typed; nothing is trimmed or re-split.
#[derive(Default)]
pub(super) struct Args {
    /// positional words seen before any `--`
    pub(super) pos: Vec<String>,
    /// accepted options in order: (name, value); flags carry an empty value
    opts: Vec<(&'static str, String)>,
    /// words after a literal `--` (None when `--` was not given)
    pub(super) tail: Option<Vec<String>>,
}

impl Args {
    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    pub(super) fn values(&self, name: &str) -> Vec<&str> {
        self.opts
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub(super) fn has(&self, name: &str) -> bool {
        self.opts.iter().any(|(n, _)| *n == name)
    }

    pub(super) fn exclusive(&self, names: &[&str]) -> Result<(), String> {
        if names.iter().filter(|n| self.has(n)).count() > 1 {
            let list: Vec<String> = names.iter().map(|n| format!("--{n}")).collect();
            return Err(format!("use only one of: {}", list.join(" ")));
        }
        Ok(())
    }

    /// Positionals followed by the words after `--`.
    pub(super) fn words(&self) -> Vec<String> {
        let mut words = self.pos.clone();
        words.extend(self.tail.iter().flatten().cloned());
        words
    }
}

/// Scan `argv` against `specs`. Options may appear anywhere before `--`.
pub(super) fn scan(argv: &[String], specs: &[Spec]) -> Result<Args, String> {
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

/// Options must be absent; returns the words (positionals + after `--`).
pub(super) fn no_opts(rest: &[String]) -> Result<Vec<String>, String> {
    Ok(scan(rest, &[])?.words())
}
