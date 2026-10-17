//! Commands that need server state: list-keys lines, next-attention, blocked toast,
//! first-attach hint, debug-frame hooks and the confirm-before prompts.
use super::*;
use crate::api::{Status, pane_id_str};
use crate::copy::CopyState;
use crate::render::{Frame, render_client};
use crate::server::{Client, Message, Prompt, PromptKind, State};
use crate::theme::ClientEnv;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// Key bindings as `list-keys` and the `debug-frame --popup` hook show them.
pub fn key_lines(st: &State) -> Vec<String> {
    let mut lines = vec![
        format!(
            "prefix = {}   (j/k/Space scroll, q closes)",
            st.cfg.prefix_name()
        ),
        String::new(),
    ];
    lines.extend(st.bindings.iter().map(|(k, c)| format!("{k:>10}  {c}")));
    lines
}

/// Pick the next pane to attend to. Blocked panes come before done ones; within a state the
/// search is cyclic from `origin` over `items` (session/window/pane order), and `origin`
/// itself is never picked. `backward` walks the cycle the other way.
pub fn pick_attention(
    items: &[(usize, Status)],
    origin: Option<usize>,
    backward: bool,
) -> Option<(usize, Status)> {
    let n = items.len();
    let origin_idx = origin.and_then(|o| items.iter().position(|(p, _)| *p == o));
    let order: Vec<usize> = match (origin_idx, backward) {
        // k in 1..n: visits every pane once and leaves the origin out
        (Some(o), false) => (1..n).map(|k| (o + k) % n).collect(),
        (Some(o), true) => (1..n).map(|k| (o + n - k) % n).collect(),
        (None, false) => (0..n).collect(),
        (None, true) => (0..n).rev().collect(),
    };
    [Status::Blocked, Status::Done]
        .into_iter()
        .find_map(|want| order.iter().map(|&i| items[i]).find(|(_, s)| *s == want))
}

/// Every pane with its effective status, in session/window/pane order.
pub fn attention_items(st: &State) -> Vec<(usize, Status)> {
    st.sessions
        .iter()
        .flat_map(|s| s.windows.iter())
        .flat_map(|w| w.panes())
        .map(|pid| (pid, pane_status(st, pid)))
        .collect()
}

fn pane_status(st: &State, pid: usize) -> Status {
    st.panes
        .get(&pid)
        .map_or(Status::Idle, |p| crate::api::agent::status(p, &st.cfg))
}

/// Name shown for a pane: agent name, agent kind, label, then window name (as the sidebar does).
pub fn pane_who(st: &State, pid: usize) -> String {
    let Some(p) = st.panes.get(&pid) else {
        return pane_id_str(pid);
    };
    p.agent
        .as_ref()
        .map(|a| a.name.clone().unwrap_or_else(|| a.agent.clone()))
        .or_else(|| p.label.clone())
        .or_else(|| {
            st.locate(pid)
                .map(|(si, wi)| st.sessions[si].windows[wi].name.clone())
        })
        .unwrap_or_else(|| pane_id_str(pid))
}

/// (session name, window index, pane index within the window) of a pane.
fn pane_place(st: &State, pid: usize) -> Option<(String, usize, usize)> {
    let (si, wi) = st.locate(pid)?;
    let pi = st.sessions[si].windows[wi]
        .panes()
        .iter()
        .position(|p| *p == pid)?;
    Some((st.sessions[si].name.clone(), wi, pi))
}

/// `: message` from the agent's report, or nothing.
fn agent_note(st: &State, pid: usize) -> String {
    st.panes
        .get(&pid)
        .and_then(|p| p.agent.as_ref())
        .and_then(|a| a.message.as_deref())
        .map(|m| format!(": {m}"))
        .unwrap_or_default()
}

/// `▲ claude 0.1 blocked: need approval`
pub fn attention_text(st: &State, pid: usize, status: Status) -> String {
    let glyph = if status == Status::Blocked {
        "▲"
    } else {
        "◆"
    };
    let place = pane_place(st, pid)
        .map(|(_, wi, pi)| format!(" {wi}.{pi}"))
        .unwrap_or_default();
    format!(
        "{glyph} {}{place} {}{}",
        pane_who(st, pid),
        status.as_str(),
        agent_note(st, pid)
    )
}

