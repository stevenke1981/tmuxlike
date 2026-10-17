//! tmux-like command language: tokenizer, key names, command parsing and default bindings.
//! The helpers at the bottom back the commands that need server state (`debug-frame`,
//! `next-attention`, `confirm-before` prompts, the first-attach hint).
use crate::api::{Status, pane_id_str};
use crate::copy::CopyState;
use crate::layout::Move;
use crate::render::{Frame, render_client};
use crate::server::{Client, Message, Prompt, PromptKind, State};
use crate::theme::ClientEnv;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

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
                .or_else(|| rest())
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
    if !literal && arg.chars().count() > 1 {
        if let Ok(n) = normalize_key(arg) {
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

/// Key bindings as `list-keys` and the `debug-frame --popup` hook show them.
pub fn key_lines(st: &State) -> Vec<String> {
    let mut lines = vec![
        format!(
            "prefix = {}   (j/k/Space scroll, q closes)",
            st.cfg.prefix_name()
        ),
        String::new(),
    ];
    lines.extend(st.bindings.iter().map(|(k, c)| format!("{k:>10}  {c}")));
    lines
}

/// Pick the next pane to attend to. Blocked panes come before done ones; within a state the
/// search is cyclic from `origin` over `items` (session/window/pane order), and `origin`
/// itself is never picked. `backward` walks the cycle the other way.
pub fn pick_attention(
    items: &[(usize, Status)],
    origin: Option<usize>,
    backward: bool,
) -> Option<(usize, Status)> {
    let n = items.len();
    let origin_idx = origin.and_then(|o| items.iter().position(|(p, _)| *p == o));
    let order: Vec<usize> = match (origin_idx, backward) {
        // k in 1..n: visits every pane once and leaves the origin out
        (Some(o), false) => (1..n).map(|k| (o + k) % n).collect(),
        (Some(o), true) => (1..n).map(|k| (o + n - k) % n).collect(),
        (None, false) => (0..n).collect(),
        (None, true) => (0..n).rev().collect(),
    };
    [Status::Blocked, Status::Done]
        .into_iter()
        .find_map(|want| order.iter().map(|&i| items[i]).find(|(_, s)| *s == want))
}

/// Every pane with its effective status, in session/window/pane order.
pub fn attention_items(st: &State) -> Vec<(usize, Status)> {
    st.sessions
        .iter()
        .flat_map(|s| s.windows.iter())
        .flat_map(|w| w.panes())
        .map(|pid| (pid, pane_status(st, pid)))
        .collect()
}

fn pane_status(st: &State, pid: usize) -> Status {
    st.panes
        .get(&pid)
        .map_or(Status::Idle, |p| crate::api::agent::status(p, &st.cfg))
}

/// Name shown for a pane: agent name, agent kind, label, then window name (as the sidebar does).
pub fn pane_who(st: &State, pid: usize) -> String {
    let Some(p) = st.panes.get(&pid) else {
        return pane_id_str(pid);
    };
    p.agent
        .as_ref()
        .map(|a| a.name.clone().unwrap_or_else(|| a.agent.clone()))
        .or_else(|| p.label.clone())
        .or_else(|| {
            st.locate(pid)
                .map(|(si, wi)| st.sessions[si].windows[wi].name.clone())
        })
        .unwrap_or_else(|| pane_id_str(pid))
}

/// (session name, window index, pane index within the window) of a pane.
fn pane_place(st: &State, pid: usize) -> Option<(String, usize, usize)> {
    let (si, wi) = st.locate(pid)?;
    let pi = st.sessions[si].windows[wi]
        .panes()
        .iter()
        .position(|p| *p == pid)?;
    Some((st.sessions[si].name.clone(), wi, pi))
}

/// `: message` from the agent's report, or nothing.
fn agent_note(st: &State, pid: usize) -> String {
    st.panes
        .get(&pid)
        .and_then(|p| p.agent.as_ref())
        .and_then(|a| a.message.as_deref())
        .map(|m| format!(": {m}"))
        .unwrap_or_default()
}

/// `▲ claude 0.1 blocked: need approval`
pub fn attention_text(st: &State, pid: usize, status: Status) -> String {
    let glyph = if status == Status::Blocked {
        "▲"
    } else {
        "◆"
    };
    let place = pane_place(st, pid)
        .map(|(_, wi, pi)| format!(" {wi}.{pi}"))
        .unwrap_or_default();
    format!(
        "{glyph} {}{place} {}{}",
        pane_who(st, pid),
        status.as_str(),
        agent_note(st, pid)
    )
}

/// Headless `next-attention` (CLI, no client): origin is the most recent session's focused pane.
/// Returns `pN blocked`/`pN done` and focuses the pane.
pub fn headless_attention(st: &mut State, backward: bool) -> Result<String, String> {
    let si = st.target_session(None)?;
    let origin = st.active_pane(si);
    let items = attention_items(st);
    let (pid, status) =
        pick_attention(&items, Some(origin), backward).ok_or("no other pane needs attention")?;
    st.focus_pane(pid);
    Ok(format!("{} {}", pane_id_str(pid), status.as_str()))
}

/// Toast for a pane that turned blocked while no client is looking at it: an info message on
/// every client that is not answering a prompt.
pub fn notify_blocked(st: &mut State, pid: usize) {
    let viewed = st.clients.values().any(|c| {
        st.sessions
            .iter()
            .any(|s| s.name == c.session && s.windows[s.active].active == pid)
    });
    if viewed {
        return;
    }
    let Some((session, wi, pi)) = pane_place(st, pid) else {
        return;
    };
    let text = format!(
        "▲ {} blocked in {session} {wi}.{pi}{} ({} Tab)",
        pane_who(st, pid),
        agent_note(st, pid),
        st.cfg.prefix_name()
    );
    let cids: Vec<usize> = st
        .clients
        .iter()
        .filter(|(_, c)| c.prompt.is_none())
        .map(|(id, _)| *id)
        .collect();
    for cid in cids {
        st.message(cid, text.clone());
    }
}

static ATTACHED: AtomicBool = AtomicBool::new(false);

/// True for the first client attach of this server process.
pub fn first_attach() -> bool {
    !ATTACHED.swap(true, Ordering::Relaxed)
}

/// First-attach hint for one client. `Message` lives 3 s, so it starts 5 s in the future to
/// stay visible for 8 s.
pub fn attach_hint(st: &mut State, cid: usize) {
    let p = st.cfg.prefix_name();
    let text = format!("{p} ? keys · {p} a agents · {p} Tab next alert");
    if let Some(c) = st.clients.get_mut(&cid) {
        c.message = Some(Message {
            text,
            at: Instant::now() + Duration::from_secs(5),
            error: false,
        });
    }
    st.mark_dirty();
}

fn chooser_index(st: &State, si: usize) -> usize {
    let cur = st.sessions[si].active;
    crate::render::chooser_entries(st)
        .iter()
        .position(|e| e.0 == si && e.1 == Some(cur))
        .unwrap_or(0)
}

/// `debug-frame`: render the target session with a throwaway client whose fields the flags set.
/// Without flags this is the plain ANSI frame; `--text` returns `rows` plain lines.
pub fn debug_frame(st: &mut State, target: Option<&str>, o: &FrameOpts) -> Result<String, String> {
    let si = st.target_session(target)?;
    let (name, cols, rows) = {
        let s = &st.sessions[si];
        (s.name.clone(), s.cols, s.rows)
    };
    let default_sel = chooser_index(st, si);
    let chooser = o.chooser.map(|sel| sel.unwrap_or(default_sel));
    let popup = if o.popup {
        Some((key_lines(st), 0))
    } else {
        None
    };
    let mut c = Client {
        session: name,
        sender: Box::new(io::sink()),
        prefix: o.prefix,
        frame: Frame::default(),
        prompt: o.prompt.as_ref().map(|(kind, text)| match kind {
            PromptKind::Confirm => Prompt::confirm(text.clone(), String::new()),
            kind => Prompt::new(*kind, text.clone()),
        }),
        chooser,
        popup,
        message: o.message.as_ref().map(|text| Message {
            text: text.clone(),
            at: Instant::now(),
            error: o.error,
        }),
        drag: None,
        env: ClientEnv::default(),
    };
    // copy mode is a field of the pane; set it for this render only and put it back
    let pid = st.active_pane(si);
    // outer Some: the pane exists; inner: the copy state it had before
    let prev_copy: Option<Option<CopyState>> = match (o.copy, st.panes.get_mut(&pid)) {
        (true, Some(p)) => {
            let (cy, cx) = p.parser.screen().cursor_position();
            Some(p.copy.replace(CopyState {
                cx,
                cy,
                anchor: None,
                line: false,
            }))
        }
        _ => None,
    };
    let ansi = render_client(st, &mut c).unwrap_or_default();
    if let (Some(prev), Some(p)) = (prev_copy, st.panes.get_mut(&pid)) {
        p.copy = prev;
    }
    Ok(if o.text {
        screen_text(&ansi, cols, rows)
    } else {
        ansi
    })
}

/// Replay an emitted frame into a terminal of the session size and return its visible rows,
/// each padded to `cols` display columns, one per line.
pub fn screen_text(ansi: &str, cols: u16, rows: u16) -> String {
    let (cols, rows) = (cols.max(2), rows.max(2));
    let mut parser = vt100::Parser::new(rows, cols, 0);
    let _ = crate::server::contain_vt(&mut parser, rows, cols, 0, |vt| vt.process(ansi.as_bytes()));
    let mut out = String::new();
    for line in parser.screen().rows(0, cols) {
        let pad = usize::from(cols).saturating_sub(line.width());
        out += &line;
        out += &" ".repeat(pad);
        out.push('\n');
    }
    out
}

impl State {
    /// `next-attention` from an attached client: switch the client to the target's session
    /// and focus it.
    pub(crate) fn next_attention(&mut self, cid: usize, si: usize, backward: bool) {
        let origin = self.active_pane(si);
        let items = attention_items(self);
        let Some((pid, status)) = pick_attention(&items, Some(origin), backward) else {
            self.message(cid, "no pane needs attention");
            return;
        };
        let Some((nsi, _)) = self.locate(pid) else {
            return;
        };
        self.switch_session(cid, nsi);
        self.focus_pane(pid);
        let text = attention_text(self, pid, status);
        self.message(cid, text);
    }

    /// `(session index, window index)` for `SESSION:IDX`, `SESSION` (its current window) or,
    /// with an empty session part, the default session.
    pub(crate) fn window_for(
        &self,
        default_si: Option<usize>,
        target: Option<&str>,
    ) -> Result<(usize, usize), String> {
        let Some(t) = target else {
            let si = default_si.ok_or("kill-window: needs -t SESSION:IDX")?;
            return Ok((si, self.sessions[si].active));
        };
        let (name, idx) = match t.rsplit_once(':') {
            Some((n, i)) => (n, Some(i)),
            None => (t, None),
        };
        let si = if name.is_empty() {
            default_si.ok_or_else(|| format!("can't find session: {t}"))?
        } else {
            self.sidx(name)
                .ok_or_else(|| format!("can't find session: {name}"))?
        };
        let sess = &self.sessions[si];
        let wi = match idx {
            None => sess.active,
            Some(i) => i
                .parse::<usize>()
                .ok()
                .filter(|w| *w < sess.windows.len())
                .ok_or_else(|| format!("can't find window: {t}"))?,
        };
        Ok((si, wi))
    }

    pub(crate) fn kill_window_at(&mut self, si: usize, wi: usize) {
        for pid in self.sessions[si].windows[wi].panes() {
            self.pane_exited(pid);
        }
    }

    /// Question for a `confirm-before` without `-p`: names the target and its agent state.
    pub(crate) fn confirm_prompt_text(&self, si: usize, command: &str) -> Result<String, String> {
        Ok(match parse(command)? {
            Cmd::KillPane => {
                let pid = self.active_pane(si);
                format!(
                    "kill pane {} ({}, {})?",
                    pane_id_str(pid),
                    pane_who(self, pid),
                    pane_status(self, pid).as_str()
                )
            }
            Cmd::KillWindow { target } => {
                let (s, w) = self.window_for(Some(si), target.as_deref())?;
                let win = &self.sessions[s].windows[w];
                let panes = win.panes();
                format!(
                    "kill window {} ({}{})?",
                    win.name,
                    count(panes.len(), "pane"),
                    blocked_note(self, &panes)
                )
            }
            Cmd::KillSession { target } => {
                let s = match target {
                    None => si,
                    Some(t) => self.target_session(Some(t.as_str()))?,
                };
                let sess = &self.sessions[s];
                let panes: Vec<usize> = sess.windows.iter().flat_map(|w| w.panes()).collect();
                format!(
                    "kill session {} ({}{})?",
                    sess.name,
                    count(sess.windows.len(), "window"),
                    blocked_note(self, &panes)
                )
            }
            _ => format!("{command}?"),
        })
    }
}

fn count(n: usize, unit: &str) -> String {
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn blocked_note(st: &State, panes: &[usize]) -> String {
    let n = panes
        .iter()
        .filter(|p| pane_status(st, **p) == Status::Blocked)
        .count();
    if n == 0 {
        String::new()
    } else {
        format!(", {n} blocked")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_quotes() {
        assert_eq!(
            tokenize(r#"rename-window "my win"  x"#),
            vec!["rename-window", "my win", "x"]
        );
        assert_eq!(
            tokenize(r#"display 'a b' c\ d # comment"#),
            vec!["display", "a b", "c d"]
        );
        assert_eq!(tokenize(r#"set x """#), vec!["set", "x", ""]);
    }

    #[test]
    fn parse_commands() {
        assert_eq!(
            parse("splitw -h").unwrap(),
            Cmd::SplitWindow { horizontal: true }
        );
        assert_eq!(
            parse("split-window").unwrap(),
            Cmd::SplitWindow { horizontal: false }
        );
        assert_eq!(
            parse("resize-pane -L 10").unwrap(),
            Cmd::ResizePane(Move::Left, 10)
        );
        assert_eq!(parse("resize-pane -Z").unwrap(), Cmd::Zoom);
        assert_eq!(parse("select-pane -U").unwrap(), Cmd::SelectPane(Move::Up));
        assert_eq!(parse("select-pane -t :.+").unwrap(), Cmd::NextPane);
        assert_eq!(parse("select-window -t :3").unwrap(), Cmd::SelectWindow(3));
        assert_eq!(
            parse("rename-window hello world").unwrap(),
            Cmd::RenameWindow(Some("hello world".into()))
        );
        assert_eq!(parse("rename-window").unwrap(), Cmd::RenameWindow(None));
        assert_eq!(
            parse("set -g mouse off").unwrap(),
            Cmd::Set("mouse".into(), "off".into())
        );
        assert_eq!(
            parse("new-window -n logs").unwrap(),
            Cmd::NewWindow {
                name: Some("logs".into())
            }
        );
        assert_eq!(
            parse("bind-key C-k display \"hi there\"").unwrap(),
            Cmd::BindKey("C-k".into(), "display \"hi there\"".into())
        );
        assert_eq!(
            parse("bind -r M-h resize-pane -L 2").unwrap(),
            Cmd::BindKey("M-h".into(), "resize-pane -L 2".into())
        );
        assert!(parse("bind x bogus").is_err());
        assert_eq!(
            parse("unbind PPage").unwrap(),
            Cmd::UnbindKey("PageUp".into())
        );
        assert!(parse("frobnicate").is_err());
        for (k, c) in DEFAULT_BINDINGS {
            assert!(parse(c).is_ok(), "{c}");
            assert_eq!(&normalize_key(k).unwrap(), k);
        }
    }

    #[test]
    fn send_keys() {
        assert_eq!(send_key_bytes("Enter", false), b"\r");
        assert_eq!(send_key_bytes("C-c", false), vec![3]);
        assert_eq!(send_key_bytes("echo hi", false), b"echo hi");
        assert_eq!(send_key_bytes("Enter", true), b"Enter");
        assert_eq!(send_key_bytes("x", false), b"x");
        assert_eq!(
            parse("send-keys -t s1 'echo ok' Enter").unwrap(),
            Cmd::SendKeys {
                target: Some("s1".into()),
                literal: false,
                keys: vec!["echo ok".into(), "Enter".into()]
            }
        );
        assert_eq!(
            parse("capture-pane -p -t x").unwrap(),
            Cmd::CapturePane {
                target: Some("x".into()),
                print: true
            }
        );
    }

    #[test]
    fn attention_prefers_blocked_over_done() {
        // Arrange: pane 1 done comes first in order, pane 3 blocked later
        let items = [(1, Status::Done), (2, Status::Idle), (3, Status::Blocked)];
        // Act
        let got = pick_attention(&items, Some(2), false);
        // Assert
        assert_eq!(got, Some((3, Status::Blocked)));
    }

    #[test]
    fn attention_is_cyclic_from_origin_and_skips_origin() {
        let items = [
            (1, Status::Blocked),
            (2, Status::Blocked),
            (3, Status::Idle),
        ];
        assert_eq!(pick_attention(&items, Some(2), false).unwrap().0, 1);
        assert_eq!(pick_attention(&items, Some(1), false).unwrap().0, 2);
        // backward from 1 wraps to 2 (pane 1 itself is never picked)
        assert_eq!(pick_attention(&items, Some(1), true).unwrap().0, 2);
        // the only candidate is the origin: nothing else needs attention
        assert_eq!(pick_attention(&items[..1], Some(1), false), None);
    }

    #[test]
    fn attention_falls_back_to_done_and_reports_none() {
        let items = [(1, Status::Done), (2, Status::Idle), (3, Status::Working)];
        assert_eq!(
            pick_attention(&items, Some(3), false),
            Some((1, Status::Done))
        );
        let idle = [(1, Status::Idle), (2, Status::Working)];
        assert_eq!(pick_attention(&idle, Some(1), false), None);
        assert_eq!(pick_attention(&[], None, false), None);
    }

    #[test]
    fn attention_without_origin_starts_at_the_ends() {
        let items = [(4, Status::Blocked), (5, Status::Blocked)];
        assert_eq!(pick_attention(&items, None, false).unwrap().0, 4);
        assert_eq!(pick_attention(&items, None, true).unwrap().0, 5);
    }

    #[test]
    fn debug_frame_flags_compose() {
        let Cmd::DebugFrame { target, opts } = parse(
            "debug-frame -t demo --text --prefix --prompt confirm:\"kill pane?\" \
             --chooser=2 --message hi --error --copy --popup",
        )
        .unwrap() else {
            panic!("not debug-frame");
        };
        assert_eq!(target.as_deref(), Some("demo"));
        assert!(opts.text && opts.prefix && opts.error && opts.copy && opts.popup);
        assert_eq!(
            opts.prompt,
            Some((PromptKind::Confirm, "kill pane?".to_string()))
        );
        assert_eq!(opts.chooser, Some(Some(2)));
        assert_eq!(opts.message.as_deref(), Some("hi"));
    }

    #[test]
    fn debug_frame_defaults_and_bad_flags() {
        let Cmd::DebugFrame { opts, .. } = parse("debug-frame").unwrap() else {
            panic!("not debug-frame");
        };
        assert_eq!(opts, FrameOpts::default());
        let Cmd::DebugFrame { opts, .. } = parse("debug-frame --chooser").unwrap() else {
            panic!("not debug-frame");
        };
        assert_eq!(opts.chooser, Some(None));
        assert!(parse("debug-frame --bogus").is_err());
        assert!(parse("debug-frame --prompt nokind").is_err());
        assert!(parse("debug-frame --prompt wat:x").is_err());
    }

    #[test]
    fn screen_text_is_plain_rows_of_exact_width() {
        // Arrange: "ab" on row 1, a wide char at column 3 of row 1 (index 2)
        let ansi = "\x1b[?2026h\x1b[2J\x1b[1;1Hab\x1b[1;3H中\x1b[?2026l";
        // Act
        let text = screen_text(ansi, 6, 3);
        // Assert
        assert!(!text.contains('\x1b'));
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row.width(), 6, "{row:?}");
        }
        assert!(rows[0].starts_with("ab"), "{:?}", rows[0]);
        assert!(rows[0].contains('中'));
        assert_eq!(rows[2].trim(), "");
    }

    #[test]
    fn confirm_before_parses_prompt_and_command() {
        assert_eq!(
            parse("confirm-before -p \"kill pane?\" kill-pane").unwrap(),
            Cmd::ConfirmBefore {
                prompt: Some("kill pane?".into()),
                command: "kill-pane".into()
            }
        );
        // several words are re-quoted into one command line
        assert_eq!(
            parse("confirm-before kill-window -t demo:1").unwrap(),
            Cmd::ConfirmBefore {
                prompt: None,
                command: "kill-window -t demo:1".into()
            }
        );
        // a single word is taken as a whole command line
        assert_eq!(
            parse("confirm-before \"kill-window -t demo:1\"").unwrap(),
            Cmd::ConfirmBefore {
                prompt: None,
                command: "kill-window -t demo:1".into()
            }
        );
        assert!(parse("confirm-before").is_err());
        assert!(parse("confirm-before bogus").is_err());
        assert!(parse("confirm-before -p").is_err());
    }

    #[test]
    fn kill_targets_and_next_attention_parse() {
        assert_eq!(
            parse("kill-window -t demo:2").unwrap(),
            Cmd::KillWindow {
                target: Some("demo:2".into())
            }
        );
        assert_eq!(
            parse("kill-session -t work").unwrap(),
            Cmd::KillSession {
                target: Some("work".into())
            }
        );
        assert_eq!(
            parse("next-attention").unwrap(),
            Cmd::NextAttention { backward: false }
        );
        assert_eq!(
            parse("next-attention -p").unwrap(),
            Cmd::NextAttention { backward: true }
        );
        assert_eq!(parse("resize-pane -Z").unwrap().name(), "resize-pane");
        assert_eq!(parse("kill-pane").unwrap().name(), "kill-pane");
    }

    #[test]
    fn attention_and_confirm_bindings_are_defaults() {
        let has = |k: &str, c: &str| DEFAULT_BINDINGS.iter().any(|(a, b)| *a == k && *b == c);
        assert!(has("Tab", "next-attention"));
        assert!(has("BTab", "next-attention -p"));
        assert!(has("g", "choose-tree"));
        assert!(has("x", "confirm-before kill-pane"));
        assert!(has("&", "confirm-before kill-window"));
    }

    #[test]
    fn keys() {
        assert_eq!(normalize_key("C-a").unwrap(), "C-a");
        assert_eq!(normalize_key("^B").unwrap(), "C-b");
        assert_eq!(normalize_key("m-left").unwrap(), "M-Left");
        assert_eq!(normalize_key("C-M-x").unwrap(), "C-M-x");
        assert_eq!(normalize_key("npage").unwrap(), "PageDown");
        assert_eq!(normalize_key("-").unwrap(), "-");
        assert_eq!(normalize_key("F5").unwrap(), "F5");
        let ev = |c, m| KeyEvent::new(c, m);
        assert_eq!(
            key_name(&ev(KeyCode::Char('a'), KeyModifiers::CONTROL)).unwrap(),
            "C-a"
        );
        assert_eq!(
            key_name(&ev(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap(),
            "A"
        );
        assert_eq!(
            key_name(&ev(KeyCode::Char('%'), KeyModifiers::SHIFT)).unwrap(),
            "%"
        );
        assert_eq!(
            key_name(&ev(KeyCode::Left, KeyModifiers::ALT)).unwrap(),
            "M-Left"
        );
        assert_eq!(
            key_name(&ev(KeyCode::Char('\u{2}'), KeyModifiers::NONE)).unwrap(),
            "C-b"
        );
    }
}
