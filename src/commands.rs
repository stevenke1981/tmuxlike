//! tmux-like command language: tokenizer, key names, command parsing and default bindings.
//! The helpers at the bottom back the commands that need server state (`debug-frame`,
//! `next-attention`, `confirm-before` prompts, the first-attach hint).
use crate::layout::Move;
use crate::server::PromptKind;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Options of `debug-frame`: test hooks that set fields on a throwaway client.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameOpts {
    /// print plain text rows instead of the ANSI frame
    pub text: bool,
    pub prefix: bool,
    pub prompt: Option<(PromptKind, String)>,
    /// `--chooser` (bare: the target's current window) or `--chooser=N`
    pub chooser: Option<Option<usize>>,
    pub popup: bool,
    pub message: Option<String>,
    pub error: bool,
    pub copy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SplitWindow {
        horizontal: bool,
    },
    NewWindow {
        name: Option<String>,
    },
    RenameWindow(Option<String>),
    RenameSession(Option<String>),
    KillPane,
    /// `SESSION:IDX`; default is the current window
    KillWindow {
        target: Option<String>,
    },
    /// session name; default is the current session
    KillSession {
        target: Option<String>,
    },
    KillServer,
    SelectPane(Move),
    NextPane,
    ResizePane(Move, u16),
    Zoom,
    NextWindow,
    PrevWindow,
    SelectWindow(usize),
    CopyMode {
        page_up: bool,
    },
    PasteBuffer,
    Detach,
    ChooseTree,
    ToggleSidebar,
    SourceFile(Option<String>),
    Set(String, String),
    BindKey(String, String),
    UnbindKey(String),
    CommandPrompt,
    SwitchClient {
        next: bool,
    },
    SendPrefix,
    DisplayMessage(String),
    ListKeys,
    DebugFrame {
        target: Option<String>,
        opts: FrameOpts,
    },
    CapturePane {
        target: Option<String>,
        print: bool,
    },
    SendKeys {
        target: Option<String>,
        literal: bool,
        keys: Vec<String>,
    },
    /// jump to the next (or, with `-p`, previous) pane that needs attention
    NextAttention {
        backward: bool,
    },
    /// y/n question before `command`; `prompt` overrides the generated question
    ConfirmBefore {
        prompt: Option<String>,
        command: String,
    },
}

impl Cmd {
    /// Command name as typed, for error messages (`"kill-pane" needs an attached client`).
    pub fn name(&self) -> &'static str {
        match self {
            Cmd::SplitWindow { .. } => "split-window",
            Cmd::NewWindow { .. } => "new-window",
            Cmd::RenameWindow(_) => "rename-window",
            Cmd::RenameSession(_) => "rename-session",
            Cmd::KillPane => "kill-pane",
            Cmd::KillWindow { .. } => "kill-window",
            Cmd::KillSession { .. } => "kill-session",
            Cmd::KillServer => "kill-server",
            Cmd::SelectPane(_) => "select-pane",
            Cmd::NextPane => "next-pane",
            Cmd::ResizePane(..) | Cmd::Zoom => "resize-pane",
            Cmd::NextWindow => "next-window",
            Cmd::PrevWindow => "previous-window",
            Cmd::SelectWindow(_) => "select-window",
            Cmd::CopyMode { .. } => "copy-mode",
            Cmd::PasteBuffer => "paste-buffer",
            Cmd::Detach => "detach-client",
            Cmd::ChooseTree => "choose-tree",
            Cmd::ToggleSidebar => "toggle-sidebar",
            Cmd::SourceFile(_) => "source-file",
            Cmd::Set(..) => "set-option",
            Cmd::BindKey(..) => "bind-key",
            Cmd::UnbindKey(_) => "unbind-key",
            Cmd::CommandPrompt => "command-prompt",
            Cmd::SwitchClient { .. } => "switch-client",
            Cmd::SendPrefix => "send-prefix",
            Cmd::DisplayMessage(_) => "display-message",
            Cmd::ListKeys => "list-keys",
            Cmd::DebugFrame { .. } => "debug-frame",
            Cmd::CapturePane { .. } => "capture-pane",
            Cmd::SendKeys { .. } => "send-keys",
            Cmd::NextAttention { .. } => "next-attention",
            Cmd::ConfirmBefore { .. } => "confirm-before",
        }
    }
}

