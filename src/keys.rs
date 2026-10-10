//! Translate crossterm key events into bytes for the child pty.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn encode(k: &KeyEvent, app_cursor: bool) -> Vec<u8> {
    let m = k.modifiers;
    let alt = m.contains(KeyModifiers::ALT);
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let shift = m.contains(KeyModifiers::SHIFT);
    let modnum = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
    let mut out = Vec::new();
    let csi_mod = |out: &mut Vec<u8>, fin: char, app: bool| {
        if modnum > 1 {
            out.extend_from_slice(format!("\x1b[1;{modnum}{fin}").as_bytes());
        } else if app {
            out.extend_from_slice(format!("\x1bO{fin}").as_bytes());
        } else {
            out.extend_from_slice(format!("\x1b[{fin}").as_bytes());
        }
    };
    let tilde = |out: &mut Vec<u8>, n: u8| {
        if modnum > 1 {
            out.extend_from_slice(format!("\x1b[{n};{modnum}~").as_bytes());
        } else {
            out.extend_from_slice(format!("\x1b[{n}~").as_bytes());
        }
    };
    let mut alt_prefix = alt;
    match k.code {
        KeyCode::Char(c) => {
            if ctrl {
                let lc = c.to_ascii_lowercase();
                let b = match lc {
                    'a'..='z' => lc as u8 - b'a' + 1,
                    '@' | ' ' | '2' => 0,
                    '[' | '3' => 27,
                    '\\' | '4' => 28,
                    ']' | '5' => 29,
                    '^' | '6' => 30,
                    '_' | '7' | '/' | '-' => 31,
                    '8' | '?' => 127,
                    _ => {
                        let mut b = [0u8; 4];
                        out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                        return finish(out, alt_prefix);
                    }
                };
                out.push(b);
            } else {
                let mut b = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
        }
        KeyCode::Enter => out.push(b'\r'),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => out.push(if ctrl { 0x08 } else { 0x7f }),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Up => {
            alt_prefix = false;
            csi_mod(&mut out, 'A', app_cursor)
        }
        KeyCode::Down => {
            alt_prefix = false;
            csi_mod(&mut out, 'B', app_cursor)
        }
        KeyCode::Right => {
            alt_prefix = false;
            csi_mod(&mut out, 'C', app_cursor)
        }
        KeyCode::Left => {
            alt_prefix = false;
            csi_mod(&mut out, 'D', app_cursor)
        }
        KeyCode::Home => {
            alt_prefix = false;
            csi_mod(&mut out, 'H', app_cursor)
        }
        KeyCode::End => {
            alt_prefix = false;
            csi_mod(&mut out, 'F', app_cursor)
        }
        KeyCode::Insert => {
            alt_prefix = false;
            tilde(&mut out, 2)
        }
        KeyCode::Delete => {
            alt_prefix = false;
            tilde(&mut out, 3)
        }
        KeyCode::PageUp => {
            alt_prefix = false;
            tilde(&mut out, 5)
        }
        KeyCode::PageDown => {
            alt_prefix = false;
            tilde(&mut out, 6)
        }
        KeyCode::F(n) => {
            alt_prefix = false;
            match n {
                1..=4 => {
                    let fin = [b'P', b'Q', b'R', b'S'][n as usize - 1] as char;
                    if modnum > 1 {
                        out.extend_from_slice(format!("\x1b[1;{modnum}{fin}").as_bytes());
                    } else {
                        out.extend_from_slice(format!("\x1bO{fin}").as_bytes());
                    }
                }
                5..=12 => {
                    let code = [15, 17, 18, 19, 20, 21, 23, 24][n as usize - 5];
                    tilde(&mut out, code)
                }
                _ => {}
            }
        }
        _ => {}
    }
    finish(out, alt_prefix)
}

fn finish(mut out: Vec<u8>, alt: bool) -> Vec<u8> {
    if alt && !out.is_empty() {
        out.insert(0, 0x1b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }
    #[test]
    fn basic() {
        assert_eq!(
            encode(&k(KeyCode::Char('a'), KeyModifiers::NONE), false),
            b"a"
        );
        assert_eq!(
            encode(&k(KeyCode::Char('c'), KeyModifiers::CONTROL), false),
            vec![3]
        );
        assert_eq!(
            encode(&k(KeyCode::Char('x'), KeyModifiers::ALT), false),
            b"\x1bx"
        );
        assert_eq!(
            encode(&k(KeyCode::Up, KeyModifiers::NONE), false),
            b"\x1b[A"
        );
        assert_eq!(encode(&k(KeyCode::Up, KeyModifiers::NONE), true), b"\x1bOA");
        assert_eq!(
            encode(&k(KeyCode::Left, KeyModifiers::CONTROL), false),
            b"\x1b[1;5D"
        );
        assert_eq!(encode(&k(KeyCode::Enter, KeyModifiers::NONE), false), b"\r");
        assert_eq!(
            encode(&k(KeyCode::F(5), KeyModifiers::NONE), false),
            b"\x1b[15~"
        );
        assert_eq!(
            encode(&k(KeyCode::Char('é'), KeyModifiers::NONE), false),
            "é".as_bytes()
        );
    }
}
