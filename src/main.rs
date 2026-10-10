mod client;
mod config;
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
  tmuxlike keys                  show key bindings

Config: $TMUXLIKE_CONFIG or <config_dir>/tmuxlike/config.toml";

const KEYS: &str = "Prefix: Ctrl-b (configurable), then:
  c          new window           n / p      next / previous window
  0-9        select window        %  or |    split left/right
  \"  or -     split top/bottom     arrows/hjkl move focus
  Ctrl/Alt+arrows, H J K L  resize pane
  o          next pane            z          zoom pane
  x          kill pane            &          kill window
  [ / PgUp   scroll mode (Up/Down/PgUp/PgDn/g/G, q to exit)
  a  or b    toggle status sidebar (agents / panes)
  s  or ( )  switch session       d          detach
  Ctrl-b     send prefix to the program
Mouse: click to focus pane / select window in status bar, wheel to scroll.";

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
        _ => {
            println!("{HELP}");
            Ok(())
        }
    };
    if let Err(e) = res {
        eprintln!("tmuxlike: {e}");
        std::process::exit(1);
    }
}
