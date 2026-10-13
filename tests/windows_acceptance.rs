//! Real release/debug executable + ConPTY, with a mock clipboard client.
//! Never reads or writes the interactive user's clipboard or runs their shell profile.
#![cfg(windows)]

#[path = "../src/proto.rs"]
mod proto;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use interprocess::local_socket::{GenericNamespaced, Stream, prelude::*};
use proto::{ClientMsg, Command, ServerMsg, read_msg, write_msg};
use std::io::Write;
use std::process::{Command as Process, Output};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

struct App {
    exe: String,
    socket: String,
    config: std::path::PathBuf,
    log: std::path::PathBuf,
}
impl App {
    fn run(&self, args: &[&str]) -> Output {
        Process::new(&self.exe)
            .args(args)
            .env("TMUXLIKE_SOCKET", &self.socket)
            .env("TMUXLIKE_CONFIG", &self.config)
            .env("TMUXLIKE_LOG", &self.log)
            .env_remove("TMUXLIKE")
            .output()
            .unwrap()
    }
    fn command(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    fn capture(&self) -> String {
        self.command(&["capture-pane", "-p", "-t", "acceptance"])
    }
    fn wait(&self, label: &str, predicate: impl Fn(&str) -> bool) -> String {
        let started = Instant::now();
        loop {
            let pane = self.capture();
            if predicate(&pane) {
                return pane;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "waiting for {label}: {pane}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn attach(&self) -> Peer {
        let socket = Stream::connect(
            self.socket
                .clone()
                .to_ns_name::<GenericNamespaced>()
                .unwrap(),
        )
        .unwrap();
        let (mut reader, writer) = socket.split();
        let messages = Arc::new(Mutex::new(Vec::new()));
        let captured = messages.clone();
        std::thread::spawn(move || {
            while let Ok(message) = read_msg::<ServerMsg>(&mut reader) {
                captured.lock().unwrap().push(message);
            }
        });
        let mut peer = Peer {
            writer: Box::new(writer),
            messages,
        };
        write_msg(
            &mut *peer.writer,
            &ClientMsg::Hello(Command::Attach {
                session: Some("acceptance".into()),
                create: false,
                cols: 120,
                rows: 30,
                cwd: None,
                detached: false,
            }),
        )
        .unwrap();
        peer
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
    }
}
struct Peer {
    writer: Box<dyn Write>,
    messages: Arc<Mutex<Vec<ServerMsg>>>,
}
impl Peer {
    fn event(&mut self, event: Event) {
        write_msg(&mut *self.writer, &ClientMsg::Event(event)).unwrap();
    }
    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.event(Event::Key(KeyEvent::new(code, modifiers)));
    }
    fn prefix(&mut self, code: KeyCode) {
        self.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
        self.key(code, KeyModifiers::NONE);
    }
    fn wait_message(&self, predicate: impl Fn(&ServerMsg) -> bool) {
        let start = Instant::now();
        loop {
            if self.messages.lock().unwrap().iter().any(&predicate) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "message timeout: {:?}",
                self.messages.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        self.event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
}

#[test]
fn conpty_copy_paste_resize_interrupt_detach_and_shell_exit() {
    let temp = std::env::temp_dir().join(format!("tmuxlike-acceptance-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let app = App {
        exe: std::env::var("TMUXLIKE_TEST_EXE")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_tmuxlike").into()),
        socket: format!("tmuxlike-acceptance-{}.sock", std::process::id()),
        config: temp.join("config.toml"),
        log: temp.join("server.log"),
    };
    let history = temp
        .join("shell-history.txt")
        .to_string_lossy()
        .replace('\'', "''");
    let shell = format!(
        "powershell.exe -NoLogo -NoProfile -NoExit -Command \"Set-PSReadLineOption -HistorySavePath '{history}' -HistorySaveStyle SaveNothing\""
    );
    std::fs::write(
        &app.config,
        format!(
            "shell = {}\nosc52 = true\n",
            serde_json::to_string(&shell).unwrap()
        ),
    )
    .unwrap();
    app.command(&["new", "-d", "-s", "acceptance"]);
    app.wait("PowerShell prompt", |pane| pane.contains("PS "));
    let mut peer = app.attach();
    peer.wait_message(|m| matches!(m, ServerMsg::Output(s) if s.contains("[acceptance]")));

    // A normal paste inserts text without executing it until an explicit Enter.
    peer.event(Event::Paste("Write-Output ('single-' + 'paste-ok')".into()));
    std::thread::sleep(Duration::from_millis(150));
    assert!(!app.capture().lines().any(|l| l.trim() == "single-paste-ok"));
    peer.key(KeyCode::Enter, KeyModifiers::NONE);
    app.wait("single paste output", |p| {
        p.lines().any(|l| l.trim() == "single-paste-ok")
    });

    let multiline = "Write-Output ('line-' + 'one-ok')\r\nWrite-Output ('line-' + 'two-ok')\r\n";
    peer.event(Event::Paste(multiline.into()));
    peer.wait_message(|m| matches!(m, ServerMsg::Output(s) if s.contains("y confirm")));
    assert!(!app.capture().contains("line-one-ok"));
    peer.key(KeyCode::Enter, KeyModifiers::NONE); // Enter is NOT consent.
    peer.key(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.capture().contains("line-one-ok"));
    peer.event(Event::Paste(multiline.into()));
    std::thread::sleep(Duration::from_millis(100));
    peer.key(KeyCode::Char('y'), KeyModifiers::NONE);
    app.wait("confirmed CRLF paste", |p| {
        p.lines().any(|l| l.trim() == "line-two-ok")
    });

    // System paste requests the attaching client's clipboard; this peer supplies only self-made text.
    peer.prefix(KeyCode::Char('P'));
    peer.wait_message(|m| matches!(m, ServerMsg::ReadClipboard(_)));
    let request_id = peer
        .messages
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|m| {
            if let ServerMsg::ReadClipboard(id) = m {
                Some(*id)
            } else {
                None
            }
        })
        .unwrap();
    write_msg(
        &mut *peer.writer,
        &ClientMsg::ClipboardPaste {
            request_id,
            text: "Write-Output ('mock-system-' + 'paste-ok')".into(),
        },
    )
    .unwrap();
    peer.key(KeyCode::Enter, KeyModifiers::NONE);
    app.wait("mock system paste", |p| {
        p.lines().any(|l| l.trim() == "mock-system-paste-ok")
    });

    // Produce Unicode via code points so the literal text only appears as output, not command echo.
    app.command(&["send-keys", "-t", "acceptance", "Clear-Host; Write-Output ('COPY ' + [char]0x4e2d + [char]0x6587 + [char]0x65e5 + [char]0x672c + [char]0x8a9e + ' ' + [char]::ConvertFromUtf32(0x1f600) + ' e' + [char]0x301 + ' END')", "Enter"]);
    let text = "COPY 中文日本語 😀 e\u{301} END";
    let pane = app.wait("Unicode output", |p| p.lines().any(|l| l.trim() == text));
    std::fs::write(temp.join("unicode-pane.txt"), &pane).unwrap();
    let row = pane.lines().position(|l| l.trim() == text).unwrap() as u16;
    peer.mouse(MouseEventKind::Down(MouseButton::Left), 0, row);
    peer.mouse(
        MouseEventKind::Drag(MouseButton::Left),
        text.width() as u16 - 1,
        row,
    );
    peer.mouse(
        MouseEventKind::Up(MouseButton::Left),
        text.width() as u16 - 1,
        row,
    );
    peer.wait_message(|m| matches!(m, ServerMsg::SetClipboard(s) if s == text));
    // Internal buffer goes through the same paste path, with no OS clipboard access.
    peer.prefix(KeyCode::Char(']'));
    app.wait("internal paste buffer", |p| {
        p.lines().any(|l| l.starts_with("PS ") && l.ends_with(text))
    });
    peer.key(KeyCode::Char('c'), KeyModifiers::CONTROL); // cancel pasted text, do not execute it

    peer.event(Event::Resize(100, 25));
    app.command(&["send-keys", "-t", "acceptance", "Write-Output ('SIZE=' + $Host.UI.RawUI.WindowSize.Width + ',' + $Host.UI.RawUI.WindowSize.Height)", "Enter"]);
    app.wait("ConPTY resize", |p| {
        p.lines().any(|l| l.trim() == "SIZE=100,24")
    });
    app.command(&[
        "send-keys",
        "-t",
        "acceptance",
        "Write-Output ('sleep-' + 'started'); Start-Sleep -Seconds 30",
        "Enter",
    ]);
    app.wait("sleep pipeline started", |p| {
        p.lines().any(|l| l.trim() == "sleep-started")
    });
    let interrupt = Instant::now();
    peer.key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    // PowerShell flushes queued input while unwinding a cancelled pipeline.
    // Wait for prompt recovery before sending the next command on another IPC connection.
    app.wait("Ctrl+C prompt recovery", |p| {
        p.lines().last().is_some_and(|l| {
            let line = l.trim();
            line.starts_with("PS ") && line.ends_with('>')
        })
    });
    app.command(&[
        "send-keys",
        "-t",
        "acceptance",
        "Write-Output ('interrupt-' + 'alive')",
        "Enter",
    ]);
    app.wait("Ctrl+C interrupt", |p| {
        p.lines().any(|l| l.trim() == "interrupt-alive")
    });
    assert!(interrupt.elapsed() < Duration::from_secs(5));
    let frame = app.command(&["debug-frame", "-t", "acceptance"]);
    let mut rendered = vt100::Parser::new(25, 100, 0);
    rendered.process(frame.as_bytes());
    assert!(rendered.screen().contents().contains("interrupt-alive"));
    assert!(rendered.screen().contents().contains(text));
    std::fs::write(temp.join("terminal-frame.ansi"), &frame).unwrap();
    std::fs::write(
        temp.join("rendered-terminal.txt"),
        rendered.screen().contents(),
    )
    .unwrap();

    peer.prefix(KeyCode::Char('d'));
    peer.wait_message(|m| matches!(m, ServerMsg::Exit(s) if s.contains("detached")));
    assert!(app.command(&["ls"]).contains("acceptance"));
    let again = app.attach();
    again.wait_message(|m| matches!(m, ServerMsg::Output(s) if s.contains("interrupt-alive")));
    app.command(&["new", "-d", "-s", "keeper"]);
    app.command(&["send-keys", "-t", "acceptance", "exit", "Enter"]);
    let start = Instant::now();
    while app.command(&["ls"]).contains("acceptance") {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "ConPTY child exit not observed"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::write(temp.join("PASS.txt"), "PASS: real ConPTY Unicode copy/internal+mock system paste, CRLF consent, resize, Ctrl+C, detach/attach, child exit. Native clipboard tested separately in a private window station. GUI visibility/physical keyboard not asserted.\n").unwrap();
    println!("Acceptance evidence: {}", temp.display());
}
