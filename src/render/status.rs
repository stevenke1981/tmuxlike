//! Bottom status bar: session tag, window labels, prefix hint, and the prompt or message line.
use super::{BOLD, Cell, Ctx, Grid};
use crate::server::{PromptKind, Session};
use crossterm::style::Color;
use unicode_width::UnicodeWidthStr;

/// Window labels in the status bar: (window index, start col, end col)
fn status_labels(s: &Session) -> Vec<(usize, u16, u16, String)> {
    let mut x = (s.name.len() + 3) as u16;
    let mut v = vec![];
    for (i, w) in s.windows.iter().enumerate() {
        let flag = if i == s.active {
            "*"
        } else if w.zoomed {
            "Z"
        } else {
            " "
        };
        let label = format!("{}:{}{}", i, w.name, flag);
        let len = label.width() as u16;
        v.push((i, x, x + len, label));
        x += len + 1;
    }
    v
}

pub fn status_hit(s: &Session, col: u16) -> Option<usize> {
    status_labels(s)
        .into_iter()
        .find(|(_, a, b, _)| col >= *a && col < *b)
        .map(|l| l.0)
}

/// Draw the status bar on the last row. Returns the cursor position when a command
/// prompt is open (the prompt owns the cursor), otherwise `None`.
pub(super) fn draw_status(g: &mut Grid, cx: &Ctx) -> Option<(u16, u16)> {
    let (st, s, c, w, h) = (cx.st, cx.s, cx.c, cx.w, cx.h);
    let sy = h - 1;
    for x in 0..w {
        g.put(
            x,
            sy,
            Cell {
                bg: Color::Green,
                fg: Color::Black,
                ..Cell::default()
            },
        );
    }
    g.text(
        0,
        sy,
        w,
        &format!("[{}]", s.name),
        Color::Black,
        Color::Green,
        BOLD,
    );
    for (i, a, _, label) in status_labels(s) {
        let act = st
            .panes
            .iter()
            .any(|(id, p)| p.attention && s.windows[i].layout.contains(*id));
        let (fg, bg, at) = if i == s.active {
            (Color::Green, Color::Black, BOLD)
        } else if act {
            (Color::Black, Color::Yellow, 0)
        } else {
            (Color::Black, Color::Green, 0)
        };
        g.text(a, sy, w.saturating_sub(a), &label, fg, bg, at);
    }
    let right = if c.prefix {
        " PREFIX | tmuxlike ".to_string()
    } else {
        format!(" {} | tmuxlike ", st.cfg.prefix)
    };
    let rl = right.width() as u16;
    if w > rl {
        let (fg, bg) = if c.prefix {
            (Color::Black, Color::Yellow)
        } else {
            (Color::Black, Color::Green)
        };
        g.text(w - rl, sy, rl, &right, fg, bg, 0);
    }
    draw_prompt_or_message(g, cx, sy)
}

/// Prompt (takes the status row and the cursor) or transient message (status row only).
fn draw_prompt_or_message(g: &mut Grid, cx: &Ctx, sy: u16) -> Option<(u16, u16)> {
    let (w, c) = (cx.w, cx.c);
    if let Some(p) = &c.prompt {
        let label = match p.kind {
            PromptKind::Command => ":".to_string(),
            PromptKind::RenameWindow => "(rename-window) ".into(),
            PromptKind::RenameSession => "(rename-session) ".into(),
        };
        for x in 0..w {
            g.put(
                x,
                sy,
                Cell {
                    bg: Color::Yellow,
                    fg: Color::Black,
                    ..Cell::default()
                },
            );
        }
        let text = format!("{label}{}", p.buf);
        // keep the end of the input visible
        let mut shown = text.as_str();
        while shown.width() as u16 >= w && !shown.is_empty() {
            let mut it = shown.chars();
            it.next();
            shown = it.as_str();
        }
        let used = g.text(0, sy, w, shown, Color::Black, Color::Yellow, 0);
        Some((used.min(w - 1), sy))
    } else {
        if let Some((m, _)) = &c.message {
            for x in 0..w {
                g.put(
                    x,
                    sy,
                    Cell {
                        bg: Color::Yellow,
                        fg: Color::Black,
                        ..Cell::default()
                    },
                );
            }
            g.text(0, sy, w, m, Color::Black, Color::Yellow, 0);
        }
        None
    }
}
