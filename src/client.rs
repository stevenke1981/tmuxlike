use crate::ipc;
use crate::proto::{ClientMsg, Command, ServerMsg, read_msg, write_msg};
use anyhow::{Result, anyhow};
use crossterm::{event, execute, terminal};
use interprocess::local_socket::{Stream, prelude::*};
use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
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
    execute!(
        out,
        terminal::EnterAlternateScreen,
        event::EnableMouseCapture,
        event::EnableBracketedPaste,
        terminal::Clear(terminal::ClearType::All)
    )?;

    let (tx, rx) = mpsc::channel::<String>();
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
                    let _ = tx.send(reason);
                    return;
                }
                Ok(ServerMsg::Error(e)) => {
                    let _ = tx.send(format!("error: {e}"));
                    return;
                }
                Ok(ServerMsg::Text(_) | ServerMsg::Api(_)) => {}
                Err(_) => {
                    let _ = tx.send("server exited".into());
                    return;
                }
            }
        }
    });

    let reason = loop {
        if let Ok(r) = rx.try_recv() {
            break r;
        }
        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;
            if write_msg(&mut w, &ClientMsg::Event(ev)).is_err() {
                break "connection lost".into();
            }
        }
    };

    execute!(
        out,
        event::DisableBracketedPaste,
        event::DisableMouseCapture,
        crossterm::cursor::Show,
        terminal::LeaveAlternateScreen
    )?;
    terminal::disable_raw_mode()?;
    println!("[{reason}]");
    Ok(())
}

/// herdr-style API command: prints `{"result": ...}` JSON (or raw text) on stdout and
/// returns 0; on failure prints `{"error": {code, message}}` on stderr and returns 1.
pub fn api(req: crate::api::Request) -> Result<i32> {
    use crate::api::{ApiError, Reply};
    let s = match connect(false) {
        Ok(s) => s,
        Err(e) => {
            let err = ApiError::new("server_not_running", e.to_string());
            eprintln!("{}", serde_json::json!({ "error": err }));
            return Ok(1);
        }
    };
    let (mut r, mut w) = s.split();
    write_msg(&mut w, &ClientMsg::Hello(Command::Api(req)))?;
    match read_msg::<ServerMsg>(&mut r)? {
        ServerMsg::Api(Ok(Reply::Json(v))) => {
            println!("{}", serde_json::json!({ "result": v }));
            Ok(0)
        }
        ServerMsg::Api(Ok(Reply::Text(t))) => {
            print!("{t}");
            if !t.is_empty() && !t.ends_with('\n') {
                println!();
            }
            Ok(0)
        }
        ServerMsg::Api(Err(e)) => {
            eprintln!("{}", serde_json::json!({ "error": e }));
            Ok(1)
        }
        ServerMsg::Error(e) => Err(anyhow!(e)),
        _ => Err(anyhow!("unexpected reply from server")),
    }
}
