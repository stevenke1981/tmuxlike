/// Append a line to $TMUXLIKE_LOG (if set). Used to diagnose the detached server.
#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        if let Some(p) = std::env::var_os("TMUXLIKE_LOG") {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "[{:?}] {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0), format!($($t)*));
            }
        }
    }};
}

mod api;
mod client;
mod commands;
mod config;
mod copy;
mod input;
mod ipc;
mod keys;
mod layout;
mod persist;
mod proto;
mod render;
mod server;
mod theme;

use proto::Command;

const HELP: &str = "tmuxlike - a cross-platform terminal multiplexer

USAGE:
  tmuxlike                       attach to the last session, or create one
  tmuxlike new [-d] [-s NAME]    create a new session and attach (-d: in background)
  tmuxlike attach [-t NAME]      attach to an existing session (alias: a)
  tmuxlike ls                    list sessions
  tmuxlike kill-session -t NAME  kill a session
  tmuxlike kill-server           kill the server and all sessions
  tmuxlike keys                  show default key bindings
  tmuxlike capture-pane -p [-t S] print the active pane of a session
  tmuxlike send-keys [-t S] KEYS  type text / keys (Enter, C-c, ...) into it
  tmuxlike source-file [PATH]    reload config in the running server
  tmuxlike <command> [args]      run any server command, e.g. `tmuxlike set mouse off`

AUTOMATION (herdr-style, JSON output; see `tmuxlike pane --help`):
  tmuxlike pane list|get|split|read|send-text|send-keys|run|wait-output|...
  tmuxlike agent list|get|read|prompt|wait|rename|focus|send-keys
  tmuxlike session list   tmuxlike api snapshot   tmuxlike server [stop]

Config: $TMUXLIKE_CONFIG or <config_dir>/tmuxlike/config.toml";

const KEYS: &str = "Prefix: Ctrl-b (configurable), then:
  c          new window           n / p      next / previous window
  0-9        select window        %  or |    split left/right
  \"  or -     split top/bottom     arrows/hjkl move focus
  Ctrl/Alt+arrows, H J K L  resize pane
  o          next pane            z          zoom pane
  x          kill pane (y/n)      &          kill window (y/n)
  Tab        next pane needing you (blocked first, then done)
  BTab       previous such pane   g          tree chooser (like s / w)
  [ / PgUp   copy mode (hjkl, v/V select, y copy, q exit)
  ]          paste buffer         :          command prompt
  ,          rename window        $          rename session
  s / w      session chooser      ( )        prev/next session
  a  or b    toggle sidebar       r          reload config
  ?          list keys            d          detach
  Ctrl-b     send prefix to the program
Options (`set NAME VALUE` or config.toml):
  hints on|off            status-bar key hints and the first-attach tip (default on)
  confirm-close on|off    y/n before x and & (default on; off = kill at once)
  pane-titles auto|on|off title row per pane (default auto)
Mouse: click to focus pane / select window in status bar, wheel to scroll, drag to copy.\nCustom bindings: [bind] in config.toml, or `bind-key KEY COMMAND` at the : prompt.";

fn flag(args: &[String], f: &str) -> Option<String> {
    args.iter()
        .position(|a| a == f)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    let res = match sub {
        "__server" => server::run(),
        "server" if args.len() == 1 => server::run(),
        g if api::cli::API_COMMANDS.contains(&g) => {
            let current = std::env::var("TMUXLIKE_PANE_ID")
                .ok()
                .and_then(|v| api::parse_pane_id(&v));
            match api::cli::parse(&args, current) {
                Ok(req) => match client::api(req) {
                    Ok(code) => std::process::exit(code),
                    Err(e) => Err(e),
                },
                Err(help) if help == api::cli::HELP => {
                    println!("{help}");
                    Ok(())
                }
                Err(usage) => {
                    eprintln!("tmuxlike: {usage}");
                    std::process::exit(2);
                }
            }
        }
        "" => client::attach(None, false).or_else(|_| client::attach(None, true)),
        "new" | "new-session" if args.iter().any(|a| a == "-d") => {
            client::new_detached(flag(&args, "-s"))
        }
        "new" | "new-session" => client::attach(flag(&args, "-s"), true),
        "attach" | "a" | "attach-session" => client::attach(flag(&args, "-t"), false),
        "ls" | "list-sessions" => client::simple(Command::List),
        "kill-session" => match flag(&args, "-t") {
            Some(s) => client::simple(Command::Kill { session: s }),
            None => Err(anyhow::anyhow!("usage: tmuxlike kill-session -t NAME")),
        },
        "kill-server" => client::simple(Command::KillServer),
        "keys" | "list-keys" => {
            println!("{KEYS}");
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("tmuxlike {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "-h" | "--help" | "help" => {
            println!("{HELP}");
            Ok(())
        }
        _ => {
            // any other tmux-style command is sent to the running server (e.g. source-file, set, bind-key)
            let line = args
                .iter()
                .map(|a| {
                    if a.contains(' ') {
                        format!("\"{a}\"")
                    } else {
                        a.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            match commands::parse(&line) {
                Ok(_) => client::simple(Command::Run { line }),
                Err(e) => Err(anyhow::anyhow!("{e}\n\n{HELP}")),
            }
        }
    };
    if let Err(e) = res {
        eprintln!("tmuxlike: {e}");
        std::process::exit(1);
    }
}
