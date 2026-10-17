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
mod i18n;
mod input;
mod ipc;
mod keys;
mod layout;
mod persist;
mod proto;
mod render;
mod server;
mod theme;

use i18n::tr;
use proto::Command;

fn flag(args: &[String], f: &str) -> Option<String> {
    args.iter()
        .position(|a| a == f)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sub = args.first().map(|s| s.as_str()).unwrap_or("");
    let lang = client::lang();
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
                    println!("{}", tr(lang, "cli.api_help"));
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
            println!("{}", tr(lang, "cli.keys"));
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("tmuxlike {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "-h" | "--help" | "help" => {
            println!("{}", tr(lang, "cli.help"));
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
                Err(e) => Err(anyhow::anyhow!("{e}\n\n{}", tr(lang, "cli.help"))),
            }
        }
    };
    if let Err(e) = res {
        eprintln!("tmuxlike: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::i18n::{Lang, tr};

    const HELP_KEYS: [&str; 3] = ["cli.help", "cli.keys", "cli.api_help"];

    /// Command, option and key names that the help texts must keep in zh-TW.
    const NAMES: [&str; 14] = [
        "kill-server",
        "kill-session",
        "send-keys",
        "capture-pane",
        "source-file",
        "bind-key",
        "send-text",
        "wait-output",
        "report-agent",
        "release-agent",
        "pane-titles",
        "confirm-close",
        "Ctrl-b",
        "--until",
    ];

    /// Flags such as `-d`, `-t`, `--direction`: tokens that start with `-` and a letter.
    fn flags(text: &str) -> Vec<&str> {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .filter(|w| {
                w.starts_with('-')
                    && w.trim_start_matches('-')
                        .starts_with(|c: char| c.is_ascii_alphabetic())
            })
            .collect()
    }

    /// Usage lines (`tmuxlike new [-d] [-s NAME]`, `pane split [TARGET] ...`): the command
    /// words (`tmuxlike new`, `pane split`) are never translated. The first line is a title;
    /// prose lines (ending in a period) are not usage lines.
    fn usage_heads(text: &str) -> Vec<String> {
        const GROUPS: [&str; 6] = [
            "tmuxlike ",
            "pane ",
            "agent ",
            "api ",
            "session ",
            "server ",
        ];
        text.lines()
            .skip(1)
            .map(str::trim)
            .filter(|l| !l.ends_with('.') && GROUPS.iter().any(|g| l.starts_with(g)))
            .map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
            .collect()
    }

    #[test]
    fn help_and_keys_are_non_empty_and_translated() {
        for key in HELP_KEYS {
            let (en, zh) = (tr(Lang::En, key), tr(Lang::ZhTw, key));
            assert!(!en.is_empty() && !zh.is_empty(), "{key}: empty text");
            assert_ne!(en, zh, "{key}: zh-TW equals English");
        }
    }

    #[test]
    fn english_help_keeps_its_original_first_lines() {
        assert!(
            tr(Lang::En, "cli.help")
                .starts_with("tmuxlike - a cross-platform terminal multiplexer")
        );
        assert!(tr(Lang::En, "cli.keys").starts_with("Prefix: Ctrl-b (configurable), then:"));
        assert!(
            tr(Lang::En, "cli.api_help")
                .starts_with("tmuxlike pane|agent|api|session|server  (herdr-style")
        );
    }

    #[test]
    fn zh_tw_help_uses_glossary_terms() {
        let main_help = tr(Lang::ZhTw, "cli.help");
        assert!(main_help.contains("工作階段") && main_help.contains("窗格"));
        let api_help = tr(Lang::ZhTw, "cli.api_help");
        assert!(api_help.contains("工作階段") && api_help.contains("窗格"));
        assert!(tr(Lang::ZhTw, "cli.keys").contains("窗格"));
    }

    #[test]
    fn command_names_and_flags_survive_translation() {
        for key in HELP_KEYS {
            let (en, zh) = (tr(Lang::En, key), tr(Lang::ZhTw, key));
            for head in usage_heads(en) {
                assert!(
                    zh.contains(head.as_str()),
                    "{key}: zh-TW lost usage '{head}'"
                );
            }
            for flag in flags(en) {
                assert!(zh.contains(flag), "{key}: zh-TW lost flag '{flag}'");
            }
        }
    }

    #[test]
    fn named_commands_and_keys_survive_translation() {
        let all_en: String = HELP_KEYS.iter().map(|k| tr(Lang::En, k)).collect();
        let all_zh: String = HELP_KEYS.iter().map(|k| tr(Lang::ZhTw, k)).collect();
        for name in NAMES {
            assert!(all_en.contains(name), "{name} is not in the English help");
            assert!(all_zh.contains(name), "zh-TW lost {name}");
        }
        assert!(
            tr(Lang::ZhTw, "cli.api_help").contains("pane split [TARGET] --direction right|down")
        );
    }
}
