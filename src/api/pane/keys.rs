//! Key specs and terminal input: herdr key syntax and legacy tmux names to bytes, sending keys,
//! and `submit` (paste, short delay, Enter).
use super::ensure_pane;
use crate::api::{ApiError, lock};
use crate::keys::encode;
use crate::server::{Shared, State};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Map a key spec (herdr syntax or legacy tmux names) to the bytes a pane should receive.
pub fn key_bytes(spec: &str, app_cursor: bool) -> Result<Vec<u8>, String> {
    let unknown = || format!("unknown key: {spec}");
    let (mods, base) = split_modifiers(spec);
    let (code, mods) = base_key(base, mods).ok_or_else(unknown)?;
    let ctrl_unmapped = matches!(
        code,
        KeyCode::Char(c) if mods.contains(KeyModifiers::CONTROL) && !ctrl_maps(c)
    );
    if ctrl_unmapped {
        return Err(unknown());
    }
    let bytes = encode(&KeyEvent::new(code, mods), app_cursor);
    if bytes.is_empty() {
        return Err(unknown());
    }
    Ok(bytes)
}

const MODIFIER_PREFIXES: &[(&str, KeyModifiers)] = &[
    ("ctrl+", KeyModifiers::CONTROL),
    ("control+", KeyModifiers::CONTROL),
    ("alt+", KeyModifiers::ALT),
    ("shift+", KeyModifiers::SHIFT),
    ("c-", KeyModifiers::CONTROL),
    ("m-", KeyModifiers::ALT),
    ("s-", KeyModifiers::SHIFT),
    ("^", KeyModifiers::CONTROL),
];

/// Strip any stacked modifier prefixes (case-insensitive). A prefix is only taken when
/// something follows it, so a lone "-" or "^" stays a plain key.
fn split_modifiers(spec: &str) -> (KeyModifiers, &str) {
    let mut mods = KeyModifiers::NONE;
    let mut rest = spec;
    loop {
        let lower = rest.to_ascii_lowercase();
        let hit = MODIFIER_PREFIXES
            .iter()
            .find(|(p, _)| lower.starts_with(p) && rest.len() > p.len());
        match hit {
            Some((p, m)) => {
                mods |= *m;
                rest = &rest[p.len()..];
            }
            None => return (mods, rest),
        }
    }
}

/// Characters that `encode` turns into control bytes when Ctrl is held.
fn ctrl_maps(c: char) -> bool {
    matches!(
        c.to_ascii_lowercase(),
        'a'..='z'
            | '@'
            | ' '
            | '2'
            | '['
            | '3'
            | '\\'
            | '4'
            | ']'
            | '5'
            | '^'
            | '6'
            | '_'
            | '7'
            | '/'
            | '-'
            | '8'
            | '?'
    )
}

fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "btab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" | "bspace" => KeyCode::Backspace,
        "space" => KeyCode::Char(' '),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" | "ppage" => KeyCode::PageUp,
        "pagedown" | "pgdn" | "npage" => KeyCode::PageDown,
        "insert" | "ic" => KeyCode::Insert,
        "delete" | "dc" => KeyCode::Delete,
        "minus" => KeyCode::Char('-'),
        "plus" => KeyCode::Char('+'),
        "backtick" => KeyCode::Char('`'),
        "equals" => KeyCode::Char('='),
        "comma" => KeyCode::Char(','),
        "period" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "backslash" => KeyCode::Char('\\'),
        "semicolon" => KeyCode::Char(';'),
        "quote" => KeyCode::Char('\''),
        "lbracket" => KeyCode::Char('['),
        "rbracket" => KeyCode::Char(']'),
        f => return function_key(f),
    })
}

fn function_key(name: &str) -> Option<KeyCode> {
    let n: u8 = name.strip_prefix('f')?.parse().ok()?;
    (1..=12).contains(&n).then_some(KeyCode::F(n))
}

