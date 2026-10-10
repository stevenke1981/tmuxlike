mod client;
mod commands;
mod config;
mod copy;
mod ipc;
mod keys;
mod layout;
mod proto;
mod render;
mod server;

use proto::Command;

const HELP: &str = "tmuxlike - a cross-platform terminal multiplexer

USAGE:
  tmuxlike                       attach to the last session, or create one
  tmuxlike new [-s NAME]         create a new session and attach
  tmuxlike attach [-t NAME]      attach to an existing session (alias: a)
  tmuxlike ls                    list sessions
  tmuxlike kill-session -t NAME  kill a session
  tmuxlike kill-server           kill the server and all sessions
  tmuxlike keys                  show default key bindings
  tmuxlike source-file [PATH]    reload config in the running server
  tmuxlike <command> [args]      run any server command, e.g. `tmuxlike set mouse off`

Config: $TMUXLIKE_CONFIG or <config_dir>/tmuxlike/config.toml";

const KEYS: &str = "Prefix: Ctrl-b (configurable), then:
  c          new window           n / p      next / previous window
  0-9        select window        %  or |    split left/right
  \"  or -     split top/bottom     arrows/hjkl move focus
  Ctrl/Alt+arrows, H J K L  resize pane
  o          next pane            z          zoom pane
  x          kill pane            &          kill window
  [ / PgUp   copy mode (hjkl, v/V select, y copy, q exit)
  ]          paste buffer         :          command prompt
  ,          rename window        $          rename session
  s / w      session chooser      ( )        prev/next session
  a  or b    toggle sidebar       r          reload config
  ?          list keys            d          detach
  Ctrl-b     send prefix to the program
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
        "" => client::attach(None, false).or_else(|_| client::attach(None, true)),
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
