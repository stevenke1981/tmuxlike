//! Copy mode: cursor, selection and text extraction over a vt100 screen + scrollback.

#[derive(Debug, Clone, Default)]
pub struct CopyState {
    /// cursor in viewport coordinates
    pub cx: u16,
    pub cy: u16,
    /// selection anchor in absolute coordinates (row relative to the live screen top, may be negative)
    pub anchor: Option<(i64, u16)>,
    pub line: bool,
}

pub fn abs_row(view_row: u16, offset: usize) -> i64 {
    view_row as i64 - offset as i64
}

/// Is (row, col) inside the selection spanning a..b?
pub fn in_selection(a: (i64, u16), b: (i64, u16), line: bool, p: (i64, u16)) -> bool {
    let (s, e) = if a <= b { (a, b) } else { (b, a) };
    if line {
        p.0 >= s.0 && p.0 <= e.0
    } else {
        p >= s && p <= e
    }
}

/// Extract text between two absolute positions (inclusive).
pub fn extract(parser: &mut vt100::Parser, a: (i64, u16), b: (i64, u16), line: bool) -> String {
    let (s, e) = if a <= b { (a, b) } else { (b, a) };
    let saved = parser.screen().scrollback();
    let (rows, cols) = parser.screen().size();
    let mut out = String::new();
    for r in s.0..=e.0 {
        let off = if r < 0 { (-r) as usize } else { 0 };
        parser.screen_mut().set_scrollback(off);
        if parser.screen().scrollback() != off {
            continue;
        }
        let v = (r + off as i64) as u16;
        if v >= rows {
            continue;
        }
        let c0 = if r == s.0 && !line { s.1 } else { 0 };
        let c1 = if r == e.0 && !line {
            (e.1 + 1).min(cols)
        } else {
            cols
        };
        let mut text = String::new();
        for c in c0..c1 {
            if let Some(cell) = parser.screen().cell(v, c) {
                if cell.is_wide_continuation() {
                    continue;
                }
                let t = cell.contents();
                text.push_str(if t.is_empty() { " " } else { t });
            }
        }
        let wrapped = parser.screen().row_wrapped(v);
        if wrapped && r != e.0 {
            out.push_str(&text);
        } else {
            out.push_str(text.trim_end());
            if r != e.0 || line {
                out.push('\n');
            }
        }
    }
    parser.screen_mut().set_scrollback(saved);
    out
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if ch.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if ch.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn b64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn selection_and_extract() {
        assert!(in_selection((0, 5), (1, 2), false, (0, 7)));
        assert!(!in_selection((0, 5), (1, 2), false, (1, 3)));
        assert!(in_selection((1, 2), (0, 5), true, (1, 9)));
        let mut p = vt100::Parser::new(3, 10, 100);
        p.process(b"line1\r\nline2\r\nline3\r\nline4\r\nhello");
        // live screen: line3, line4, hello ; line1,line2 in scrollback
        assert_eq!(extract(&mut p, (0, 0), (0, 4), false), "line3");
        assert_eq!(extract(&mut p, (-2, 2), (0, 1), false), "ne1\nline2\nli");
        assert_eq!(extract(&mut p, (-1, 3), (-1, 3), true), "line2\n");
        assert_eq!(p.screen().scrollback(), 0);
    }
}
