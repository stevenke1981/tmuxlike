//! Right-hand sidebar: session and pane list with agent status glyphs.
use super::{BOLD, Cell, Ctx, Grid};
use crate::api::Status;
use crossterm::style::Color;

/// Draw the sidebar (if the session has room for one) along the right edge.
pub(super) fn draw_sidebar(g: &mut Grid, cx: &Ctx) {
    let (st, s, w, h) = (cx.st, cx.s, cx.w, cx.h);
    let sw = st.sidebar_width(s);
    if sw == 0 {
        return;
    }
    let sx = w - sw;
    for y in 0..h.saturating_sub(1) {
        g.put(
            sx,
            y,
            Cell {
                s: "│".into(),
                fg: Color::DarkGrey,
                ..Cell::default()
            },
        );
    }
    let tx = sx + 2;
    let tw = sw - 3;
    let mut y = 0u16;
    g.text(
        tx,
        y,
        tw,
        "tmuxlike · panes",
        Color::Cyan,
        Color::Reset,
        BOLD,
    );
    y += 2;
    for sess in &st.sessions {
        if y >= h - 1 {
            break;
        }
        let cur = sess.name == s.name;
        g.text(
            tx,
            y,
            tw,
            &format!("▸ {}", sess.name),
            if cur { Color::Green } else { Color::White },
            Color::Reset,
            if cur { BOLD } else { 0 },
        );
        y += 1;
        for (wi, wnd) in sess.windows.iter().enumerate() {
            for (pi, pid) in wnd.panes().into_iter().enumerate() {
                if y >= h - 1 {
                    break;
                }
                let Some(p) = st.panes.get(&pid) else {
                    continue;
                };
                let (glyph, color, label) = match crate::api::agent::status(p, &st.cfg) {
                    Status::Working => ("●", Color::Yellow, "working"),
                    Status::Blocked => ("▲", Color::Red, "blocked"),
                    Status::Done => ("◆", Color::Blue, "done"),
                    Status::Unknown => ("?", Color::DarkGrey, "unknown"),
                    Status::Idle => ("○", Color::DarkGrey, "idle"),
                };
                let who = p
                    .agent
                    .as_ref()
                    .and_then(|a| a.name.as_deref().or(Some(a.agent.as_str())))
                    .or(p.label.as_deref())
                    .unwrap_or(wnd.name.as_str());
                let focused = cur && wi == sess.active && pid == wnd.active;
                let x2 = tx + 1;
                g.text(x2, y, 2, glyph, color, Color::Reset, 0);
                let txt = format!("{}.{} {} {}", wi, pi, who, label);
                g.text(
                    x2 + 2,
                    y,
                    tw.saturating_sub(3),
                    &txt,
                    if focused { Color::Green } else { Color::Reset },
                    Color::Reset,
                    if focused { BOLD } else { 0 },
                );
                y += 1;
            }
        }
        y += 1;
    }
    if h > 4 {
        g.text(
            tx,
            h - 2,
            tw,
            "● work ▲ block ◆ done ○ idle",
            Color::DarkGrey,
            Color::Reset,
            0,
        );
    }
}