/// Headless `next-attention` (CLI, no client): origin is the most recent session's focused pane.
/// Returns `pN blocked`/`pN done` and focuses the pane.
pub fn headless_attention(st: &mut State, backward: bool) -> Result<String, String> {
    let si = st.target_session(None)?;
    let origin = st.active_pane(si);
    let items = attention_items(st);
    let (pid, status) =
        pick_attention(&items, Some(origin), backward).ok_or("no other pane needs attention")?;
    st.focus_pane(pid);
    Ok(format!("{} {}", pane_id_str(pid), status.as_str()))
}

/// Toast for a pane that turned blocked while no client is looking at it: an info message on
/// every client that is not answering a prompt.
pub fn notify_blocked(st: &mut State, pid: usize) {
    let viewed = st.clients.values().any(|c| {
        st.sessions
            .iter()
            .any(|s| s.name == c.session && s.windows[s.active].active == pid)
    });
    if viewed {
        return;
    }
    let Some((session, wi, pi)) = pane_place(st, pid) else {
        return;
    };
    let text = format!(
        "▲ {} blocked in {session} {wi}.{pi}{} ({} Tab)",
        pane_who(st, pid),
        agent_note(st, pid),
        st.cfg.prefix_name()
    );
    let cids: Vec<usize> = st
        .clients
        .iter()
        .filter(|(_, c)| c.prompt.is_none())
        .map(|(id, _)| *id)
        .collect();
    for cid in cids {
        st.message(cid, text.clone());
    }
}

static ATTACHED: AtomicBool = AtomicBool::new(false);

/// True for the first client attach of this server process.
pub fn first_attach() -> bool {
    !ATTACHED.swap(true, Ordering::Relaxed)
}

/// First-attach hint for one client. `Message` lives 3 s, so it starts 5 s in the future to
/// stay visible for 8 s.
pub fn attach_hint(st: &mut State, cid: usize) {
    let p = st.cfg.prefix_name();
    let text = format!("{p} ? keys · {p} a agents · {p} Tab next alert");
    if let Some(c) = st.clients.get_mut(&cid) {
        c.message = Some(Message {
            text,
            at: Instant::now() + Duration::from_secs(5),
            error: false,
        });
    }
    st.mark_dirty();
}

fn chooser_index(st: &State, si: usize) -> usize {
    let cur = st.sessions[si].active;
    crate::render::chooser_entries(st)
        .iter()
        .position(|e| e.0 == si && e.1 == Some(cur))
        .unwrap_or(0)
}

/// `debug-frame`: render the target session with a throwaway client whose fields the flags set.
/// Without flags this is the plain ANSI frame; `--text` returns `rows` plain lines.
pub fn debug_frame(st: &mut State, target: Option<&str>, o: &FrameOpts) -> Result<String, String> {
    let si = st.target_session(target)?;
    let (name, cols, rows) = {
        let s = &st.sessions[si];
        (s.name.clone(), s.cols, s.rows)
    };
    let default_sel = chooser_index(st, si);
    let chooser = o.chooser.map(|sel| sel.unwrap_or(default_sel));
    let popup = if o.popup {
        Some((key_lines(st), 0))
    } else {
        None
    };
    let mut c = Client {
        session: name,
        sender: Box::new(io::sink()),
        prefix: o.prefix,
        frame: Frame::default(),
        prompt: o.prompt.as_ref().map(|(kind, text)| match kind {
            PromptKind::Confirm => Prompt::confirm(text.clone(), String::new()),
            kind => Prompt::new(*kind, text.clone()),
        }),
        chooser,
        popup,
        message: o.message.as_ref().map(|text| Message {
            text: text.clone(),
            at: Instant::now(),
            error: o.error,
        }),
        drag: None,
        env: ClientEnv::default(),
    };
    // copy mode is a field of the pane; set it for this render only and put it back
    let pid = st.active_pane(si);
    // outer Some: the pane exists; inner: the copy state it had before
    let prev_copy: Option<Option<CopyState>> = match (o.copy, st.panes.get_mut(&pid)) {
        (true, Some(p)) => {
            let (cy, cx) = p.parser.screen().cursor_position();
            Some(p.copy.replace(CopyState {
                cx,
                cy,
                anchor: None,
                line: false,
            }))
        }
        _ => None,
    };
    let ansi = render_client(st, &mut c).unwrap_or_default();
    if let (Some(prev), Some(p)) = (prev_copy, st.panes.get_mut(&pid)) {
        p.copy = prev;
    }
    Ok(if o.text {
        screen_text(&ansi, cols, rows)
    } else {
        ansi
    })
}

