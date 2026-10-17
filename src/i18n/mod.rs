//! UI translations. English is the source language; every key must exist in every catalog.
//!
//! Catalogs are split by area so they can be edited independently:
//! `catalog/render.rs` (status bar, sidebar, chooser, titles), `catalog/messages.rs`
//! (notices, errors and prompts from input/commands/server) and `catalog/cli.rs`
//! (command-line help and client-side messages). Each entry is `(key, en, zh_tw)`.
//!
//! Placeholders are `{name}` and are filled by [`trf`]. Key names, command names, JSON API
//! codes/messages and status values in JSON stay English on purpose (scripts depend on them).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

mod catalog;

/// UI language.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Lang {
    #[default]
    En,
    /// Traditional Chinese (Taiwan). Also used for other `zh*` locales until they get their own.
    ZhTw,
}

/// One catalog entry: key, English, Traditional Chinese.
pub type Entry = (&'static str, &'static str, &'static str);

impl Lang {
    /// Config / CLI spelling: `en`, `zh-TW`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::ZhTw => "zh-TW",
        }
    }

    /// Parse a config value or locale name (`zh-TW`, `zh_TW.UTF-8`, `zh-Hant`, `en_US`, `C`).
    /// Unknown languages give `None`.
    pub fn from_locale(s: &str) -> Option<Lang> {
        let l = s.trim().to_ascii_lowercase().replace('_', "-");
        let base = l.split(['.', '@']).next().unwrap_or("");
        if base.starts_with("zh") {
            Some(Lang::ZhTw)
        } else if base.starts_with("en") || base == "c" || base == "posix" {
            Some(Lang::En)
        } else {
            None
        }
    }

    /// Language of the current process (client side): LC_ALL, LC_MESSAGES, LANG, LANGUAGE,
    /// then on Windows the user's locale name. `None` when nothing is recognised.
    pub fn detect() -> Option<Lang> {
        for k in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
            if let Some(v) = std::env::var(k).ok().filter(|v| !v.is_empty()) {
                let first = v.split(':').next().unwrap_or(&v).to_string();
                if let Some(l) = Lang::from_locale(&first) {
                    return Some(l);
                }
            }
        }
        windows_locale().and_then(|n| Lang::from_locale(&n))
    }

    /// Effective language: config `lang` (`auto` | `en` | `zh-TW`), then the client's detected
    /// language for `auto`, else English.
    pub fn resolve(cfg_lang: &str, client: Option<Lang>) -> Lang {
        match cfg_lang {
            "auto" | "" => client.unwrap_or_default(),
            other => Lang::from_locale(other).unwrap_or_default(),
        }
    }
}

#[cfg(windows)]
fn windows_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    // SAFETY: the buffer length is passed and the API writes at most that many UTF-16 units.
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..(n as usize - 1)]))
}

#[cfg(not(windows))]
fn windows_locale() -> Option<String> {
    None
}

fn table() -> &'static HashMap<&'static str, (&'static str, &'static str)> {
    static TABLE: OnceLock<HashMap<&'static str, (&'static str, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        catalog::all()
            .flat_map(|entries| entries.iter())
            .map(|(k, en, zh)| (*k, (*en, *zh)))
            .collect()
    })
}

/// Translated text for `key`. Missing translations fall back to English; an unknown key
/// returns the key itself (and fails the catalog tests).
pub fn tr(lang: Lang, key: &'static str) -> &'static str {
    match table().get(key) {
        Some((en, zh)) => match lang {
            Lang::En => en,
            Lang::ZhTw if zh.is_empty() => en,
            Lang::ZhTw => zh,
        },
        None => key,
    }
}

/// [`tr`] with `{name}` placeholders replaced by `args`.
pub fn trf(lang: Lang, key: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = tr(lang, key).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn keys_are_unique_and_complete() {
        let mut seen = HashSet::new();
        for entries in catalog::all() {
            for (k, en, zh) in entries.iter() {
                assert!(seen.insert(*k), "duplicate key {k}");
                assert!(!en.is_empty(), "{k}: empty English text");
                assert!(!zh.is_empty(), "{k}: missing zh-TW translation");
                assert_eq!(
                    placeholders(en),
                    placeholders(zh),
                    "{k}: placeholders differ"
                );
            }
        }
    }

    #[test]
    fn locale_parsing_and_resolution() {
        assert_eq!(Lang::from_locale("zh_TW.UTF-8"), Some(Lang::ZhTw));
        assert_eq!(Lang::from_locale("zh-Hant"), Some(Lang::ZhTw));
        assert_eq!(Lang::from_locale("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_locale("C.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_locale("fr_FR"), None);
        assert_eq!(Lang::resolve("auto", Some(Lang::ZhTw)), Lang::ZhTw);
        assert_eq!(Lang::resolve("auto", None), Lang::En);
        assert_eq!(Lang::resolve("en", Some(Lang::ZhTw)), Lang::En);
        assert_eq!(Lang::resolve("zh-TW", None), Lang::ZhTw);
    }

    #[test]
    fn unknown_key_falls_back_to_the_key() {
        assert_eq!(tr(Lang::ZhTw, "no.such.key"), "no.such.key");
    }
}