/// Split a command line into words, honouring '...' "..." and backslash escapes.
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut has = false;
    let mut q: Option<char> = None;
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match (q, c) {
            (Some(qc), c) if c == qc => q = None,
            (Some('"'), '\\') | (None, '\\') => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                q = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, '#') if cur.is_empty() && !has => break,
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn quote(s: &str) -> String {
    if s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\'' || c == '#')
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn dir_flag(a: &str) -> Option<Move> {
    match a {
        "-L" => Some(Move::Left),
        "-R" => Some(Move::Right),
        "-U" => Some(Move::Up),
        "-D" => Some(Move::Down),
        _ => None,
    }
}

fn flag_val(args: &[String], f: &str) -> Option<String> {
    args.iter()
        .position(|a| a == f)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Positional arguments, skipping flags (and the values of flags that take one).
fn positional(args: &[String], with_val: &[&str]) -> Vec<String> {
    let mut v = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if with_val.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if a.starts_with('-') && a.len() > 1 && a.parse::<i64>().is_err() {
            i += 1;
            continue;
        }
        v.push(a.clone());
        i += 1;
    }
    v
}

/// Value of a `--flag VALUE` or `--flag=VALUE` option; `i` points past the flag.
fn option_value(args: &[String], i: &mut usize, inline: Option<String>) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v);
    }
    let v = args
        .get(*i)
        .cloned()
        .ok_or_else(|| "debug-frame: option needs a value".to_string())?;
    *i += 1;
    Ok(v)
}

/// `KIND:TEXT` for `debug-frame --prompt`.
fn prompt_spec(spec: &str) -> Result<(PromptKind, String), String> {
    let (kind, text) = spec
        .split_once(':')
        .ok_or("debug-frame: --prompt wants KIND:TEXT")?;
    let kind = match kind {
        "command" => PromptKind::Command,
        "rename-window" => PromptKind::RenameWindow,
        "rename-session" => PromptKind::RenameSession,
        "confirm" => PromptKind::Confirm,
        other => return Err(format!("debug-frame: unknown prompt kind {other:?}")),
    };
    Ok((kind, text.to_string()))
}

/// Flags of `debug-frame` (`-t` is read separately).
fn frame_opts(args: &[String]) -> Result<FrameOpts, String> {
    let mut o = FrameOpts::default();
    let mut i = 0;
    while i < args.len() {
        let (flag, inline) = match args[i].split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (args[i].as_str(), None),
        };
        i += 1;
        match flag {
            "-t" => i += 1,
            "--text" => o.text = true,
            "--prefix" => o.prefix = true,
            "--popup" => o.popup = true,
            "--copy" => o.copy = true,
            "--error" => o.error = true,
            "--chooser" => {
                o.chooser = Some(match inline {
                    Some(n) => Some(n.parse().map_err(|_| format!("bad chooser index: {n}"))?),
                    None => None,
                })
            }
            "--prompt" => {
                let v = option_value(args, &mut i, inline)?;
                o.prompt = Some(prompt_spec(&v)?);
            }
            "--message" => o.message = Some(option_value(args, &mut i, inline)?),
            other => return Err(format!("debug-frame: unknown option {other}")),
        }
    }
    Ok(o)
}

/// `confirm-before [-p TEXT] CMD...`. A single word is taken as a whole command line
/// (`"kill-window -t x"`); several words are re-quoted into one.
fn confirm_before(args: &[String]) -> Result<Cmd, String> {
    let mut prompt = None;
    let mut rest: Vec<String> = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-p" && rest.is_empty() && prompt.is_none() {
            prompt = Some(it.next().cloned().ok_or("confirm-before: -p needs TEXT")?);
        } else {
            rest.push(a.clone());
        }
    }
    if rest.is_empty() {
        return Err("usage: confirm-before [-p TEXT] CMD".into());
    }
    let command = if rest.len() == 1 {
        rest.remove(0)
    } else {
        rest.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ")
    };
    parse(&command)?; // validate, like bind-key
    Ok(Cmd::ConfirmBefore { prompt, command })
}

