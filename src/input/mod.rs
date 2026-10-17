//! Client input: terminal events from attached clients become server actions.
//! Submodules: `keys` (keyboard and overlays), `mouse`, `copy` (copy mode and clipboard).
mod copy;
mod keys;
mod mouse;

use crate::commands;
use crate::server::{Shared, State};
use crossterm::event::{Event, KeyEventKind};
use std::time::Instant;

impl State {
    pub(crate) fn run_line(&mut self, shared: &Shared, cid: usize, line: &str) {
        if let Err(e) = commands::parse(line).and_then(|c| self.exec(shared, cid, c)) {
            self.message(cid, e);
        }
    }

    pub(crate) fn handle_event(&mut self, shared: &Shared, cid: usize, ev: Event) {
        let Some(si) = self.client_si(cid) else {
            return;
        };
        self.sessions[si].last_used = Instant::now();
        match ev {
            Event::Resize(cols, rows) => {
                let s = &mut self.sessions[si];
                s.cols = cols.max(10);
                s.rows = rows.max(3);
                self.relayout(si);
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.frame.invalidate();
                }
            }
            Event::Key(k) if k.kind != KeyEventKind::Release => self.handle_key(shared, cid, si, k),
            Event::Paste(text) => {
                if let Some(p) = self.clients.get_mut(&cid).and_then(|c| c.prompt.as_mut()) {
                    p.buf.push_str(&text.replace(['\r', '\n'], " "));
                    self.mark_dirty();
                    return;
                }
                let pid = self.active_pane(si);
                self.paste(pid, &text);
            }
            Event::Mouse(m) if self.cfg.mouse => self.handle_mouse(cid, si, m),
            _ => {}
        }
    }
}
