use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    /// Prefix key, e.g. "C-b", "C-a"
    pub prefix: String,
    /// Shell to launch in new panes (default: $SHELL / powershell)
    pub shell: Option<String>,
    pub scrollback: usize,
    pub mouse: bool,
    /// Show herdr-like sidebar with pane/agent status
    pub sidebar: bool,
    pub sidebar_width: u16,
    /// Seconds after last output during which a pane is considered "busy"
    pub busy_secs: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            prefix: "C-b".into(),
            shell: None,
            scrollback: 10000,
            mouse: true,
            sidebar: false,
            sidebar_width: 28,
            busy_secs: 1.5,
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TMUXLIKE_CONFIG") {
        return Some(PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("tmuxlike").join("config.toml"))
}

impl Config {
    pub fn load() -> Config {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| match toml::from_str(&s) {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("tmuxlike: config error: {e}");
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn default_shell(&self) -> String {
        if let Some(s) = &self.shell {
            return s.clone();
        }
        if cfg!(windows) {
            "powershell.exe".into()
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
        }
    }

    /// Parsed prefix: (char, needs ctrl)
    pub fn prefix_key(&self) -> (char, bool) {
        parse_key(&self.prefix).unwrap_or(('b', true))
    }

    pub fn is_prefix(&self, k: &KeyEvent) -> bool {
        let (c, ctrl) = self.prefix_key();
        match k.code {
            KeyCode::Char(kc) => {
                kc.eq_ignore_ascii_case(&c) && k.modifiers.contains(KeyModifiers::CONTROL) == ctrl
            }
            _ => false,
        }
    }
}

pub fn parse_key(s: &str) -> Option<(char, bool)> {
    let s = s.trim();
    if let Some(rest) = s
        .strip_prefix("C-")
        .or_else(|| s.strip_prefix("Ctrl-"))
        .or_else(|| s.strip_prefix("^"))
    {
        let mut ch = rest.chars();
        let c = ch.next()?;
        if ch.next().is_some() {
            return None;
        }
        return Some((c.to_ascii_lowercase(), true));
    }
    let mut ch = s.chars();
    let c = ch.next()?;
    if ch.next().is_some() {
        return None;
    }
    Some((c, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse() {
        assert_eq!(parse_key("C-a"), Some(('a', true)));
        assert_eq!(parse_key("^B"), Some(('b', true)));
        assert_eq!(parse_key("`"), Some(('`', false)));
        let c: Config = toml::from_str("prefix = \"C-a\"\nsidebar = true").unwrap();
        assert!(c.sidebar);
        assert_eq!(c.prefix_key(), ('a', true));
        assert_eq!(c.scrollback, 10000);
    }
}
