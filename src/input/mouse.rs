//! Mouse input: click to focus and select, drag to copy, wheel to scroll.
//!
//! Click targets match what is drawn: sidebar lines come from `sidebar_rows`, window labels
//! from `status_hit`, and the session chip and counters from `status_regions` (same layout).
//! Overlays (chooser, key popup) take the wheel; any click closes them, as before.
use crate::copy::{self, CopyState};
use crate::layout::Rect;
use crate::render::{
    SidebarKind, chooser_entries, sidebar_rows, sidebar_text_width, status_hit, status_regions,
};
use crate::server::State;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

/// Lines one wheel tick moves a key popup or a pane.
const WHEEL_LINES: usize = 3;

impl State {
    /// Handle one mouse event from client `cid` on session index `si`.
    /// Returns a command line for the caller to run with `run_line` (session chip and
    /// counters); every other click or wheel step is applied here.
    pub(crate) fn handle_mouse(&mut self, cid: usize, si: usize, m: MouseEvent) -> Option<String> {
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.cancel_prefix(cid);
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click_left(cid, si, m),
            MouseEventKind::Drag(MouseButton::Left) => {
                self.drag_copy(cid, si, m);
                None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.finish_drag(cid);
                None
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => self.wheel(cid, si, m),
            _ => None,
        }
    }

    /// Any click cancels a pending prefix key.
    fn cancel_prefix(&mut self, cid: usize) {
        let was = self
            .clients
            .get_mut(&cid)
            .is_some_and(|c| std::mem::replace(&mut c.prefix, false));
        if was {
            self.mark_dirty();
        }
    }

    fn click_left(&mut self, cid: usize, si: usize, m: MouseEvent) -> Option<String> {
        if let Some(c) = self.clients.get_mut(&cid) {
            c.drag = None;
            if c.chooser.is_some() || c.popup.is_some() {
                c.chooser = None;
                c.popup = None;
                self.mark_dirty();
                return None;
            }
        }
        if self.click_sidebar(cid, si, m.column, m.row) {
            return None;
        }
        if let Some((pid, r)) = self.pane_at(si, m.column, m.row) {
            self.start_pane_drag(cid, si, pid, r, m);
            return None;
        }
        if let Some(pid) = self.title_at(si, m.column, m.row) {
            self.focus_pane(pid);
            return None;
        }
        if m.row == self.sessions[si].rows - 1 {
            return self.click_status(cid, si, m.column);
        }
        None
    }

    fn pane_at(&self, si: usize, col: u16, row: u16) -> Option<(usize, Rect)> {
        let s = &self.sessions[si];
        let w = &s.windows[s.active];
        self.visible_rects(s, w)
            .into_iter()
            .find(|(_, r)| r.contains(col, row))
    }

    /// Sidebar column: focus a pane or detail line, or switch to a collapsed session.
    /// Returns `true` when the click landed in the sidebar, even on a blank or footer line.
    fn click_sidebar(&mut self, cid: usize, si: usize, col: u16, row: u16) -> bool {
        let s = &self.sessions[si];
        let sw = self.sidebar_width(s);
        if sw == 0 || col <= s.cols - sw {
            return false;
        }
        let tw = sidebar_text_width(self, s);
        let target = sidebar_rows(self, s, tw, s.rows)
            .into_iter()
            .find(|r| r.y == row);
        match target {
            Some(r) if matches!(r.kind, SidebarKind::Pane | SidebarKind::Detail) => {
                if let Some(pid) = r.pane {
                    self.focus_across_sessions(cid, pid);
                }
            }
            Some(r) if r.kind == SidebarKind::SessionClosed => {
                if let Some(nsi) = r.session {
                    self.switch_session(cid, nsi);
                    self.mark_dirty();
                }
            }
            _ => {}
        }
        true
    }

    /// Switch the client to the pane's session when needed, then focus the pane.
    fn focus_across_sessions(&mut self, cid: usize, pid: usize) {
        let Some((psi, _)) = self.locate(pid) else {
            return;
        };
        if self.client_si(cid) != Some(psi) {
            self.switch_session(cid, psi);
        }
        self.focus_pane(pid);
        self.mark_dirty();
    }

    /// Status row: the session chip opens the chooser, window labels switch windows, and the
    /// counters run `next-attention`.
    fn click_status(&mut self, cid: usize, si: usize, col: u16) -> Option<String> {
        let regions = self
            .clients
            .get(&cid)
            .map(|c| status_regions(self, c, &self.sessions[si]))?;
        let inside = |span: Option<(u16, u16)>| span.is_some_and(|(a, b)| col >= a && col < b);
        if inside(regions.session) {
            return Some("choose-tree".into());
        }
        if inside(regions.counts) {
            return Some("next-attention".into());
        }
        if let Some(wi) = status_hit(&self.sessions[si], col) {
            self.sessions[si].active = wi;
            self.relayout(si);
        }
        None
    }

