//! Mouse input: click to focus and select, drag to copy, wheel to scroll.
use crate::copy::{self, CopyState};
use crate::server::State;
use crossterm::event::{MouseButton, MouseEventKind};

impl State {
    pub(crate) fn handle_mouse(&mut self, cid: usize, si: usize, m: crossterm::event::MouseEvent) {
        let s = &self.sessions[si];
        let w = &s.windows[s.active];
        let rects = self.visible_rects(s, w);
        let hit = rects
            .iter()
            .find(|(_, r)| r.contains(m.column, m.row))
            .copied();
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.drag = None;
                    if c.chooser.is_some() || c.popup.is_some() {
                        c.chooser = None;
                        c.popup = None;
                        self.mark_dirty();
                        return;
                    }
                }
                if let Some((pid, r)) = hit {
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
                } else if m.row == s.rows - 1 {
                    if let Some(wi) = crate::render::status_hit(s, m.column) {
                        self.sessions[si].active = wi;
                        self.relayout(si);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some((pid, row, col, _)) = self.clients.get(&cid).and_then(|c| c.drag) else {
                    return;
                };
                let Some(r) = rects.iter().find(|(i, _)| *i == pid).map(|x| x.1) else {
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
            MouseEventKind::Up(MouseButton::Left) => {
                let Some((pid, _, _, moved)) =
                    self.clients.get_mut(&cid).and_then(|c| c.drag.take())
                else {
                    return;
                };
                if moved {
                    self.copy_selection(cid, pid);
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = m.kind == MouseEventKind::ScrollUp;
                if let Some((pid, r)) = hit {
                    let p = self.panes.get_mut(&pid).unwrap();
                    if p.parser.screen().alternate_screen() {
                        let seq: &[u8] = if up { b"\x1b[A" } else { b"\x1b[B" };
                        for _ in 0..3 {
                            let _ = p.writer.write_all(seq);
                        }
                        let _ = p.writer.flush();
                    } else {
                        let cur = p.parser.screen().scrollback();
                        let new = if up { cur + 3 } else { cur.saturating_sub(3) };
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
            }
            _ => {}
        }
    }
}