pub fn parse(line: &str) -> Result<Cmd, String> {
    let toks = tokenize(line);
    let Some((name, args)) = toks.split_first() else {
        return Err("empty command".into());
    };
    let rest = || {
        let p = positional(args, &["-t"]);
        if p.is_empty() {
            None
        } else {
            Some(p.join(" "))
        }
    };
    let has = |f: &str| args.iter().any(|a| a == f);
    Ok(match name.as_str() {
        "split-window" | "splitw" => Cmd::SplitWindow {
            horizontal: has("-h"),
        },
        "new-window" | "neww" => Cmd::NewWindow {
            name: flag_val(args, "-n"),
        },
        "rename-window" | "renamew" => Cmd::RenameWindow(rest()),
        "rename-session" | "rename" => Cmd::RenameSession(rest()),
        "kill-pane" | "killp" => Cmd::KillPane,
        "kill-window" | "killw" => Cmd::KillWindow {
            target: flag_val(args, "-t"),
        },
        "kill-session" => Cmd::KillSession {
            target: flag_val(args, "-t"),
        },
        "kill-server" => Cmd::KillServer,
        "select-pane" | "selectp" => match args.iter().find_map(|a| dir_flag(a)) {
            Some(m) => Cmd::SelectPane(m),
            None => Cmd::NextPane,
        },
        "next-pane" => Cmd::NextPane,
        "resize-pane" | "resizep" => {
            if has("-Z") {
                Cmd::Zoom
            } else {
                let m = args
                    .iter()
                    .find_map(|a| dir_flag(a))
                    .ok_or("resize-pane: need -L/-R/-U/-D or -Z")?;
                let n = positional(args, &["-t", "-x", "-y"])
                    .first()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                Cmd::ResizePane(m, n)
            }
        }
        "next-window" | "next" => Cmd::NextWindow,
        "previous-window" | "prev" => Cmd::PrevWindow,
        "select-window" | "selectw" => {
            let t = flag_val(args, "-t")
                .or_else(rest)
                .ok_or("select-window: need -t N")?;
            Cmd::SelectWindow(
                t.trim_start_matches(':')
                    .parse()
                    .map_err(|_| format!("bad window index: {t}"))?,
            )
        }
        "copy-mode" => Cmd::CopyMode { page_up: has("-u") },
        "paste-buffer" | "pasteb" => Cmd::PasteBuffer,
        "detach-client" | "detach" => Cmd::Detach,
        "choose-tree" | "choose-session" | "choose-window" => Cmd::ChooseTree,
        "toggle-sidebar" | "sidebar" => Cmd::ToggleSidebar,
        "source-file" | "source" => Cmd::SourceFile(rest()),
        "set-option" | "set" | "set-window-option" | "setw" => {
            let p = positional(args, &["-t"]);
            if p.is_empty() {
                return Err("usage: set OPTION VALUE".into());
            }
            Cmd::Set(p[0].clone(), p[1..].join(" "))
        }
        "bind-key" | "bind" => {
            let p: Vec<&String> = {
                // skip leading flags like -r, -n (not key-table aware)
                let mut i = 0;
                while i < args.len()
                    && args[i].starts_with('-')
                    && args[i].len() == 2
                    && args[i] != "-"
                {
                    i += 1;
                }
                args[i..].iter().collect()
            };
            if p.len() < 2 {
                return Err("usage: bind-key KEY COMMAND".into());
            }
            let cmd = p[1..]
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(" ");
            parse(&cmd)?; // validate
            Cmd::BindKey(normalize_key(p[0])?, cmd)
        }
        "unbind-key" | "unbind" => {
            let k = positional(args, &[])
                .first()
                .cloned()
                .ok_or("usage: unbind-key KEY")?;
            Cmd::UnbindKey(if has("-a") {
                "*".into()
            } else {
                normalize_key(&k)?
            })
        }
        "command-prompt" => Cmd::CommandPrompt,
        "switch-client" | "switchc" => Cmd::SwitchClient { next: !has("-p") },
        "send-prefix" => Cmd::SendPrefix,
        "display-message" | "display" => Cmd::DisplayMessage(rest().unwrap_or_default()),
        "list-keys" | "lsk" => Cmd::ListKeys,
        "debug-frame" => Cmd::DebugFrame {
            target: flag_val(args, "-t"),
            opts: frame_opts(args)?,
        },
        "next-attention" => Cmd::NextAttention {
            backward: has("-p"),
        },
        "confirm-before" | "confirm" => confirm_before(args)?,
        "capture-pane" | "capturep" => Cmd::CapturePane {
            target: flag_val(args, "-t"),
            print: has("-p"),
        },
        "send-keys" | "send" => Cmd::SendKeys {
            target: flag_val(args, "-t"),
            literal: has("-l"),
            keys: positional(args, &["-t"]),
        },
        other => return Err(format!("unknown command: {other}")),
    })
}

