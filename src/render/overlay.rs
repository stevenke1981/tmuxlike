//! Modal overlays drawn over the panes: the session/window chooser and the key popup.
use super::{BOLD, Cell, Ctx, Grid, truncate};
use crate::api::Status;
use crate::server::State;
use crossterm::style::Color;
use unicode_width::UnicodeWidthStr;

/// Entries for the session/window chooser: (session index, window index or None for the session row)
pub fn chooser_entries(st: &State) -> Vec<(usize, Option<usize>)> {
    let mut v = vec![];
    for (si, s) in st.sessions.iter().enumerate() {
        v.push((si, None));
        for wi in 0..s.windows.len() {
            v.push((si, Some(wi)));
        }
    }
    v
}

/// Draw a bordered box with a title and a list of lines; `sel` highlights one line and
/// scrolls to it, otherwise `scroll` sets the first visible line.
fn overlay(
    g: &mut Grid,
    cx: &Ctx,
    title: &str,
    lines: &[(String, bool)],
    sel: Option<usize>,
    scroll: usize,
) {
    let (w, h) = (cx.w, cx.h);
    let bw = (lines
        .iter()
        .map(|l| l.0.width())
        .max()
        .unwrap_or(10)
        .max(title.width()) as u16
        + 4)
    .min(w.saturating_sub(2))
    .max(10);
    let maxh = h.saturating_sub(3).max(3);
    let bh = (lines.len() as u16 + 2).min(maxh);
    let bx = w.saturating_sub(bw) / 2;
    let by = h.saturating_sub(1).saturating_sub(bh) / 2;
    let inner = (bh - 2) as usize;
    let start = sel
        .map(|s| s.saturating_sub(inner.saturating_sub(1)))
        .unwrap_or(scroll.min(lines.len().saturating_sub(inner)));
    for yy in 0..bh {
        for xx in 0..bw {
            let edge = yy == 0 || yy == bh - 1 || xx == 0 || xx == bw - 1;
            let ch = match (yy, xx) {
                (0, 0) => "┌",
                (0, x) if x == bw - 1 => "┐",
                (y, 0) if y == bh - 1 => "└",
                (y, x) if y == bh - 1 && x == bw - 1 => "┘",
                (0, _) | (_, _) if edge && (yy == 0 || yy == bh - 1) => "─",
                _ if edge => "│",
                _ => " ",
            };
            g.put(
                bx + xx,
                by + yy,
                Cell {
                    s: ch.into(),
                    fg: Color::Cyan,
                    ..Cell::default()
                },
            );
        }
    }
    g.text(
        bx + 2,
        by,
        bw.saturating_sub(4),
        &format!(" {title} "),
        Color::Cyan,
        Color::Reset,
        BOLD,
    );
    for (i, (l, bold)) in lines.iter().enumerate().skip(start).take(inner) {
        let y = by + 1 + (i - start) as u16;
        let selected = sel == Some(i);
        let (fg, bg) = if selected {
            (Color::Black, Color::Cyan)
        } else {
            (Color::Reset, Color::Reset)
        };
        if selected {
            for xx in 1..bw - 1 {
                g.put(
                    bx + xx,
                    y,
                    Cell {
                        bg,
                        ..Cell::default()
                    },
                );
            }
        }
        let t = truncate(l, (bw - 4) as usize);
        g.text(bx + 2, y, bw - 4, &t, fg, bg, if *bold { BOLD } else { 0 });
    }
}

/// One line per session (bold) and per window (indented) for the chooser.
fn chooser_lines(cx: &Ctx) -> Vec<(String, bool)> {
    let (st, s) = (cx.st, cx.s);
    chooser_entries(st)
        .into_iter()
        .map(|(si, wi)| {
            let ss = &st.sessions[si];
            match wi {
                None => (
                    format!(
                        "{}: {} windows{}",
                        ss.name,
                        ss.windows.len(),
                        if ss.name == s.name { " (attached)" } else { "" }
                    ),
                    true,
                ),
                Some(wi) => {
                    let wn = &ss.windows[wi];
                    let (working, blocked) =
                        wn.panes().iter().filter_map(|p| st.panes.get(p)).fold(
                            (false, false),
                            |(w, b), p| match crate::api::agent::status(p, &st.cfg) {
                                Status::Working => (true, b),
                                Status::Blocked => (w, true),
                                _ => (w, b),
                            },
                        );
                    (
                        format!(
                            "  ├─ {}: {} [{} panes]{}{}{}",
                            wi,
                            wn.name,
                            wn.panes().len(),
                            if wi == ss.active { " *" } else { "" },
                            if working { " ●" } else { "" },
                            if blocked { " ▲" } else { "" }
                        ),
                        false,
                    )
                }
            }
        })
        .collect()
}

/// Draw the chooser or key popup if open. Returns `true` when an overlay took the cursor,
/// so the caller hides it.
pub(super) fn draw_overlays(g: &mut Grid, cx: &Ctx) -> bool {
    let c = cx.c;
    if let Some(sel) = c.chooser {
        let lines = chooser_lines(cx);
        overlay(
            g,
            cx,
            "choose session/window — ↑↓ Enter, x kill, Esc",
            &lines,
            Some(sel),
            0,
        );
        true
    } else if let Some((p, scroll)) = &c.popup {
        let lines: Vec<(String, bool)> = p.iter().map(|l| (l.clone(), false)).collect();
        overlay(g, cx, "keys", &lines, None, *scroll);
        true
    } else {
        false
    }
}
