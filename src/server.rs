//! The multiplexer server: owns sessions, windows, panes (ptys) and renders frames for clients.
use crate::config::Config;
use crate::ipc;
use crate::keys;
use crate::layout::{Dir, Move, Node, Rect, neighbor};
use crate::proto::{ClientMsg, Command, ServerMsg, read_msg, write_msg};
use crate::render::{Frame, render_client};
use anyhow::{Result, anyhow};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use interprocess::local_socket::prelude::*;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[allow(dead_code)]
pub struct Pane {
    pub id: usize,
    pub parser: vt100::Parser,
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub rect: Rect,
    pub last_output: Instant,
    pub attention: bool,
    pub copy: bool,
}

impl Pane {
    pub fn busy(&self, cfg: &Config) -> bool {
        self.last_output.elapsed().as_secs_f32() < cfg.busy_secs
    }
}

pub struct Window {
    pub name: String,
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

pub struct Client {
    pub session: String,
    pub sender: Box<dyn Write + Send>,
    pub prefix: bool,
    pub frame: Frame,
}

pub struct State {
    pub cfg: Config,
    pub panes: HashMap<usize, Pane>,
    pub sessions: Vec<Session>,
    pub clients: HashMap<usize, Client>,
    pub next_id: usize,
    pub dirty: bool,
    notify: mpsc::Sender<()>,
}

type Shared = Arc<Mutex<State>>;

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

    fn sidx(&self, name: &str) -> Option<usize> {
        self.sessions.iter().position(|s| s.name == name)
    }

    fn spawn_pane(
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
        let mut parts = shell.split_whitespace();
        let mut cmd = CommandBuilder::new(parts.next().unwrap_or("sh"));
        for a in parts {
            cmd.arg(a);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("TMUXLIKE", format!("{id}"));
        if let Some(d) = cwd.filter(|d| std::path::Path::new(d).is_dir()) {
            cmd.cwd(d);
        } else if let Some(h) = dirs::home_dir() {
            cmd.cwd(h);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| anyhow!("{e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| anyhow!("{e}"))?;
        let writer = pair.master.take_writer().map_err(|e| anyhow!("{e}"))?;
        let pane = Pane {
            id,
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
            copy: false,
        };
        self.panes.insert(id, pane);
        let shared = shared.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut st = shared.lock().unwrap();
                        let Some(p) = st.panes.get_mut(&id) else {
                            return;
                        };
                        p.parser.process(&buf[..n]);
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
        let first = s.split_whitespace().next().unwrap_or("sh").to_string();
        std::path::Path::new(&first)
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or(first)
    }

    fn new_session(
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
        if let Err(e) = self.new_window(shared, si) {
            self.sessions.pop();
            return Err(e);
        }
        Ok(name)
    }

    fn new_window(&mut self, shared: &Shared, si: usize) -> Result<()> {
        let area = self.pane_area(&self.sessions[si]);
        let cwd = self.sessions[si].cwd.clone();
        let id = self.spawn_pane(shared, area.h, area.w, cwd.as_deref())?;
        let name = self.shell_name();
        let s = &mut self.sessions[si];
        s.windows.push(Window {
            name,
            layout: Node::Leaf(id),
            active: id,
            zoomed: false,
        });
        s.active = s.windows.len() - 1;
        self.relayout(si);
        Ok(())
    }

    fn split(&mut self, shared: &Shared, si: usize, dir: Dir) -> Result<()> {
        let s = &self.sessions[si];
        let w = &s.windows[s.active];
        let cur = self
            .panes
            .get(&w.active)
            .map(|p| p.rect)
            .unwrap_or_default();
        let (h, wd) = if dir == Dir::H {
            (cur.h, cur.w / 2)
        } else {
            (cur.h / 2, cur.w)
        };
        let cwd = s.cwd.clone();
        let id = self.spawn_pane(shared, h, wd, cwd.as_deref())?;
        let s = &mut self.sessions[si];
        let w = &mut s.windows[s.active];
        w.layout.split(w.active, id, dir);
        w.active = id;
        w.zoomed = false;
        self.relayout(si);
        Ok(())
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
                }
            }
        }
        self.mark_dirty();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        let _ = self.notify.send(());
    }

    fn locate(&self, pid: usize) -> Option<(usize, usize)> {
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
        let ids: Vec<usize> = self
            .clients
            .iter()
            .filter(|(_, c)| c.session == session)
            .map(|(i, _)| *i)
            .collect();
        for id in ids {
            if let Some(mut c) = self.clients.remove(&id) {
                let _ = write_msg(&mut c.sender, &ServerMsg::Exit(reason.into()));
            }
        }
    }

