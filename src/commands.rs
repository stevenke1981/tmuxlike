//! tmux-like command language: tokenizer, key names, command parsing and default bindings.
use crate::layout::Move;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SplitWindow { horizontal: bool },
    NewWindow { name: Option<String> },
    RenameWindow(Option<String>),
    RenameSession(Option<String>),
    KillPane,
    KillWindow,
    KillSession,
    KillServer,
    SelectPane(Move),
    NextPane,
    ResizePane(Move, u16),
    Zoom,
    NextWindow,
    PrevWindow,
    SelectWindow(usize),
    CopyMode { page_up: bool },
    PasteBuffer,
    Detach,
    ChooseTree,
    ToggleSidebar,
    SourceFile(Option<String>),
    Set(String, String),
    BindKey(String, String),
    UnbindKey(String),
    CommandPrompt,
    SwitchClient { next: bool },
    SendPrefix,
    DisplayMessage(String),
    ListKeys,
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
        "kill-window" | "killw" => Cmd::KillWindow,
        "kill-session" => Cmd::KillSession,
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
        other => return Err(format!("unknown command: {other}")),
    })
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
    ("x", "kill-pane"),
    ("&", "kill-window"),
    ("[", "copy-mode"),
    ("PageUp", "copy-mode -u"),
    ("]", "paste-buffer"),
    ("a", "toggle-sidebar"),
    ("b", "toggle-sidebar"),
    ("s", "choose-tree"),
    ("w", "choose-tree"),
    ("(", "switch-client -p"),
    (")", "switch-client -n"),
    ("d", "detach-client"),
    (":", "command-prompt"),
    (",", "rename-window"),
    ("$", "rename-session"),
    ("r", "source-file"),
    ("?", "list-keys"),
];

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