/// Resolve the base key name (after modifiers) to a code, folding Shift into the code where
/// `encode` would otherwise lose it.
fn base_key(base: &str, mods: KeyModifiers) -> Option<(KeyCode, KeyModifiers)> {
    let mut chars = base.chars();
    let code = match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() => KeyCode::Char(c),
        _ => named_key(&base.to_ascii_lowercase())?,
    };
    Some(match code {
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
            (KeyCode::BackTab, mods - KeyModifiers::SHIFT)
        }
        KeyCode::Char(c) if mods.contains(KeyModifiers::SHIFT) => {
            (KeyCode::Char(c.to_ascii_uppercase()), mods)
        }
        code => (code, mods),
    })
}

fn keys_to_bytes(keys: &[String], app_cursor: bool) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    for k in keys {
        out.extend(key_bytes(k, app_cursor).map_err(ApiError::invalid)?);
    }
    Ok(out)
}

/// Validate every key first, then write all bytes in one go. Nothing is sent if any key is bad.
pub fn send_keys(st: &mut State, pid: usize, keys: &[String]) -> Result<(), ApiError> {
    ensure_pane(st, pid)?;
    let app_cursor = st.panes[&pid].parser.screen().application_cursor();
    let bytes = keys_to_bytes(keys, app_cursor)?;
    st.write_pane(pid, &bytes);
    Ok(())
}

/// Paste `text` (bracketed-paste aware), wait `enter_delay_ms` without holding the lock, then Enter.
pub fn submit(
    shared: &Shared,
    pid: usize,
    text: &str,
    enter_delay_ms: u64,
) -> Result<(), ApiError> {
    {
        let mut st = lock(shared);
        ensure_pane(&st, pid)?;
        st.paste(pid, text);
        st.mark_dirty();
    }
    std::thread::sleep(Duration::from_millis(enter_delay_ms));
    let mut st = lock(shared);
    ensure_pane(&st, pid)?;
    st.write_pane(pid, b"\r");
    st.mark_dirty();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(spec: &str) -> Vec<u8> {
        key_bytes(spec, false).unwrap_or_else(|e| panic!("{spec}: {e}"))
    }

    #[test]
    fn key_bytes_named_and_modified_keys() {
        assert_eq!(bytes("enter"), b"\r");
        assert_eq!(bytes("ctrl+c"), b"\x03");
        assert_eq!(bytes("alt+x"), b"\x1bx");
        assert_eq!(bytes("ctrl+alt+x"), b"\x1b\x18");
        assert_eq!(bytes("shift+tab"), b"\x1b[Z");
        assert_eq!(bytes("f5"), b"\x1b[15~");
        assert_eq!(bytes("minus"), b"-");
        assert_eq!(bytes("BSpace"), b"\x7f");
        assert_eq!(bytes("ctrl+space"), b"\x00");
        assert_eq!(bytes("ctrl+minus"), b"\x1f");
        assert_eq!(bytes("shift+a"), b"A");
        assert_eq!(bytes("x"), b"x");
    }

    #[test]
    fn key_bytes_legacy_tmux_names() {
        assert_eq!(bytes("C-c"), b"\x03");
        assert_eq!(bytes("c-c"), b"\x03");
        assert_eq!(bytes("M-x"), b"\x1bx");
        assert_eq!(bytes("Enter"), b"\r");
        assert_eq!(bytes("^A"), b"\x01");
        assert_eq!(bytes("C--"), b"\x1f");
    }

    #[test]
    fn key_bytes_cursor_mode_and_arrows() {
        assert_eq!(key_bytes("up", false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes("up", true).unwrap(), b"\x1bOA");
        assert_eq!(bytes("shift+up"), b"\x1b[1;2A");
    }

    #[test]
    fn key_bytes_rejects_unknown_keys() {
        for bad in ["bogus", "", "ctrl+", "ctrl+1", "f13", "C-"] {
            let err = key_bytes(bad, false).unwrap_err();
            assert!(err.starts_with("unknown key: "), "{bad}: {err}");
        }
    }
}
