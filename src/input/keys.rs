//! Keyboard input: overlays (popup, chooser, prompt), the prefix key, and pane pass-through.
use crate::commands::{Cmd, key_name};
use crate::keys;
use crate::render::chooser_entries;
use crate::server::{PromptKind, Shared, State};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl State {
    pub(crate) fn handle_key(&mut self, shared: &Shared, cid: usize, si: usize, k: KeyEvent) {
        let Some(c) = self.clients.get_mut(&cid) else {
            return;
        };
        // overlays first
        if c.popup.is_some() {
            self.popup_key(cid, k);
            return;
        }
        if c.chooser.is_some() {
            self.chooser_key(cid, k);
            return;
        }
        if c.prompt.is_some() {
            self.prompt_key(shared, cid, k);
            return;
        }
        if c.prefix {
            c.prefix = false;
            self.mark_dirty();
            if self.cfg.is_prefix(&k) {
                let _ = self.exec(shared, cid, Cmd::SendPrefix);
                return;
            }
            let Some(name) = key_name(&k) else { return };
            if let Some(line) = self.bindings.get(&name).cloned() {
                self.run_line(shared, cid, &line);
            }
            return;
        }
        if self.cfg.is_prefix(&k) {
            c.prefix = true;
            self.mark_dirty();
            return;
        }
        let pid = self.active_pane(si);
        if self.panes.get(&pid).is_some_and(|p| p.copy.is_some()) {
            self.copy_key(cid, pid, k);
            return;
        }
        let Some(p) = self.panes.get_mut(&pid) else {
            return;
        };
        let app = p.parser.screen().application_cursor();
        let b = keys::encode(&k, app);
        self.write_pane(pid, &b);
    }

    /// Popup overlay: scroll with j/k, space and PageUp/PageDown; any other key closes it.
    pub(crate) fn popup_key(&mut self, cid: usize, k: KeyEvent) {
        let Some(c) = self.clients.get_mut(&cid) else {
            return;
        };
        if let Some((lines, off)) = &mut c.popup {
            let max = lines.len().saturating_sub(1);
            match k.code {
                KeyCode::Down | KeyCode::Char('j') => *off = (*off + 1).min(max),
                KeyCode::Up | KeyCode::Char('k') => *off = off.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char(' ') => *off = (*off + 10).min(max),
                KeyCode::PageUp => *off = off.saturating_sub(10),
                _ => c.popup = None,
            }
        }
        self.mark_dirty();
    }

    /// Chooser overlay: move the selection, Enter switches, `x` kills the selected entry.
    pub(crate) fn chooser_key(&mut self, cid: usize, k: KeyEvent) {
        let Some(sel) = self.clients.get(&cid).and_then(|c| c.chooser) else {
            return;
        };
        let entries = chooser_entries(self);
        let c = self.clients.get_mut(&cid).unwrap();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => c.chooser = Some(sel.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => {
                c.chooser = Some((sel + 1).min(entries.len().saturating_sub(1)))
            }
            KeyCode::Enter => {
                c.chooser = None;
                if let Some(&(nsi, wi)) = entries.get(sel) {
                    self.switch_session(cid, nsi);
                    if let Some(wi) = wi {
                        self.sessions[nsi].active = wi;
                        self.relayout(nsi);
                    }
                }
            }
            KeyCode::Char('x') => {
                c.chooser = None;
                if let Some(&(nsi, wi)) = entries.get(sel) {
                    match wi {
                        Some(wi) => {
                            for pid in self.sessions[nsi].windows[wi].panes() {
                                self.pane_exited(pid);
                            }
                        }
                        None => self.kill_session(nsi),
                    }
                }
            }
            _ => c.chooser = None,
        }
        self.mark_dirty();
    }

    /// Prompt line: edit the buffer; Enter runs it as a command, Esc or Ctrl-C cancels.
    pub(crate) fn prompt_key(&mut self, shared: &Shared, cid: usize, k: KeyEvent) {
        let Some(c) = self.clients.get_mut(&cid) else {
            return;
        };
        let Some(p) = &mut c.prompt else {
            return;
        };
        match k.code {
            KeyCode::Enter => {
                let p = c.prompt.take().unwrap();
                let line = match p.kind {
                    PromptKind::Command => p.buf,
                    PromptKind::RenameWindow => {
                        format!("rename-window \"{}\"", p.buf.replace('"', "\\\""))
                    }
                    PromptKind::RenameSession => {
                        format!("rename-session \"{}\"", p.buf.replace('"', "\\\""))
                    }
                };
                if !line.trim().is_empty() {
                    self.run_line(shared, cid, &line);
                }
            }
            KeyCode::Esc => c.prompt = None,
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => c.prompt = None,
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.clear(),
            KeyCode::Backspace => {
                p.buf.pop();
            }
            KeyCode::Char(ch) if !k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.push(ch),
            _ => {}
        }
        self.mark_dirty();
    }
}