/// Bytes for one `send-keys` argument: a key name (Enter, C-c, Up, ...) or literal text.
pub fn send_key_bytes(arg: &str, literal: bool) -> Vec<u8> {
    if !literal
        && arg.chars().count() > 1
        && let Ok(n) = normalize_key(arg)
    {
        let (mut rest, mut mods) = (n.as_str(), KeyModifiers::NONE);
        loop {
            if let Some(r) = rest.strip_prefix("C-") {
                mods |= KeyModifiers::CONTROL;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("M-") {
                mods |= KeyModifiers::ALT;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("S-") {
                mods |= KeyModifiers::SHIFT;
                rest = r;
            } else {
                break;
            }
        }
        let code = match rest {
            "Space" => Some(KeyCode::Char(' ')),
            "Enter" => Some(KeyCode::Enter),
            "Tab" => Some(KeyCode::Tab),
            "BTab" => Some(KeyCode::BackTab),
            "Escape" => Some(KeyCode::Esc),
            "BSpace" => Some(KeyCode::Backspace),
            "Up" => Some(KeyCode::Up),
            "Down" => Some(KeyCode::Down),
            "Left" => Some(KeyCode::Left),
            "Right" => Some(KeyCode::Right),
            "Home" => Some(KeyCode::Home),
            "End" => Some(KeyCode::End),
            "PageUp" => Some(KeyCode::PageUp),
            "PageDown" => Some(KeyCode::PageDown),
            "Insert" => Some(KeyCode::Insert),
            "Delete" => Some(KeyCode::Delete),
            f if f.starts_with('F') && f[1..].parse::<u8>().is_ok() => {
                Some(KeyCode::F(f[1..].parse().unwrap()))
            }
            c if c.chars().count() == 1 => Some(KeyCode::Char(c.chars().next().unwrap())),
            _ => None,
        };
        if let Some(code) = code {
            return crate::keys::encode(&KeyEvent::new(code, mods), false);
        }
    }
    arg.as_bytes().to_vec()
}

/// Canonical key name for an event, e.g. "c", "%", "C-a", "M-Left", "PageUp".
pub fn key_name(k: &KeyEvent) -> Option<String> {
    let m = k.modifiers;
    let mut ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    let base = match k.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => c.to_ascii_lowercase().to_string(),
        KeyCode::Char(c) if c.is_control() => {
            // raw control char (some terminals)
            ctrl = true;
            ((c as u8) + b'a' - 1).min(b'z') as char
        }
        .to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "BTab".into(),
        KeyCode::Esc => "Escape".into(),
        KeyCode::Backspace => "BSpace".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return None,
    };
    let is_char = matches!(k.code, KeyCode::Char(_));
    let mut s = String::new();
    if ctrl {
        s += "C-";
    }
    if alt {
        s += "M-";
    }
    if shift && !is_char {
        s += "S-";
    }
    s += &base;
    Some(s)
}

/// Normalize a user-written key spec ("C-a", "^A", "M-left", "PPage") to `key_name` form.
pub fn normalize_key(spec: &str) -> Result<String, String> {
    let mut rest = spec.trim();
    if rest.is_empty() {
        return Err("empty key".into());
    }
    let (mut c, mut m, mut s) = (false, false, false);
    loop {
        if rest.len() > 2 {
            let p = &rest[..2];
            match p {
                "C-" | "c-" => c = true,
                "M-" | "m-" => m = true,
                "S-" | "s-" => s = true,
                _ => break,
            }
            rest = &rest[2..];
        } else {
            break;
        }
    }
    if rest.len() == 2 && rest.starts_with('^') {
        c = true;
        rest = &rest[1..];
    }
    let base = match rest.to_ascii_lowercase().as_str() {
        "space" => "Space".to_string(),
        "enter" | "return" => "Enter".into(),
        "tab" => "Tab".into(),
        "btab" => "BTab".into(),
        "escape" | "esc" => "Escape".into(),
        "bspace" | "backspace" => "BSpace".into(),
        "up" => "Up".into(),
        "down" => "Down".into(),
        "left" => "Left".into(),
        "right" => "Right".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" | "ppage" | "pgup" => "PageUp".into(),
        "pagedown" | "npage" | "pgdn" => "PageDown".into(),
        "insert" | "ic" => "Insert".into(),
        "delete" | "dc" => "Delete".into(),
        f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
            f.to_ascii_uppercase()
        }
        _ => {
            let mut ch = rest.chars();
            let k = ch.next().unwrap();
            if ch.next().is_some() {
                return Err(format!("unknown key: {spec}"));
            }
            let is_named = false;
            let _ = is_named;
            if c {
                k.to_ascii_lowercase().to_string()
            } else {
                k.to_string()
            }
        }
    };
    let is_char = base.chars().count() == 1;
    let mut out = String::new();
    if c {
        out += "C-";
    }
    if m {
        out += "M-";
    }
    if s && !is_char {
        out += "S-";
    }
    out += &base;
    Ok(out)
}