/// Replay an emitted frame into a terminal of the session size and return its visible rows,
/// each padded to `cols` display columns, one per line.
pub fn screen_text(ansi: &str, cols: u16, rows: u16) -> String {
    let (cols, rows) = (cols.max(2), rows.max(2));
    let mut parser = vt100::Parser::new(rows, cols, 0);
    let _ = crate::server::contain_vt(&mut parser, rows, cols, 0, |vt| vt.process(ansi.as_bytes()));
    let mut out = String::new();
    for line in parser.screen().rows(0, cols) {
        let pad = usize::from(cols).saturating_sub(line.width());
        out += &line;
        out += &" ".repeat(pad);
        out.push('\n');
    }
    out
}

impl State {
    /// `next-attention` from an attached client: switch the client to the target's session
    /// and focus it.
    pub(crate) fn next_attention(&mut self, cid: usize, si: usize, backward: bool) {
        let origin = self.active_pane(si);
        let items = attention_items(self);
        let Some((pid, status)) = pick_attention(&items, Some(origin), backward) else {
            self.message(cid, "no pane needs attention");
            return;
        };
        let Some((nsi, _)) = self.locate(pid) else {
            return;
        };
        self.switch_session(cid, nsi);
        self.focus_pane(pid);
        let text = attention_text(self, pid, status);
        self.message(cid, text);
    }

    /// `(session index, window index)` for `SESSION:IDX`, `SESSION` (its current window) or,
    /// with an empty session part, the default session.
    pub(crate) fn window_for(
        &self,
        default_si: Option<usize>,
        target: Option<&str>,
    ) -> Result<(usize, usize), String> {
        let Some(t) = target else {
            let si = default_si.ok_or("kill-window: needs -t SESSION:IDX")?;
            return Ok((si, self.sessions[si].active));
        };
        let (name, idx) = match t.rsplit_once(':') {
            Some((n, i)) => (n, Some(i)),
            None => (t, None),
        };
        let si = if name.is_empty() {
            default_si.ok_or_else(|| format!("can't find session: {t}"))?
        } else {
            self.sidx(name)
                .ok_or_else(|| format!("can't find session: {name}"))?
        };
        let sess = &self.sessions[si];
        let wi = match idx {
            None => sess.active,
            Some(i) => i
                .parse::<usize>()
                .ok()
                .filter(|w| *w < sess.windows.len())
                .ok_or_else(|| format!("can't find window: {t}"))?,
        };
        Ok((si, wi))
    }

    pub(crate) fn kill_window_at(&mut self, si: usize, wi: usize) {
        for pid in self.sessions[si].windows[wi].panes() {
            self.pane_exited(pid);
        }
    }

    /// Question for a `confirm-before` without `-p`: names the target and its agent state.
    pub(crate) fn confirm_prompt_text(&self, si: usize, command: &str) -> Result<String, String> {
        Ok(match parse(command)? {
            Cmd::KillPane => {
                let pid = self.active_pane(si);
                format!(
                    "kill pane {} ({}, {})?",
                    pane_id_str(pid),
                    pane_who(self, pid),
                    pane_status(self, pid).as_str()
                )
            }
            Cmd::KillWindow { target } => {
                let (s, w) = self.window_for(Some(si), target.as_deref())?;
                let win = &self.sessions[s].windows[w];
                let panes = win.panes();
                format!(
                    "kill window {} ({}{})?",
                    win.name,
                    count(panes.len(), "pane"),
                    blocked_note(self, &panes)
                )
            }
            Cmd::KillSession { target } => {
                let s = match target {
                    None => si,
                    Some(t) => self.target_session(Some(t.as_str()))?,
                };
                let sess = &self.sessions[s];
                let panes: Vec<usize> = sess.windows.iter().flat_map(|w| w.panes()).collect();
                format!(
                    "kill session {} ({}{})?",
                    sess.name,
                    count(sess.windows.len(), "window"),
                    blocked_note(self, &panes)
                )
            }
            _ => format!("{command}?"),
        })
    }
}

fn count(n: usize, unit: &str) -> String {
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn blocked_note(st: &State, panes: &[usize]) -> String {
    let n = panes
        .iter()
        .filter(|p| pane_status(st, **p) == Status::Blocked)
        .count();
    if n == 0 {
        String::new()
    } else {
        format!(", {n} blocked")
    }
}
