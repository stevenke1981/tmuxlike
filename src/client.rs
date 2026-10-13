use crate::ipc;
use crate::proto::{ClientMsg, Command, ServerMsg, read_msg, write_msg};
use anyhow::{Result, anyhow};
use crossterm::{event, execute, terminal};
use interprocess::local_socket::{Stream, prelude::*};
use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

enum Action {
    Exit(String),
    Paste(u64, std::io::Result<String>),
    #[cfg(windows)]
    ClipboardError(String),
}

struct TerminalGuard;

#[cfg(any(windows, test))]
fn confirmation_key_event(mut key: event::KeyEvent, down: &mut bool) -> event::KeyEvent {
    // Windows crossterm reports native auto-repeat as Press, not Repeat.
    // Holding y before a paste dialog opens must not grant consent.
    if matches!(key.code, event::KeyCode::Char('y' | 'Y')) {
        match key.kind {
            event::KeyEventKind::Release => *down = false,
            event::KeyEventKind::Press => {
                if *down {
                    key.kind = event::KeyEventKind::Repeat;
                }
                *down = true;
            }
            _ => {}
        }
    }
    key
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stdout(),
            event::DisableBracketedPaste,
            event::DisableMouseCapture,
            crossterm::cursor::Show,
            terminal::LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn spawn_server() -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__server")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // std spawns with bInheritHandles=TRUE, so the server would inherit our own
        // stdin/stdout/stderr if they are inheritable (e.g. pipes from a script),
        // keeping them open forever. Make them non-inheritable first.
        unsafe {
            use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
            use windows_sys::Win32::System::Console::{
                GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            };
            for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let handle = GetStdHandle(h);
                if !handle.is_null() {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
        // DETACHED_PROCESS already disconnects the server from the client's console.
        // CREATE_NEW_PROCESS_GROUP would disable Ctrl+C, inherited by ConPTY shells.
        cmd.creation_flags(0x0000_0008 | 0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    cmd.spawn()?;
    Ok(())
}

pub fn connect(start: bool) -> Result<Stream> {
    match ipc::connect() {
        Ok(s) => Ok(s),
        Err(e) if !start => Err(anyhow!("no server running ({e})")),
        Err(_) => {
            spawn_server()?;
            let t = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(50));
                if let Ok(s) = ipc::connect() {
                    return Ok(s);
                }
                if t.elapsed() > Duration::from_secs(5) {
                    return Err(anyhow!("failed to start server"));
                }
            }
        }
    }
}

/// One-shot command (ls, kill...). Prints the server's reply.
pub fn simple(cmd: Command) -> Result<()> {
    let s = connect(false)?;
    let (mut r, mut w) = s.split();
    write_msg(&mut w, &ClientMsg::Hello(cmd))?;
    loop {
        match read_msg::<ServerMsg>(&mut r) {
            Ok(ServerMsg::Text(t)) => print!("{t}"),
            Ok(ServerMsg::Error(e)) => return Err(anyhow!(e)),
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
}

/// `new -d`: create a session in the background and return.
pub fn new_detached(session: Option<String>) -> Result<()> {
    let s = connect(true)?;
    let (mut r, mut w) = s.split();
    let (cols, rows) = terminal::size().unwrap_or((120, 30));
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    write_msg(
        &mut w,
        &ClientMsg::Hello(Command::Attach {
            session,
            create: true,
            cols,
            rows,
            cwd,
            detached: true,
        }),
    )?;
    match read_msg::<ServerMsg>(&mut r)? {
        ServerMsg::Error(e) => Err(anyhow!(e)),
        ServerMsg::Text(t) => {
            print!("{t}");
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn attach(session: Option<String>, create: bool) -> Result<()> {
    if std::env::var_os("TMUXLIKE").is_some() {
        return Err(anyhow!(
            "sessions should be nested with care, unset $TMUXLIKE to force"
        ));
    }
    let s = connect(create)?;
    let (mut r, mut w) = s.split();
    let (cols, rows) = terminal::size().unwrap_or((80, 24));
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    write_msg(
        &mut w,
        &ClientMsg::Hello(Command::Attach {
            session,
            create,
            cols,
            rows,
            cwd,
            detached: false,
        }),
    )?;

    // Wait for the first message so errors print on a normal terminal.
    let first: ServerMsg = read_msg(&mut r)?;
    if let ServerMsg::Error(e) = first {
        return Err(anyhow!(e));
    }

    let mut out = std::io::stdout();
    terminal::enable_raw_mode()?;
    let guard = TerminalGuard;
    execute!(
        out,
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture,
        event::EnableBracketedPaste,
        terminal::Clear(terminal::ClearType::All)
    )?;

    let (tx, rx) = mpsc::channel::<Action>();
    let first_out = match first {
        ServerMsg::Output(s) => Some(s),
        _ => None,
    };
    std::thread::spawn(move || {
        let mut out = std::io::stdout();
        if let Some(s) = first_out {
            let _ = out.write_all(s.as_bytes());
            let _ = out.flush();
        }
        loop {
            match read_msg::<ServerMsg>(&mut r) {
                Ok(ServerMsg::Output(s)) => {
                    let _ = out.write_all(s.as_bytes());
                    let _ = out.flush();
                }
                Ok(ServerMsg::Exit(reason)) => {
                    let _ = tx.send(Action::Exit(reason));
                    return;
                }
                Ok(ServerMsg::Error(e)) => {
                    let _ = tx.send(Action::Exit(format!("error: {e}")));
                    return;
                }
                Ok(ServerMsg::Text(_)) => {}
                Ok(ServerMsg::ReadClipboard(request_id)) => {
                    let _ = tx.send(Action::Paste(request_id, crate::clipboard::read()));
                }
                Ok(ServerMsg::SetClipboard(text)) => {
                    #[cfg(windows)]
                    if let Err(e) = crate::clipboard::write(&text) {
                        let _ = tx.send(Action::ClipboardError(format!(
                            "system copy failed: {e}; internal buffer retained"
                        )));
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = out.write_all(crate::copy::osc52(&text).as_bytes());
                        let _ = out.flush();
                    }
                }
                Err(_) => {
                    let _ = tx.send(Action::Exit("server exited".into()));
                    return;
                }
            }
        }
    });

    #[cfg(windows)]
    let mut confirmation_down = false;
    let reason = loop {
        if let Ok(action) = rx.try_recv() {
            let msg = match action {
                Action::Exit(reason) => break reason,
                Action::Paste(request_id, Ok(text)) => {
                    ClientMsg::ClipboardPaste { request_id, text }
                }
                Action::Paste(request_id, Err(e)) => ClientMsg::ClipboardPasteError {
                    request_id,
                    message: format!("system paste failed: {e}"),
                },
                #[cfg(windows)]
                Action::ClipboardError(e) => ClientMsg::ClipboardError(e),
            };
            if write_msg(&mut w, &msg).is_err() {
                break "connection lost".into();
            }
        }
        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;
            #[cfg(windows)]
            let ev = match ev {
                event::Event::Key(key) => {
                    event::Event::Key(confirmation_key_event(key, &mut confirmation_down))
                }
                other => other,
            };
            if write_msg(&mut w, &ClientMsg::Event(ev)).is_err() {
                break "connection lost".into();
            }
        }
    };

    drop(guard);
    println!("[{reason}]");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_repeated_y_needs_release_before_confirming_again() {
        use event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let mut down = false;
        let y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(
            confirmation_key_event(y, &mut down).kind,
            KeyEventKind::Press
        );
        assert_eq!(
            confirmation_key_event(y, &mut down).kind,
            KeyEventKind::Repeat
        );
        let mut released = y;
        released.kind = KeyEventKind::Release;
        confirmation_key_event(released, &mut down);
        assert_eq!(
            confirmation_key_event(y, &mut down).kind,
            KeyEventKind::Press
        );
        let interrupt = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(confirmation_key_event(interrupt, &mut down), interrupt);
    }
}
