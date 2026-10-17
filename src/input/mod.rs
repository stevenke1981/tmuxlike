//! Client input: terminal events from attached clients become server actions.
//! Submodules: `keys` (keyboard and overlays), `mouse`, `copy` (copy mode and clipboard).
mod copy;
mod keys;
mod mouse;

use crate::commands;
use crate::server::{PromptKind, Shared, State};
use crossterm::event::{Event, KeyEventKind};
use std::time::Instant;

impl State {
    pub(crate) fn run_line(&mut self, shared: &Shared, cid: usize, line: &str) {
        if let Err(e) = commands::parse(line).and_then(|c| self.exec(shared, cid, c)) {
            self.message(cid, e);
        }
    }

    /// Insert pasted text at the prompt cursor, like typing; line breaks become spaces.
    fn paste_prompt(&mut self, cid: usize, text: &str) {
        let Some(p) = self.clients.get_mut(&cid).and_then(|c| c.prompt.as_mut()) else {
            return;
        };
        let text = text.replace(['\r', '\n'], " ");
        let at = p
            .buf
            .char_indices()
            .nth(p.cur)
            .map_or(p.buf.len(), |(i, _)| i);
        p.buf.insert_str(at, &text);
        p.cur += text.chars().count();
        p.hist = None;
        self.mark_dirty();
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
            Event::Paste(text) => match self.clients.get(&cid).and_then(|c| c.prompt.as_ref()) {
                // a y/n question takes no text: the paste is dropped
                Some(p) if p.kind == PromptKind::Confirm => {}
                Some(_) => self.paste_prompt(cid, &text),
                None => {
                    let pid = self.active_pane(si);
                    self.paste(pid, &text);
                }
            },
            Event::Mouse(m) if self.cfg.mouse => {
                if let Some(line) = self.handle_mouse(cid, si, m) {
                    self.run_line(shared, cid, &line);
                }
            }
            _ => {}
        }
    }
}