    fn kill_session(&mut self, si: usize) {
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

    fn write_active(&mut self, si: usize, bytes: &[u8]) {
        let s = &self.sessions[si];
        let pid = s.windows[s.active].active;
        if let Some(p) = self.panes.get_mut(&pid) {
            if p.copy {
                p.copy = false;
                p.parser.screen_mut().set_scrollback(0);
                self.dirty = true;
            }
            let _ = p.writer.write_all(bytes);
            let _ = p.writer.flush();
        }
    }

    fn handle_event(&mut self, shared: &Shared, cid: usize, ev: Event) {
        let Some(c) = self.clients.get(&cid) else {
            return;
        };
        let Some(si) = self.sidx(&c.session) else {
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
                let s = &self.sessions[si];
                let pid = s.windows[s.active].active;
                let bp = self
                    .panes
                    .get(&pid)
                    .map(|p| p.parser.screen().bracketed_paste())
                    .unwrap_or(false);
                let data = if bp {
                    format!("\x1b[200~{text}\x1b[201~")
                } else {
                    text
                };
                self.write_active(si, data.as_bytes());
            }
            Event::Mouse(m) if self.cfg.mouse => {
                let s = &self.sessions[si];
                let w = &s.windows[s.active];
                let hit = self
                    .visible_rects(s, w)
                    .into_iter()
                    .find(|(_, r)| r.contains(m.column, m.row))
                    .map(|(i, _)| i);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(pid) = hit {
                            let s = &mut self.sessions[si];
                            s.windows[s.active].active = pid;
                            self.mark_dirty();
                        } else if m.row == s.rows - 1 {
                            // click on status bar: select window by label position
                            if let Some(wi) = crate::render::status_hit(s, m.column) {
                                self.sessions[si].active = wi;
                                self.relayout(si);
                            }
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let up = m.kind == MouseEventKind::ScrollUp;
                        if let Some(pid) = hit {
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
                                p.copy = p.parser.screen().scrollback() > 0;
                                self.mark_dirty();
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, shared: &Shared, cid: usize, si: usize, k: KeyEvent) {
        let prefix = self.clients[&cid].prefix;
        if prefix {
            self.clients.get_mut(&cid).unwrap().prefix = false;
            self.mark_dirty();
            if self.cfg.is_prefix(&k) {
                let b = keys::encode(&k, false);
                self.write_active(si, &b);
                return;
            }
            self.binding(shared, cid, si, k);
            return;
        }
        if self.cfg.is_prefix(&k) {
            self.clients.get_mut(&cid).unwrap().prefix = true;
            self.mark_dirty();
            return;
        }
        let s = &self.sessions[si];
        let pid = s.windows[s.active].active;
        let Some(p) = self.panes.get_mut(&pid) else {
            return;
        };
        if p.copy {
            let h = p.rect.h as usize;
            let cur = p.parser.screen().scrollback();
            let new = match k.code {
                KeyCode::Up | KeyCode::Char('k') => Some(cur + 1),
                KeyCode::Down | KeyCode::Char('j') => Some(cur.saturating_sub(1)),
                KeyCode::PageUp | KeyCode::Char('u') => Some(cur + h),
                KeyCode::PageDown | KeyCode::Char('d') => Some(cur.saturating_sub(h)),
                KeyCode::Char('g') => Some(usize::MAX),
                KeyCode::Char('G') => Some(0),
                KeyCode::Char('q') | KeyCode::Esc => {
                    p.copy = false;
                    Some(0)
                }
                _ => None,
            };
            if let Some(n) = new {
                p.parser.screen_mut().set_scrollback(n);
            }
            self.mark_dirty();
            return;
        }
        let app = p.parser.screen().application_cursor();
        let b = keys::encode(&k, app);
        self.write_active(si, &b);
    }

    fn binding(&mut self, shared: &Shared, cid: usize, si: usize, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let mv = |code: KeyCode| match code {
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H') => Some(Move::Left),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('L') => Some(Move::Right),
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => Some(Move::Up),
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => Some(Move::Down),
            _ => None,
        };
        let is_resize = ctrl || alt || matches!(k.code, KeyCode::Char('H' | 'J' | 'K' | 'L'));
        if let Some(m) = mv(k.code) {
            let s = &mut self.sessions[si];
            let nw = s.windows.len();
            let _ = nw;
            if is_resize {
                let w = &mut s.windows[s.active];
                let (dir, delta) = match m {
                    Move::Left => (Dir::H, -0.05),
                    Move::Right => (Dir::H, 0.05),
                    Move::Up => (Dir::V, -0.05),
                    Move::Down => (Dir::V, 0.05),
                };
                // grow towards direction: right/down grows panes on the left/top
                let target = w.active;
                w.layout.resize(target, dir, delta);
                self.relayout(si);
            } else {
                let s = &self.sessions[si];
                let w = &s.windows[s.active];
                let rects = self.visible_rects(s, w);
                if let Some(n) = neighbor(&rects, w.active, m) {
                    let s = &mut self.sessions[si];
                    s.windows[s.active].active = n;
                    self.mark_dirty();
                }
            }
            return;
        }
        match k.code {
            KeyCode::Char('c') => {
                let _ = self.new_window(shared, si);
            }
            KeyCode::Char('%') | KeyCode::Char('|') => {
                let _ = self.split(shared, si, Dir::H);
            }
            KeyCode::Char('"') | KeyCode::Char('-') => {
                let _ = self.split(shared, si, Dir::V);
            }
            KeyCode::Char('n') => {
                let s = &mut self.sessions[si];
                s.active = (s.active + 1) % s.windows.len();
                self.relayout(si);
            }
            KeyCode::Char('p') => {
                let s = &mut self.sessions[si];
                s.active = (s.active + s.windows.len() - 1) % s.windows.len();
                self.relayout(si);
            }
            KeyCode::Char(d @ '0'..='9') => {
                let i = d as usize - '0' as usize;
                if i < self.sessions[si].windows.len() {
                    self.sessions[si].active = i;
                    self.relayout(si);
                }
            }
            KeyCode::Char('o') => {
                let s = &mut self.sessions[si];
                let w = &mut s.windows[s.active];
                let panes = w.panes();
                let i = panes.iter().position(|p| *p == w.active).unwrap_or(0);
                w.active = panes[(i + 1) % panes.len()];
                self.mark_dirty();
            }
            KeyCode::Char('z') => {
                let s = &mut self.sessions[si];
                let w = &mut s.windows[s.active];
                w.zoomed = !w.zoomed && w.panes().len() > 1;
                self.relayout(si);
            }
            KeyCode::Char('x') => {
                let s = &self.sessions[si];
                let pid = s.windows[s.active].active;
                self.pane_exited(pid);
            }
            KeyCode::Char('&') => {
                let s = &self.sessions[si];
                for pid in s.windows[s.active].panes() {
                    self.pane_exited(pid);
                }
            }
            KeyCode::Char('[') | KeyCode::PageUp => {
                let s = &self.sessions[si];
                let pid = s.windows[s.active].active;
                if let Some(p) = self.panes.get_mut(&pid) {
                    p.copy = true;
                    if k.code == KeyCode::PageUp {
                        let h = p.rect.h as usize;
                        p.parser.screen_mut().set_scrollback(h);
                    }
                }
                self.mark_dirty();
            }
            KeyCode::Char('a') | KeyCode::Char('b') if !ctrl => {
                let s = &mut self.sessions[si];
                s.sidebar = !s.sidebar;
                self.relayout(si);
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.frame.invalidate();
                }
            }
            KeyCode::Char('s') | KeyCode::Char('(') | KeyCode::Char(')') => {
                // cycle sessions
                if self.sessions.len() > 1 {
                    let n = self.sessions.len();
                    let ni = if k.code == KeyCode::Char('(') {
                        (si + n - 1) % n
                    } else {
                        (si + 1) % n
                    };
                    let (cols, rows) = (self.sessions[si].cols, self.sessions[si].rows);
                    let s = &mut self.sessions[ni];
                    s.cols = cols;
                    s.rows = rows;
                    let name = s.name.clone();
                    if let Some(c) = self.clients.get_mut(&cid) {
                        c.session = name;
                        c.frame.invalidate();
                    }
                    self.relayout(ni);
                }
            }
            KeyCode::Char('d') => {
                if let Some(mut c) = self.clients.remove(&cid) {
                    let _ = write_msg(
                        &mut c.sender,
                        &ServerMsg::Exit(format!("detached (from session {})", c.session)),
                    );
                }
            }
            _ => {}
        }
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
            let mut st = shared.lock().unwrap();
            while !st.sessions.is_empty() {
                st.kill_session(0);
            }
            let _ = write_msg(&mut w, &ServerMsg::Text(String::new()));
            std::process::exit(0);
        }
        Command::Attach {
            session,
            create,
            cols,
            rows,
            cwd,
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
    let listener = ipc::listen()?;
    let (tx, rx) = mpsc::channel();
    let shared: Shared = Arc::new(Mutex::new(State {
        cfg: Config::load(),
        panes: HashMap::new(),
        sessions: vec![],
        clients: HashMap::new(),
        next_id: 0,
        dirty: false,
        notify: tx,
    }));

    // Render / housekeeping thread.
    let sh = shared.clone();
    std::thread::spawn(move || {
        let mut ever_had_session = false;
        let mut last_tick = Instant::now();
        loop {
            let _ = rx.recv_timeout(Duration::from_millis(500));
            std::thread::sleep(Duration::from_millis(8));
            while rx.try_recv().is_ok() {}
            let mut st = sh.lock().unwrap();
            // detect exited children (ConPTY may not deliver EOF)
            let dead: Vec<usize> = st
                .panes
                .iter_mut()
                .filter_map(|(id, p)| matches!(p.child.try_wait(), Ok(Some(_))).then_some(*id))
                .collect();
            for id in dead {
                st.pane_exited(id);
            }
            if !st.sessions.is_empty() {
                ever_had_session = true;
            } else if ever_had_session {
                std::process::exit(0);
            }
            let tick = last_tick.elapsed() > Duration::from_millis(500);
            if tick {
                last_tick = Instant::now();
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
            // merge back (new clients may have been added meanwhile? no: we hold the lock)
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
