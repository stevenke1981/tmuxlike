use crate::commands::{DEFAULT_BINDINGS, key_name, normalize_key, parse};
use crossterm::event::KeyEvent;
use serde::Deserialize;
use std::collections::BTreeMap;
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
    /// Name windows after the foreground process (unless renamed manually)
    pub automatic_rename: bool,
    /// Copy selections to the system clipboard (native on Windows, OSC 52 elsewhere)
    pub osc52: bool,
    /// Extra key bindings (after the prefix): key -> command
    pub bind: BTreeMap<String, String>,
    /// Keys to remove from the default bindings ("all" removes every default)
    pub unbind: Vec<String>,
    /// Commands run after the config is loaded
    pub commands: Vec<String>,
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
            automatic_rename: true,
            osc52: true,
            bind: BTreeMap::new(),
            unbind: vec![],
            commands: vec![],
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TMUXLIKE_CONFIG") {
        return Some(PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("tmuxlike").join("config.toml"))
}

fn parse_bool(v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(format!("expected on/off, got {v}")),
    }
}

impl Config {
    pub fn load() -> Config {
        match config_path().filter(|p| p.exists()) {
            Some(p) => Config::load_from(&p).unwrap_or_else(|e| {
                eprintln!("tmuxlike: {e}");
                Config::default()
            }),
            None => Config::default(),
        }
    }

    pub fn load_from(p: &std::path::Path) -> Result<Config, String> {
        let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        toml::from_str(&s).map_err(|e| format!("config error: {e}"))
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

    pub fn shell_words(&self) -> Result<Vec<String>, String> {
        let shell = self.default_shell();
        if shell.trim().is_empty() || shell.contains('\0') {
            return Err("shell must name an executable".into());
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::LocalFree;
            use windows_sys::Win32::UI::Shell::CommandLineToArgvW;
            let command: Vec<u16> = shell.trim().encode_utf16().chain(Some(0)).collect();
            let mut count = 0;
            let argv = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
            if argv.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let words = unsafe { std::slice::from_raw_parts(argv, count as usize) }
                .iter()
                .map(|p| {
                    let mut len = 0;
                    unsafe {
                        while *p.add(len) != 0 {
                            len += 1;
                        }
                        String::from_utf16_lossy(std::slice::from_raw_parts(*p, len))
                    }
                })
                .collect();
            unsafe {
                LocalFree(argv.cast());
            }
            Ok(words)
        }
        #[cfg(not(windows))]
        {
            Ok(crate::commands::tokenize(&shell))
        }
    }

    pub fn prefix_name(&self) -> String {
        normalize_key(&self.prefix).unwrap_or_else(|_| "C-b".into())
    }

    pub fn is_prefix(&self, k: &KeyEvent) -> bool {
        key_name(k).is_some_and(|n| n == self.prefix_name())
    }

    /// Build the effective key table: defaults - unbind + bind.
    pub fn bindings(&self) -> (BTreeMap<String, String>, Vec<String>) {
        let mut errs = vec![];
        let mut m: BTreeMap<String, String> = BTreeMap::new();
        if !self.unbind.iter().any(|u| u == "all") {
            for (k, c) in DEFAULT_BINDINGS {
                m.insert(k.to_string(), c.to_string());
            }
        }
        for u in &self.unbind {
            if let Ok(k) = normalize_key(u) {
                m.remove(&k);
            }
        }
        for (k, c) in &self.bind {
            match (normalize_key(k), parse(c)) {
                (Ok(k), Ok(_)) => {
                    m.insert(k, c.clone());
                }
                (Err(e), _) | (_, Err(e)) => errs.push(format!("bind {k}: {e}")),
            }
        }
        (m, errs)
    }

    /// `set OPTION VALUE`
    pub fn set(&mut self, key: &str, val: &str) -> Result<(), String> {
        let num = |v: &str| {
            v.parse::<f64>()
                .map_err(|_| format!("expected number, got {v}"))
        };
        match key.replace('_', "-").as_str() {
            "prefix" => {
                normalize_key(val)?;
                self.prefix = val.to_string()
            }
            "shell" | "default-shell" | "default-command" => self.shell = Some(val.to_string()),
            "mouse" => self.mouse = parse_bool(val)?,
            "sidebar" => self.sidebar = parse_bool(val)?,
            "sidebar-width" => self.sidebar_width = num(val)?.clamp(10.0, 100.0) as u16,
            "scrollback" | "history-limit" => self.scrollback = num(val)? as usize,
            "busy-secs" => self.busy_secs = num(val)? as f32,
            "automatic-rename" => self.automatic_rename = parse_bool(val)?,
            "osc52" | "set-clipboard" => self.osc52 = parse_bool(val)?,
            other => return Err(format!("unknown option: {other}")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn windows_shell_paths_keep_backslashes_and_spaces() {
        let c = Config {
            shell: Some(r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo -NoProfile"#.into()),
            ..Config::default()
        };
        assert_eq!(
            c.shell_words().unwrap(),
            [
                r"C:\Program Files\PowerShell\7\pwsh.exe",
                "-NoLogo",
                "-NoProfile"
            ]
        );
        let c = Config {
            shell: Some(r#""\\host\share\shell.exe" "a b""#.into()),
            ..Config::default()
        };
        assert_eq!(c.shell_words().unwrap(), [r"\\host\share\shell.exe", "a b"]);
    }
    #[test]
    fn parse_config() {
        let c: Config = toml::from_str(
            "prefix = \"C-a\"\nsidebar = true\nunbind = [\"x\"]\n[bind]\n\"C-k\" = \"kill-pane\"\nv = \"split-window -h\"\n",
        )
        .unwrap();
        assert!(c.sidebar);
        assert_eq!(c.prefix_name(), "C-a");
        assert_eq!(c.scrollback, 10000);
        let (b, errs) = c.bindings();
        assert!(errs.is_empty());
        assert!(!b.contains_key("x"));
        assert_eq!(b["C-k"], "kill-pane");
        assert_eq!(b["v"], "split-window -h");
        assert_eq!(b["c"], "new-window");
        let mut c2: Config = toml::from_str("unbind = [\"all\"]\n[bind]\nq = \"nope\"").unwrap();
        let (b2, e2) = c2.bindings();
        assert!(b2.is_empty());
        assert_eq!(e2.len(), 1);
        c2.set("mouse", "off").unwrap();
        assert!(!c2.mouse);
        c2.set("history-limit", "500").unwrap();
        assert_eq!(c2.scrollback, 500);
        assert!(c2.set("prefix", "C-").is_err() || c2.prefix == "C-");
        assert!(c2.set("bogus", "1").is_err());
    }
}