pub const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("c", "new-window"),
    ("n", "next-window"),
    ("p", "previous-window"),
    ("0", "select-window -t 0"),
    ("1", "select-window -t 1"),
    ("2", "select-window -t 2"),
    ("3", "select-window -t 3"),
    ("4", "select-window -t 4"),
    ("5", "select-window -t 5"),
    ("6", "select-window -t 6"),
    ("7", "select-window -t 7"),
    ("8", "select-window -t 8"),
    ("9", "select-window -t 9"),
    ("%", "split-window -h"),
    ("|", "split-window -h"),
    ("\"", "split-window -v"),
    ("-", "split-window -v"),
    ("Left", "select-pane -L"),
    ("Right", "select-pane -R"),
    ("Up", "select-pane -U"),
    ("Down", "select-pane -D"),
    ("h", "select-pane -L"),
    ("l", "select-pane -R"),
    ("k", "select-pane -U"),
    ("j", "select-pane -D"),
    ("o", "select-pane -t :.+"),
    ("C-Left", "resize-pane -L 5"),
    ("C-Right", "resize-pane -R 5"),
    ("C-Up", "resize-pane -U 3"),
    ("C-Down", "resize-pane -D 3"),
    ("M-Left", "resize-pane -L 5"),
    ("M-Right", "resize-pane -R 5"),
    ("M-Up", "resize-pane -U 3"),
    ("M-Down", "resize-pane -D 3"),
    ("H", "resize-pane -L 5"),
    ("L", "resize-pane -R 5"),
    ("K", "resize-pane -U 3"),
    ("J", "resize-pane -D 3"),
    ("z", "resize-pane -Z"),
    ("x", "confirm-before kill-pane"),
    ("&", "confirm-before kill-window"),
    ("Tab", "next-attention"),
    ("BTab", "next-attention -p"),
    ("[", "copy-mode"),
    ("PageUp", "copy-mode -u"),
    ("]", "paste-buffer"),
    ("a", "toggle-sidebar"),
    ("b", "toggle-sidebar"),
    ("s", "choose-tree"),
    ("w", "choose-tree"),
    ("g", "choose-tree"),
    ("(", "switch-client -p"),
    (")", "switch-client -n"),
    ("d", "detach-client"),
    (":", "command-prompt"),
    (",", "rename-window"),
    ("$", "rename-session"),
    ("r", "source-file"),
    ("?", "list-keys"),
];

mod ui;
pub use ui::*;

#[cfg(test)]
mod tests;