    /// Pane whose title row (reserved above its rect, see `visible_rects`) is at `row`.
    fn title_at(&self, si: usize, col: u16, row: u16) -> Option<usize> {
        let s = &self.sessions[si];
        self.title_rows(s, &s.windows[s.active])
            .into_iter()
            .find(|(_, r)| r.y == row && col >= r.x && col < r.x + r.w)
            .map(|(pid, _)| pid)
    }

    fn start_pane_drag(&mut self, cid: usize, si: usize, pid: usize, r: Rect, m: MouseEvent) {
        let s = &mut self.sessions[si];
        s.windows[s.active].active = pid;
        if let Some(p) = self.panes.get_mut(&pid) {
            if let Some(cs) = &mut p.copy {
                cs.anchor = None;
            }
            let off = p.parser.screen().scrollback();
            let row = copy::abs_row(m.row - r.y, off);
            if let Some(c) = self.clients.get_mut(&cid) {
                c.drag = Some((pid, row, m.column - r.x, false));
            }
        }
        self.mark_dirty();
    }

    fn drag_copy(&mut self, cid: usize, si: usize, m: MouseEvent) {
        let Some((pid, row, col, _)) = self.clients.get(&cid).and_then(|c| c.drag) else {
            return;
        };
        let s = &self.sessions[si];
        let w = &s.windows[s.active];
        let Some(r) = self
            .visible_rects(s, w)
            .iter()
            .find(|(i, _)| *i == pid)
            .map(|x| x.1)
        else {
            return;
        };
        let Some(p) = self.panes.get_mut(&pid) else {
            return;
        };
        let mut cy = m.row.clamp(r.y, r.y + r.h - 1) - r.y;
        let cx = m.column.clamp(r.x, r.x + r.w - 1) - r.x;
        // auto-scroll when dragging past the edges
        let off = p.parser.screen().scrollback();
        if m.row < r.y {
            p.parser.screen_mut().set_scrollback(off + 1);
            cy = 0;
        } else if m.row >= r.y + r.h && off > 0 {
            p.parser.screen_mut().set_scrollback(off - 1);
        }
        let cs = p.copy.get_or_insert_with(CopyState::default);
        cs.anchor = Some((row, col));
        cs.line = false;
        cs.cx = cx;
        cs.cy = cy;
        if let Some(c) = self.clients.get_mut(&cid) {
            c.drag = Some((pid, row, col, true));
        }
        self.mark_dirty();
    }

    fn finish_drag(&mut self, cid: usize) {
        let Some((pid, _, _, moved)) = self.clients.get_mut(&cid).and_then(|c| c.drag.take())
        else {
            return;
        };
        if moved {
            self.copy_selection(cid, pid);
        }
    }

    /// Wheel: overlays first (chooser moves its selection, popup scrolls), then the pane
    /// under the pointer (app cursor keys on the alternate screen, else scrollback).
    fn wheel(&mut self, cid: usize, si: usize, m: MouseEvent) -> Option<String> {
        let up = m.kind == MouseEventKind::ScrollUp;
        if self.wheel_overlay(cid, up) {
            return None;
        }
        if let Some((pid, r)) = self.pane_at(si, m.column, m.row) {
            self.wheel_pane(pid, r, up);
        }
        None
    }

    /// Move the chooser selection by one entry or scroll the key popup. `true` if an overlay
    /// was open.
    fn wheel_overlay(&mut self, cid: usize, up: bool) -> bool {
        let entries = chooser_entries(self).len();
        let Some(c) = self.clients.get_mut(&cid) else {
            return false;
        };
        if let Some(sel) = c.chooser {
            c.chooser = Some(if up {
                sel.saturating_sub(1)
            } else {
                (sel + 1).min(entries.saturating_sub(1))
            });
        } else if let Some((lines, off)) = &mut c.popup {
            let max = lines.len().saturating_sub(1);
            *off = if up {
                off.saturating_sub(WHEEL_LINES)
            } else {
                (*off + WHEEL_LINES).min(max)
            };
        } else {
            return false;
        }
        self.mark_dirty();
        true
    }

    fn wheel_pane(&mut self, pid: usize, r: Rect, up: bool) {
        let Some(p) = self.panes.get_mut(&pid) else {
            return;
        };
        if p.parser.screen().alternate_screen() {
            let seq: &[u8] = if up { b"\x1b[A" } else { b"\x1b[B" };
            for _ in 0..WHEEL_LINES {
                let _ = p.writer.write_all(seq);
            }
            let _ = p.writer.flush();
            return;
        }
        let cur = p.parser.screen().scrollback();
        let new = if up {
            cur + WHEEL_LINES
        } else {
            cur.saturating_sub(WHEEL_LINES)
        };
        p.parser.screen_mut().set_scrollback(new);
        if p.parser.screen().scrollback() > 0 {
            p.copy.get_or_insert(CopyState {
                cx: 0,
                cy: r.h - 1,
                anchor: None,
                line: false,
            });
        } else if p.copy.as_ref().is_some_and(|c| c.anchor.is_none()) {
            p.copy = None;
        }
        self.mark_dirty();
    }
}
