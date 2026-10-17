//! The multiplexer server: owns sessions, windows, panes (ptys) and renders frames for clients.
use crate::commands::{self, Cmd, key_name};
use crate::config::{Config, config_path};
use crate::copy::{self, CopyState};
use crate::ipc;
use crate::keys;
use crate::layout::{Dir, Move, Node, Rect, neighbor};
use crate::proto::{ClientMsg, Command, ServerMsg, read_msg, write_msg};
use crate::render::{Frame, chooser_entries, render_client};
use anyhow::{Result, anyhow};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use interprocess::local_socket::prelude::*;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub struct Pane {
    pub parser: vt100::Parser,
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub rect: Rect,
    pub last_output: Instant,
    pub attention: bool,
    pub copy: Option<CopyState>,
    /// reported agent state (herdr-style `pane report-agent`)
    pub agent: Option<crate::api::AgentInfo>,
    /// user label (`pane rename`)
    pub label: Option<String>,
    /// working directory the pane was started in
    pub cwd: Option<String>,
}

impl Pane {
    pub fn busy(&self, cfg: &Config) -> bool {
        self.last_output.elapsed().as_secs_f32() < cfg.busy_secs
    }

    /// Name of the foreground process in this pane, if the platform lets us find it.
    pub fn foreground_name(&self) -> Option<String> {
        #[cfg(unix)]
        {
            let pid = self.master.process_group_leader()?;
            #[cfg(target_os = "linux")]
            {
                return std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .ok()
                    .map(|s| s.trim().to_string());
            }
            #[cfg(target_os = "macos")]
            {
                let mut buf = [0u8; 256];
                let n = unsafe {
                    libc::proc_name(pid, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32)
                };
                if n > 0 {
                    return Some(String::from_utf8_lossy(&buf[..n as usize]).into_owned());
                }
                return None;
            }
            #[allow(unreachable_code)]
            {
                let _ = pid;
                None
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}

pub struct Window {
    pub name: String,
    pub manual_name: bool,
    pub layout: Node,
    pub active: usize,
    pub zoomed: bool,
}

impl Window {
    pub fn panes(&self) -> Vec<usize> {
        let mut v = vec![];
        self.layout.leaves(&mut v);
        v
    }
}

pub struct Session {
    pub name: String,
    pub windows: Vec<Window>,
    pub active: usize,
    pub cols: u16,
    pub rows: u16,
    pub sidebar: bool,
    pub cwd: Option<String>,
    pub last_used: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PromptKind {
    Command,
    RenameWindow,
    RenameSession,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub buf: String,
}

pub struct Client {
    pub session: String,
    pub sender: Box<dyn Write + Send>,
    pub prefix: bool,
    pub frame: Frame,
    pub prompt: Option<Prompt>,
    pub chooser: Option<usize>,
    pub popup: Option<(Vec<String>, usize)>,
    pub message: Option<(String, Instant)>,
    /// mouse drag start: (pane, abs row, col), and whether we moved
    pub drag: Option<(usize, i64, u16, bool)>,
}

pub struct State {
    pub cfg: Config,
    pub bindings: BTreeMap<String, String>,
    pub buffer: Option<String>,
    pub panes: HashMap<usize, Pane>,
    pub sessions: Vec<Session>,
    pub clients: HashMap<usize, Client>,
    pub next_id: usize,
    pub dirty: bool,
    notify: mpsc::Sender<()>,
}

pub type Shared = Arc<Mutex<State>>;

impl State {
    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    pub fn sidebar_width(&self, s: &Session) -> u16 {
        if s.sidebar && s.cols > self.cfg.sidebar_width + 20 {
            self.cfg.sidebar_width
        } else {
            0
        }
    }

    pub fn pane_area(&self, s: &Session) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: s.cols - self.sidebar_width(s),
            h: s.rows.saturating_sub(1).max(1),
        }
    }

    pub fn visible_rects(&self, s: &Session, w: &Window) -> Vec<(usize, Rect)> {
        let area = self.pane_area(s);
        if w.zoomed {
            vec![(w.active, area)]
        } else {
            let mut v = vec![];
            w.layout.layout(area, &mut v);
            v
        }
    }

    pub(crate) fn sidx(&self, name: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.name == name)
    }

    fn client_si(&self, cid: usize) -> Option<usize> {
        self.clients.get(&cid).and_then(|c| self.sidx(&c.session))
    }

    pub(crate) fn active_pane(&self, si: usize) -> usize {
        let s = &self.sessions[si];
        s.windows[s.active].active
    }

    fn message(&mut self, cid: usize, msg: impl Into<String>) {
        if let Some(c) = self.clients.get_mut(&cid) {
            c.message = Some((msg.into(), Instant::now()));
        }
        self.mark_dirty();
    }

    pub(crate) fn spawn_pane(
        &mut self,
        shared: &Shared,
        rows: u16,
        cols: u16,
        cwd: Option<&str>,
    ) -> Result<usize> {
        let id = self.id();
        let pty = native_pty_system();
        let size = PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty.openpty(size).map_err(|e| anyhow!("{e}"))?;
        let shell = self.cfg.default_shell();
        let words = commands::tokenize(&shell);
        let mut cmd = CommandBuilder::new(words.first().map(|s| s.as_str()).unwrap_or("sh"));
        for a in words.iter().skip(1) {
            cmd.arg(a);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("TMUXLIKE", format!("{id}"));
        cmd.env("TMUXLIKE_PANE_ID", crate::api::pane_id_str(id));
        cmd.env("TMUXLIKE_SOCKET", ipc::base_name());
        let start_dir = match cwd.filter(|d| std::path::Path::new(d).is_dir()) {
            Some(d) => Some(d.to_string()),
            None => dirs::home_dir().map(|h| h.to_string_lossy().into_owned()),
        };
        if let Some(d) = &start_dir {
            cmd.cwd(d);
        }
        crate::log!("pane {id}: spawning {shell:?} {}x{}", size.cols, size.rows);
        let child = pair.slave.spawn_command(cmd).map_err(|e| {
            crate::log!("pane {id}: spawn failed: {e}");
            anyhow!("{e}")
        })?;
        crate::log!("pane {id}: spawned pid {:?}", child.process_id());
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| anyhow!("{e}"))?;
        let writer = pair.master.take_writer().map_err(|e| anyhow!("{e}"))?;
        let pane = Pane {
            parser: vt100::Parser::new(size.rows, size.cols, self.cfg.scrollback),
            master: pair.master,
            writer,
            child,
            rect: Rect {
                x: 0,
                y: 0,
                w: size.cols,
                h: size.rows,
            },
            last_output: Instant::now(),
            attention: false,
            copy: None,
            agent: None,
            label: None,
            cwd: start_dir,
        };
        self.panes.insert(id, pane);
        let shared = shared.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let mut logged = 0usize;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        crate::log!("pane {id}: EOF");
                        break;
                    }
                    Err(e) => {
                        crate::log!("pane {id}: read error {e}");
                        break;
                    }
                    Ok(n) => {
                        if logged < 8 {
                            logged += 1;
                            crate::log!(
                                "pane {id}: read {n} bytes: {:?}",
                                String::from_utf8_lossy(&buf[..n.min(300)])
                            );
                        }
                        let mut st = shared.lock().unwrap();
                        let Some(p) = st.panes.get_mut(&id) else {
                            return;
                        };
                        p.parser.process(&buf[..n]);
                        // Answer terminal queries (ConPTY and many apps block waiting for these).
                        let replies = crate::keys::query_replies(
                            &buf[..n],
                            p.parser.screen().cursor_position(),
                        );
                        if !replies.is_empty() {
                            crate::log!(
                                "pane {id}: answering queries {:?}",
                                String::from_utf8_lossy(&replies)
                            );
                            let _ = p.writer.write_all(&replies);
                            let _ = p.writer.flush();
                        }
                        p.last_output = Instant::now();
                        p.attention = true;
                        st.dirty = true;
                        let _ = st.notify.send(());
                    }
                }
            }
            let mut st = shared.lock().unwrap();
            st.pane_exited(id);
        });
        Ok(id)
    }

    fn shell_name(&self) -> String {
        let s = self.cfg.default_shell();
        let first = commands::tokenize(&s)
            .into_iter()
            .next()
            .unwrap_or_else(|| "sh".into());
        std::path::Path::new(&first)
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or(first)
    }

    pub(crate) fn new_session(
        &mut self,
        shared: &Shared,
        name: Option<String>,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
    ) -> Result<String> {
        let name = match name {
            Some(n) => {
                if self.sidx(&n).is_some() {
                    return Err(anyhow!("duplicate session: {n}"));
                }
                n
            }
            None => (0..)
                .map(|i: u32| i.to_string())
                .find(|n| self.sidx(n).is_none())
                .unwrap(),
        };
        let sidebar = self.cfg.sidebar;
        self.sessions.push(Session {
            name: name.clone(),
            windows: vec![],
            active: 0,
            cols: cols.max(10),
            rows: rows.max(3),
            sidebar,
            cwd,
            last_used: Instant::now(),
        });
        let si = self.sessions.len() - 1;
        if let Err(e) = self.new_window(shared, si, None) {
            self.sessions.pop();
            return Err(e);
        }
        Ok(name)
    }

    pub(crate) fn new_window(
        &mut self,
        shared: &Shared,
        si: usize,
        name: Option<String>,
    ) -> Result<()> {
        let area = self.pane_area(&self.sessions[si]);
        let cwd = self.sessions[si].cwd.clone();
        let id = self.spawn_pane(shared, area.h, area.w, cwd.as_deref())?;
        let manual = name.is_some();
        let name = name.unwrap_or_else(|| self.shell_name());
        let s = &mut self.sessions[si];
        s.windows.push(Window {
            name,
            manual_name: manual,
            layout: Node::Leaf(id),
            active: id,
            zoomed: false,
        });
        s.active = s.windows.len() - 1;
        self.relayout(si);
        Ok(())
    }

    fn split(&mut self, shared: &Shared, si: usize, dir: Dir) -> Result<()> {
        let pid = self.active_pane(si);
        self.split_pane(shared, pid, dir, None, None, true)
            .map(|_| ())
    }

    /// Split pane `pid` (the new pane goes right/below). `ratio` = share of the existing pane.
    /// cwd defaults to the source pane's start dir. Returns the new pane id.
    pub(crate) fn split_pane(
        &mut self,
        shared: &Shared,
        pid: usize,
        dir: Dir,
        ratio: Option<f32>,
        cwd: Option<String>,
        focus: bool,
    ) -> Result<usize> {
        let (si, wi) = self.locate(pid).ok_or_else(|| anyhow!("pane not found"))?;
        let cur = self.panes.get(&pid).map(|p| p.rect).unwrap_or_default();
        let (h, wd) = if dir == Dir::H {
            (cur.h, cur.w / 2)
        } else {
            (cur.h / 2, cur.w)
        };
        let cwd = cwd
            .or_else(|| self.panes.get(&pid).and_then(|p| p.cwd.clone()))
            .or_else(|| self.sessions[si].cwd.clone());
        let id = self.spawn_pane(shared, h, wd, cwd.as_deref())?;
        let w = &mut self.sessions[si].windows[wi];
        w.layout
            .split_with(pid, id, dir, ratio.unwrap_or(0.5).clamp(0.05, 0.95));
        w.zoomed = false;
        if focus {
            self.focus_pane(id);
        }
        self.relayout(si);
        Ok(id)
    }

    /// Make `pid` the focused pane of its window/session and mark it seen.
    pub(crate) fn focus_pane(&mut self, pid: usize) -> bool {
        let Some((si, wi)) = self.locate(pid) else {
            return false;
        };
        let s = &mut self.sessions[si];
        s.active = wi;
        s.last_used = Instant::now();
        let w = &mut s.windows[wi];
        if w.active != pid {
            w.zoomed = false;
        }
        w.active = pid;
        if let Some(p) = self.panes.get_mut(&pid) {
            p.attention = false;
        }
        self.relayout(si);
        true
    }

    /// Close one pane (kills its process) and clean up its window/session.
    pub(crate) fn kill_pane(&mut self, pid: usize) -> bool {
        if !self.panes.contains_key(&pid) {
            return false;
        }
        self.pane_exited(pid);
        true
    }

    pub fn relayout(&mut self, si: usize) {
        let s = &self.sessions[si];
        let mut all = vec![];
        for w in &s.windows {
            all.extend(self.visible_rects(s, w));
        }
        for (id, r) in all {
            if let Some(p) = self.panes.get_mut(&id) {
                if p.rect != r && r.w > 0 && r.h > 0 {
                    p.rect = r;
                    p.parser.screen_mut().set_size(r.h, r.w);
                    let _ = p.master.resize(PtySize {
                        rows: r.h,
                        cols: r.w,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                    if let Some(c) = &mut p.copy {
                        c.cx = c.cx.min(r.w - 1);
                        c.cy = c.cy.min(r.h - 1);
                    }
                }
            }
        }
        self.mark_dirty();
    }

    fn relayout_all(&mut self) {
        for si in 0..self.sessions.len() {
            self.relayout(si);
        }
        for c in self.clients.values_mut() {
            c.frame.invalidate();
        }
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
        let _ = self.notify.send(());
    }

    pub(crate) fn locate(&self, pid: usize) -> Option<(usize, usize)> {
        for (si, s) in self.sessions.iter().enumerate() {
            for (wi, w) in s.windows.iter().enumerate() {
                if w.layout.contains(pid) {
                    return Some((si, wi));
                }
            }
        }
        None
    }

    /// Remove a pane (after its process exited or it was killed) and clean up empty windows/sessions.
    pub fn pane_exited(&mut self, pid: usize) {
        if let Some(mut p) = self.panes.remove(&pid) {
            let _ = p.child.kill();
        }
        let Some((si, wi)) = self.locate(pid) else {
            return;
        };
        let s = &mut self.sessions[si];
        let w = &mut s.windows[wi];
        if matches!(w.layout, Node::Leaf(_)) {
            s.windows.remove(wi);
            if s.active >= s.windows.len() && s.active > 0 {
                s.active -= 1;
            }
        } else {
            w.layout.remove(pid);
            if w.active == pid {
                w.active = w.panes()[0];
            }
            w.zoomed = false;
        }
        if self.sessions[si].windows.is_empty() {
            let name = self.sessions.remove(si).name;
            self.drop_clients_of(&name, "exited");
        } else {
            self.relayout(si);
        }
        self.mark_dirty();
    }

    fn drop_clients_of(&mut self, session: &str, reason: &str) {
        // Move clients to another session if one exists (like tmux detach-on-destroy off)
        let other = self
            .sessions
            .iter()
            .max_by_key(|s| s.last_used)
            .map(|s| s.name.clone());
        let ids: Vec<usize> = self
            .clients
            .iter()
            .filter(|(_, c)| c.session == session)
            .map(|(i, _)| *i)
            .collect();
        for id in ids {
            if let Some(o) = &other {
                let c = self.clients.get_mut(&id).unwrap();
                c.session = o.clone();
                c.frame.invalidate();
                continue;
            }
            if let Some(mut c) = self.clients.remove(&id) {
                let _ = write_msg(&mut c.sender, &ServerMsg::Exit(reason.into()));
            }
        }
    }

    pub(crate) fn kill_session(&mut self, si: usize) {
        let s = self.sessions.remove(si);
        for w in &s.windows {
            for pid in w.panes() {
                if let Some(mut p) = self.panes.remove(&pid) {
                    let _ = p.child.kill();
                }
            }
        }
        self.drop_clients_of(&s.name, "session killed");
    }

    pub(crate) fn write_pane(&mut self, pid: usize, bytes: &[u8]) {
        if let Some(p) = self.panes.get_mut(&pid) {
            if p.copy.is_some() {
                p.copy = None;
                p.parser.screen_mut().set_scrollback(0);
                self.dirty = true;
            }
            let _ = p.writer.write_all(bytes);
            let _ = p.writer.flush();
        }
    }

    pub(crate) fn paste(&mut self, pid: usize, text: &str) {
        let bp = self
            .panes
            .get(&pid)
            .map(|p| p.parser.screen().bracketed_paste())
            .unwrap_or(false);
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let data = if bp {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text
        };
        self.write_pane(pid, data.as_bytes());
    }

    fn set_clipboard(&mut self, cid: usize, text: String) {
        let n = text.chars().count();
        if self.cfg.osc52 {
            let seq = copy::osc52(&text);
            for c in self.clients.values_mut() {
                let _ = write_msg(&mut c.sender, &ServerMsg::Output(seq.clone()));
            }
        }
        self.buffer = Some(text);
        self.message(cid, format!("copied {n} characters"));
    }

    fn switch_session(&mut self, cid: usize, ni: usize) {
        let Some(si) = self.client_si(cid) else {
            return;
        };
        let (cols, rows) = (self.sessions[si].cols, self.sessions[si].rows);
        let s = &mut self.sessions[ni];
        s.cols = cols;
        s.rows = rows;
        s.last_used = Instant::now();
        let name = s.name.clone();
        if let Some(c) = self.clients.get_mut(&cid) {
            c.session = name;
            c.frame.invalidate();
        }
        self.relayout(ni);
    }

    pub fn reload_config(&mut self, path: Option<&str>) -> Result<String, String> {
        let p = match path {
            Some(p) => std::path::PathBuf::from(p),
            None => config_path().ok_or("no config path")?,
        };
        let is_toml = p.extension().is_some_and(|e| e == "toml");
        if is_toml {
            let cfg = if p.exists() {
                Config::load_from(&p)?
            } else {
                Config::default()
            };
            self.cfg = cfg;
            let (b, errs) = self.cfg.bindings();
            self.bindings = b;
            let mut errs = errs;
            for c in self.cfg.commands.clone() {
                if let Err(e) = commands::parse(&c).and_then(|cmd| self.exec_global(cmd)) {
                    errs.push(e);
                }
            }
            self.relayout_all();
            if errs.is_empty() {
                Ok(format!("sourced {}", p.display()))
            } else {
                Err(errs.join("; "))
            }
        } else {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let mut errs = vec![];
            for line in text.lines() {
                if commands::tokenize(line).is_empty() {
                    continue;
                }
                if let Err(e) = commands::parse(line).and_then(|cmd| self.exec_global(cmd)) {
                    errs.push(e);
                }
            }
            self.relayout_all();
            if errs.is_empty() {
                Ok(format!("sourced {}", p.display()))
            } else {
                Err(errs.join("; "))
            }
        }
    }

    pub(crate) fn target_session(&self, target: Option<&str>) -> Result<usize, String> {
        match target {
            Some(t) => {
                let name = t.split([':', '.']).next().unwrap_or(t);
                self.sidx(name)
                    .ok_or_else(|| format!("can't find session: {name}"))
            }
            None => self
                .sessions
                .iter()
                .enumerate()
                .max_by_key(|(_, s)| s.last_used)
                .map(|(i, _)| i)
                .ok_or_else(|| "no sessions".to_string()),
        }
    }

    /// Visible text of the active pane of a session (like `tmux capture-pane -p`).
    pub fn capture(&self, target: Option<&str>) -> Result<String, String> {
        let si = self.target_session(target)?;
        let p = self.panes.get(&self.active_pane(si)).ok_or("no pane")?;
        let mut out = String::new();
        for line in p.parser.screen().contents().lines() {
            out.push_str(line.trim_end());
            out.push('\n');
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        Ok(out)
    }

    /// Commands that do not need a client/session.
    fn exec_global(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::Set(k, v) => {
                self.cfg.set(&k, &v)?;
                if k == "sidebar" {
                    let sb = self.cfg.sidebar;
                    for s in &mut self.sessions {
                        s.sidebar = sb;
                    }
                }
                self.relayout_all();
            }
            Cmd::BindKey(k, c) => {
                self.bindings.insert(k, c);
            }
            Cmd::UnbindKey(k) => {
                if k == "*" {
                    self.bindings.clear();
                } else {
                    self.bindings.remove(&k);
                }
            }
            Cmd::SourceFile(p) => {
                self.reload_config(p.as_deref())?;
            }
            Cmd::SendKeys {
                target,
                literal,
                keys,
            } => {
                let si = self.target_session(target.as_deref())?;
                let pid = self.active_pane(si);
                let bytes: Vec<u8> = keys
                    .iter()
                    .flat_map(|k| commands::send_key_bytes(k, literal))
                    .collect();
                self.write_pane(pid, &bytes);
            }
            Cmd::CapturePane { target, .. } => {
                let text = self.capture(target.as_deref())?;
                self.buffer = Some(text);
            }
            other => return Err(format!("{other:?}: needs an attached client")),
        }
        Ok(())
    }

    pub fn exec(&mut self, shared: &Shared, cid: usize, cmd: Cmd) -> Result<(), String> {
        let Some(si) = self.client_si(cid) else {
            return self.exec_global(cmd);
        };
        let e = |e: anyhow::Error| e.to_string();
        match cmd {
            Cmd::SplitWindow { horizontal } => self
                .split(shared, si, if horizontal { Dir::H } else { Dir::V })
                .map_err(e)?,
            Cmd::NewWindow { name } => self.new_window(shared, si, name).map_err(e)?,
            Cmd::RenameWindow(None) | Cmd::RenameSession(None) | Cmd::CommandPrompt => {
                let s = &self.sessions[si];
                let (kind, buf) = match cmd {
                    Cmd::RenameWindow(_) => {
                        (PromptKind::RenameWindow, s.windows[s.active].name.clone())
                    }
                    Cmd::RenameSession(_) => (PromptKind::RenameSession, s.name.clone()),
                    _ => (PromptKind::Command, String::new()),
                };
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.prompt = Some(Prompt { kind, buf });
                }
            }
            Cmd::RenameWindow(Some(n)) => {
                let s = &mut self.sessions[si];
                let w = &mut s.windows[s.active];
                w.manual_name = !n.is_empty();
                if !n.is_empty() {
                    w.name = n;
                }
            }
            Cmd::RenameSession(Some(n)) => {
                if n.is_empty() || n.contains(':') {
                    return Err("invalid session name".into());
                }
                if self.sidx(&n).is_some_and(|i| i != si) {
                    return Err(format!("duplicate session: {n}"));
                }
                let old = std::mem::replace(&mut self.sessions[si].name, n.clone());
                for c in self.clients.values_mut() {
                    if c.session == old {
                        c.session = n.clone();
                    }
                }
            }
            Cmd::KillPane => {
                let pid = self.active_pane(si);
                self.pane_exited(pid);
            }
            Cmd::KillWindow => {
                let s = &self.sessions[si];
                for pid in s.windows[s.active].panes() {
                    self.pane_exited(pid);
                }
            }
            Cmd::KillSession => self.kill_session(si),
            Cmd::KillServer => {
                crate::persist::clear();
                while !self.sessions.is_empty() {
                    self.kill_session(0);
                }
                std::process::exit(0);
            }
            Cmd::SelectPane(m) => {
                let s = &self.sessions[si];
                let w = &s.windows[s.active];
                let rects = self.visible_rects(s, w);
                if let Some(n) = neighbor(&rects, w.active, m) {
                    let s = &mut self.sessions[si];
                    let w = &mut s.windows[s.active];
                    w.active = n;
                    w.zoomed = false;
                    self.relayout(si);
                }
            }
            Cmd::NextPane => {
                let s = &mut self.sessions[si];
                let w = &mut s.windows[s.active];
                let panes = w.panes();
                let i = panes.iter().position(|p| *p == w.active).unwrap_or(0);
                w.active = panes[(i + 1) % panes.len()];
                if w.zoomed {
                    self.relayout(si);
                }
            }
            Cmd::ResizePane(m, n) => {
                let area = self.pane_area(&self.sessions[si]);
                let s = &mut self.sessions[si];
                let w = &mut s.windows[s.active];
                let (dir, sign, total) = match m {
                    Move::Left => (Dir::H, -1.0, area.w),
                    Move::Right => (Dir::H, 1.0, area.w),
                    Move::Up => (Dir::V, -1.0, area.h),
                    Move::Down => (Dir::V, 1.0, area.h),
                };
                let target = w.active;
                w.layout
                    .resize(target, dir, sign * n as f32 / total.max(1) as f32);
                self.relayout(si);
            }
            Cmd::Zoom => {
                let s = &mut self.sessions[si];
                let w = &mut s.windows[s.active];
                w.zoomed = !w.zoomed && w.panes().len() > 1;
                self.relayout(si);
            }
            Cmd::NextWindow | Cmd::PrevWindow | Cmd::SelectWindow(_) => {
                let s = &mut self.sessions[si];
                let n = s.windows.len();
                s.active = match cmd {
                    Cmd::NextWindow => (s.active + 1) % n,
                    Cmd::PrevWindow => (s.active + n - 1) % n,
                    Cmd::SelectWindow(i) if i < n => i,
                    _ => return Err("no such window".into()),
                };
                self.relayout(si);
            }
            Cmd::CopyMode { page_up } => {
                let pid = self.active_pane(si);
                if let Some(p) = self.panes.get_mut(&pid) {
                    let (cy, cx) = p.parser.screen().cursor_position();
                    let c = p.copy.get_or_insert(CopyState {
                        cx,
                        cy,
                        anchor: None,
                        line: false,
                    });
                    if page_up {
                        let off = p.parser.screen().scrollback() + p.rect.h as usize;
                        p.parser.screen_mut().set_scrollback(off);
                        c.cy = 0;
                    }
                }
            }
            Cmd::PasteBuffer => {
                if let Some(b) = self.buffer.clone() {
                    let pid = self.active_pane(si);
                    self.paste(pid, &b);
                } else {
                    return Err("no buffer".into());
                }
            }
            Cmd::Detach => {
                if let Some(mut c) = self.clients.remove(&cid) {
                    let _ = write_msg(
                        &mut c.sender,
                        &ServerMsg::Exit(format!("detached (from session {})", c.session)),
                    );
                }
            }
            Cmd::ChooseTree => {
                let entries = chooser_entries(self);
                let cur = self.sessions[si].active;
                let sel = entries
                    .iter()
                    .position(|e| e.0 == si && e.1 == Some(cur))
                    .unwrap_or(0);
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.chooser = Some(sel);
                }
            }
            Cmd::ToggleSidebar => {
                let s = &mut self.sessions[si];
                s.sidebar = !s.sidebar;
                self.relayout(si);
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.frame.invalidate();
                }
            }
            Cmd::SourceFile(p) => {
                let m = self.reload_config(p.as_deref())?;
                self.message(cid, m);
            }
            Cmd::SwitchClient { next } => {
                let n = self.sessions.len();
                if n > 1 {
                    let ni = if next { (si + 1) % n } else { (si + n - 1) % n };
                    self.switch_session(cid, ni);
                }
            }
            Cmd::SendPrefix => {
                let (c, ctrl) = {
                    let p = self.cfg.prefix_name();
                    match p.strip_prefix("C-") {
                        Some(r) if r.len() == 1 => (r.chars().next().unwrap(), true),
                        _ => (p.chars().next().unwrap_or('b'), false),
                    }
                };
                let k = KeyEvent::new(
                    KeyCode::Char(c),
                    if ctrl {
                        KeyModifiers::CONTROL
                    } else {
                        KeyModifiers::NONE
                    },
                );
                let pid = self.active_pane(si);
                self.write_pane(pid, &keys::encode(&k, false));
            }
            Cmd::DisplayMessage(m) => self.message(cid, m),
            Cmd::ListKeys => {
                let mut lines = vec![
                    format!(
                        "prefix = {}   (j/k/Space scroll, q closes)",
                        self.cfg.prefix_name()
                    ),
                    String::new(),
                ];
                for (k, c) in &self.bindings {
                    lines.push(format!("{k:>10}  {c}"));
                }
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.popup = Some((lines, 0));
                }
            }
            other => self.exec_global(other)?,
        }
        self.mark_dirty();
        Ok(())
    }

    fn run_line(&mut self, shared: &Shared, cid: usize, line: &str) {
        if let Err(e) = commands::parse(line).and_then(|c| self.exec(shared, cid, c)) {
            self.message(cid, e);
        }
    }

    fn handle_event(&mut self, shared: &Shared, cid: usize, ev: Event) {
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

    fn handle_mouse(&mut self, cid: usize, si: usize, m: crossterm::event::MouseEvent) {
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

    fn copy_selection(&mut self, cid: usize, pid: usize) {
        let Some(p) = self.panes.get_mut(&pid) else {
            return;
        };
        let Some(cs) = p.copy.clone() else { return };
        let Some(anchor) = cs.anchor else { return };
        let off = p.parser.screen().scrollback();
        let cur = (copy::abs_row(cs.cy, off), cs.cx);
        let text = copy::extract(&mut p.parser, anchor, cur, cs.line);
        p.copy = None;
        p.parser.screen_mut().set_scrollback(0);
        self.set_clipboard(cid, text);
    }

    fn handle_key(&mut self, shared: &Shared, cid: usize, si: usize, k: KeyEvent) {
        let Some(c) = self.clients.get_mut(&cid) else {
            return;
        };
        // overlays first
        if let Some((lines, off)) = &mut c.popup {
            let max = lines.len().saturating_sub(1);
            match k.code {
                KeyCode::Down | KeyCode::Char('j') => *off = (*off + 1).min(max),
                KeyCode::Up | KeyCode::Char('k') => *off = off.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char(' ') => *off = (*off + 10).min(max),
                KeyCode::PageUp => *off = off.saturating_sub(10),
                _ => c.popup = None,
            }
            self.mark_dirty();
            return;
        }
        if let Some(sel) = c.chooser {
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
            return;
        }
        if let Some(p) = &mut c.prompt {
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
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    c.prompt = None
                }
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.clear(),
                KeyCode::Backspace => {
                    p.buf.pop();
                }
                KeyCode::Char(ch) if !k.modifiers.contains(KeyModifiers::CONTROL) => p.buf.push(ch),
                _ => {}
            }
            self.mark_dirty();
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
            match self.bindings.get(&name).cloned() {
                Some(line) => self.run_line(shared, cid, &line),
                None => {}
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

    #[allow(unused_assignments)]
    fn copy_key(&mut self, cid: usize, pid: usize, k: KeyEvent) {
        let Some(p) = self.panes.get_mut(&pid) else {
            return;
        };
        let (h, w) = (p.rect.h, p.rect.w);
        let mut off = p.parser.screen().scrollback();
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let mut cs = p.copy.clone().unwrap();
        let scroll_to = |n: usize, parser: &mut vt100::Parser| -> usize {
            parser.screen_mut().set_scrollback(n);
            parser.screen().scrollback()
        };
        let row_text = |parser: &vt100::Parser, y: u16| -> String {
            (0..w)
                .map(|x| {
                    parser
                        .screen()
                        .cell(y, x)
                        .map(|c| c.contents().to_string())
                        .unwrap_or_default()
                })
                .map(|s| if s.is_empty() { " ".into() } else { s })
                .collect()
        };
        let mut done = false;
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc
                if cs.anchor.is_none() || k.code == KeyCode::Char('q') =>
            {
                done = true
            }
            KeyCode::Esc => cs.anchor = None,
            KeyCode::Char('c') if ctrl => done = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if cs.cy > 0 {
                    cs.cy -= 1
                } else {
                    off = scroll_to(off + 1, &mut p.parser)
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if cs.cy + 1 < h {
                    cs.cy += 1
                } else if off > 0 {
                    off = scroll_to(off - 1, &mut p.parser)
                }
            }
            KeyCode::Left | KeyCode::Char('h') => cs.cx = cs.cx.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => cs.cx = (cs.cx + 1).min(w - 1),
            KeyCode::Char('0') | KeyCode::Home => cs.cx = 0,
            KeyCode::Char('$') | KeyCode::End => {
                cs.cx =
                    (row_text(&p.parser, cs.cy).trim_end().chars().count() as u16).saturating_sub(1)
            }
            KeyCode::Char('w') => {
                let t: Vec<char> = row_text(&p.parser, cs.cy).chars().collect();
                let mut x = cs.cx as usize;
                while x < t.len() && !t[x].is_whitespace() {
                    x += 1;
                }
                while x < t.len() && t[x].is_whitespace() {
                    x += 1;
                }
                cs.cx = (x.min(t.len().saturating_sub(1))) as u16;
            }
            KeyCode::Char('b') if !ctrl => {
                let t: Vec<char> = row_text(&p.parser, cs.cy).chars().collect();
                let mut x = cs.cx as usize;
                while x > 0 && t.get(x - 1).is_some_and(|c| c.is_whitespace()) {
                    x -= 1;
                }
                while x > 0 && t.get(x - 1).is_some_and(|c| !c.is_whitespace()) {
                    x -= 1;
                }
                cs.cx = x as u16;
            }
            KeyCode::PageUp | KeyCode::Char('b') => {
                off = scroll_to(off + h as usize, &mut p.parser)
            }
            KeyCode::PageDown | KeyCode::Char('f') => {
                off = scroll_to(off.saturating_sub(h as usize), &mut p.parser)
            }
            KeyCode::Char('u') => off = scroll_to(off + h as usize / 2, &mut p.parser),
            KeyCode::Char('d') => {
                off = scroll_to(off.saturating_sub(h as usize / 2), &mut p.parser)
            }
            KeyCode::Char('g') => {
                off = scroll_to(usize::MAX, &mut p.parser);
                cs.cy = 0;
            }
            KeyCode::Char('G') => {
                off = scroll_to(0, &mut p.parser);
                cs.cy = h - 1;
            }
            KeyCode::Char('H') => cs.cy = 0,
            KeyCode::Char('L') => cs.cy = h - 1,
            KeyCode::Char('v') | KeyCode::Char(' ') => {
                cs.line = false;
                cs.anchor = if cs.anchor.is_some() && !cs.line {
                    None
                } else {
                    Some((copy::abs_row(cs.cy, off), cs.cx))
                };
            }
            KeyCode::Char('V') => {
                cs.line = true;
                cs.anchor = Some((copy::abs_row(cs.cy, off), cs.cx));
            }
            KeyCode::Char('y') | KeyCode::Enter => {
                if cs.anchor.is_some() {
                    p.copy = Some(cs);
                    self.copy_selection(cid, pid);
                    self.mark_dirty();
                    return;
                }
                done = true;
            }
            _ => {}
        }
        if done {
            p.copy = None;
            p.parser.screen_mut().set_scrollback(0);
        } else {
            p.copy = Some(cs);
        }
        self.mark_dirty();
    }
}

fn handle_conn(shared: Shared, cid: usize, stream: interprocess::local_socket::Stream) {
    let (mut r, mut w) = stream.split();
    let Ok(ClientMsg::Hello(cmd)) = read_msg::<ClientMsg>(&mut r) else {
        return;
    };
    match cmd {
        Command::List => {
            let st = shared.lock().unwrap();
            let mut out = String::new();
            for s in &st.sessions {
                let n = st.clients.values().filter(|c| c.session == s.name).count();
                out += &format!(
                    "{}: {} windows ({}x{}){}\n",
                    s.name,
                    s.windows.len(),
                    s.cols,
                    s.rows,
                    if n > 0 { " (attached)" } else { "" }
                );
            }
            let _ = write_msg(&mut w, &ServerMsg::Text(out));
        }
        Command::Kill { session } => {
            let mut st = shared.lock().unwrap();
            match st.sidx(&session) {
                Some(si) => {
                    st.kill_session(si);
                    let _ = write_msg(&mut w, &ServerMsg::Text(String::new()));
                }
                None => {
                    let _ = write_msg(
                        &mut w,
                        &ServerMsg::Error(format!("can't find session: {session}")),
                    );
                }
            }
        }
        Command::KillServer => {
            crate::persist::clear();
            let mut st = shared.lock().unwrap();
            while !st.sessions.is_empty() {
                st.kill_session(0);
            }
            let _ = write_msg(&mut w, &ServerMsg::Text(String::new()));
            std::process::exit(0);
        }
        Command::Api(req) => {
            // handlers lock `shared` themselves so waits can release it between polls
            let reply = crate::api::handle(&shared, req);
            let _ = write_msg(&mut w, &ServerMsg::Api(reply));
        }
        Command::Run { line } => {
            let mut st = shared.lock().unwrap();
            let res = commands::parse(&line).and_then(|c| match c {
                Cmd::CapturePane {
                    target,
                    print: true,
                } => st.capture(target.as_deref()),
                Cmd::DebugFrame { target } => {
                    let si = st.target_session(target.as_deref())?;
                    let mut c = Client {
                        session: st.sessions[si].name.clone(),
                        sender: Box::new(std::io::sink()),
                        prefix: false,
                        frame: Frame::default(),
                        prompt: None,
                        chooser: None,
                        popup: None,
                        message: None,
                        drag: None,
                    };
                    Ok(render_client(&st, &mut c).unwrap_or_default())
                }
                c => st.exec_global(c).map(|_| String::new()),
            });
            let msg = match res {
                Ok(t) => ServerMsg::Text(t),
                Err(e) => ServerMsg::Error(e),
            };
            let _ = write_msg(&mut w, &msg);
        }
        Command::Attach {
            session,
            create,
            cols,
            rows,
            cwd,
            detached,
        } => {
            let mut st = shared.lock().unwrap();
            let name = match session {
                Some(n) if st.sidx(&n).is_some() && !create => Ok(n),
                Some(n) if !create => Err(anyhow!("can't find session: {n}")),
                None if !create => st
                    .sessions
                    .iter()
                    .max_by_key(|s| s.last_used)
                    .map(|s| s.name.clone())
                    .ok_or_else(|| anyhow!("no sessions")),
                n => st.new_session(&shared, n, cols, rows, cwd),
            };
            let name = match name {
                Ok(n) => n,
                Err(e) => {
                    let _ = write_msg(&mut w, &ServerMsg::Error(e.to_string()));
                    return;
                }
            };
            let si = st.sidx(&name).unwrap();
            if detached {
                let _ = write_msg(&mut w, &ServerMsg::Text(String::new()));
                return;
            }
            {
                let s = &mut st.sessions[si];
                s.cols = cols.max(10);
                s.rows = rows.max(3);
                s.last_used = Instant::now();
            }
            st.relayout(si);
            st.clients.insert(
                cid,
                Client {
                    session: name,
                    sender: Box::new(w),
                    prefix: false,
                    frame: Frame::default(),
                    prompt: None,
                    chooser: None,
                    popup: None,
                    message: None,
                    drag: None,
                },
            );
            st.mark_dirty();
            drop(st);
            while let Ok(msg) = read_msg::<ClientMsg>(&mut r) {
                if let ClientMsg::Event(ev) = msg {
                    let mut st = shared.lock().unwrap();
                    if !st.clients.contains_key(&cid) {
                        break;
                    }
                    st.handle_event(&shared, cid, ev);
                }
            }
            shared.lock().unwrap().clients.remove(&cid);
        }
    }
}

pub fn run() -> Result<()> {
    crate::log!("server starting, pid {}", std::process::id());
    let listener = ipc::listen().inspect_err(|e| crate::log!("listen failed: {e}"))?;
    let (tx, rx) = mpsc::channel();
    let cfg = Config::load();
    let (bindings, _) = cfg.bindings();
    let shared: Shared = Arc::new(Mutex::new(State {
        cfg,
        bindings,
        buffer: None,
        panes: HashMap::new(),
        sessions: vec![],
        clients: HashMap::new(),
        next_id: 0,
        dirty: false,
        notify: tx,
    }));
    {
        let mut st = shared.lock().unwrap();
        for c in st.cfg.commands.clone() {
            let _ = commands::parse(&c).and_then(|cmd| st.exec_global(cmd));
        }
    }
    if shared.lock().unwrap().cfg.persist {
        match crate::persist::restore_on_start(&shared) {
            Ok(n) => crate::log!("restored {n} sessions"),
            Err(e) => crate::log!("restore failed: {e}"),
        }
    }

    // Render / housekeeping thread.
    let sh = shared.clone();
    std::thread::spawn(move || {
        let mut ever_had_session = false;
        let mut last_tick = Instant::now();
        let mut last_saved: Option<String> = None;
        loop {
            let _ = rx.recv_timeout(Duration::from_millis(500));
            std::thread::sleep(Duration::from_millis(8));
            while rx.try_recv().is_ok() {}
            let mut st = sh.lock().unwrap();
            let tick = last_tick.elapsed() > Duration::from_millis(500);
            if tick {
                last_tick = Instant::now();
                // detect exited children (ConPTY may not deliver EOF)
                let dead: Vec<usize> = st
                    .panes
                    .iter_mut()
                    .filter_map(|(id, p)| match p.child.try_wait() {
                        Ok(Some(status)) => {
                            crate::log!("pane {id}: child exited {status:?}");
                            Some(*id)
                        }
                        _ => None,
                    })
                    .collect();
                for id in dead {
                    st.pane_exited(id);
                }
                // automatic window names
                if st.cfg.automatic_rename {
                    let st = &mut *st;
                    for s in &mut st.sessions {
                        for w in &mut s.windows {
                            if w.manual_name {
                                continue;
                            }
                            if let Some(n) =
                                st.panes.get(&w.active).and_then(|p| p.foreground_name())
                            {
                                if !n.is_empty() && n != w.name {
                                    w.name = n;
                                    st.dirty = true;
                                }
                            }
                        }
                    }
                }
                if st.cfg.persist {
                    crate::persist::autosave(&st, &mut last_saved);
                }
                for c in st.clients.values_mut() {
                    if c.message
                        .as_ref()
                        .is_some_and(|m| m.1.elapsed() > Duration::from_secs(3))
                    {
                        c.message = None;
                    }
                }
            }
            if !st.sessions.is_empty() {
                ever_had_session = true;
            } else if ever_had_session {
                std::process::exit(0);
            }
            if !st.dirty && !tick {
                continue;
            }
            st.dirty = false;
            // clear attention for panes being looked at
            let viewed: Vec<usize> = st
                .clients
                .values()
                .filter_map(|c| st.sessions.iter().find(|s| s.name == c.session))
                .map(|s| s.windows[s.active].active)
                .collect();
            for id in viewed {
                if let Some(p) = st.panes.get_mut(&id) {
                    p.attention = false;
                }
            }
            let mut clients = std::mem::take(&mut st.clients);
            let mut gone = vec![];
            for (id, c) in clients.iter_mut() {
                if let Some(out) = render_client(&st, c) {
                    if !out.is_empty() && write_msg(&mut c.sender, &ServerMsg::Output(out)).is_err()
                    {
                        gone.push(*id);
                    }
                }
            }
            for id in gone {
                clients.remove(&id);
            }
            st.clients = clients;
        }
    });

    let mut cid = 0;
    for conn in listener.incoming().flatten() {
        cid += 1;
        let sh = shared.clone();
        let id = cid;
        std::thread::spawn(move || handle_conn(sh, id, conn));
    }
    Ok(())
}
