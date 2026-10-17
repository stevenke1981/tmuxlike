//! Pure text helpers for the status row: width-safe cuts, which-key line, prompt view.
use super::{CONFIRM_SUFFIX, PREFIX_CHIP, WHICH_KEY_END, WHICH_KEY_GROUPS};
use crate::commands::tokenize;
use crate::render::cut;
use std::collections::BTreeMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Keys that run command `cmd`: the single-char key first, then the shortest.
pub(super) fn key_for<'a>(bindings: &'a BTreeMap<String, String>, cmd: &str) -> Option<&'a str> {
    let want = tokenize(cmd);
    bindings
        .iter()
        .filter(|(_, v)| runs(v.as_str(), &want))
        .map(|(k, _)| k.as_str())
        .min_by_key(|k| {
            (
                k.chars().count() != 1,
                default_rank(k),
                k.chars().count(),
                *k,
            )
        })
}

/// Position of `key` in the default table (tmux-conventional keys come first there, so `s`
/// is shown for choose-tree rather than its newer `g` alias); user-added keys sort last.
pub(super) fn default_rank(key: &str) -> usize {
    crate::commands::DEFAULT_BINDINGS
        .iter()
        .position(|(k, _)| *k == key)
        .unwrap_or(usize::MAX)
}

/// True when the binding line `value` runs the command `want` (also when wrapped in
/// `confirm-before [-p TEXT]`).
pub(super) fn runs(value: &str, want: &[String]) -> bool {
    let mut toks = tokenize(value);
    if toks.first().is_some_and(|t| t == "confirm-before") {
        let mut i = 1;
        while toks.get(i).is_some_and(|t| t == "-p") {
            i += 2;
        }
        let skip = i.min(toks.len());
        toks.drain(..skip);
    }
    toks.len() >= want.len() && toks[..want.len()] == want[..]
}

/// Which-key bar shown while the prefix is pending: `PREFIX` chip, then `key label` pairs in
/// priority order (a pair that does not fit is dropped), then the reserved right end. The
/// result is exactly `width` columns when it fits.
pub(super) fn which_key(bindings: &BTreeMap<String, String>, width: u16) -> String {
    let width = width as usize;
    let room = width.saturating_sub(WHICH_KEY_END.width());
    let mut line = PREFIX_CHIP.to_string();
    let mut pairs = 0;
    for (word, cmds) in WHICH_KEY_GROUPS {
        let keys: Vec<&str> = cmds.iter().filter_map(|c| key_for(bindings, c)).collect();
        if keys.is_empty() {
            continue;
        }
        let label = format!("{} {}", keys.join("/"), word);
        let sep = if pairs == 0 { "" } else { "  " };
        if line.width() + sep.width() + label.width() <= room {
            line.push_str(sep);
            line.push_str(&label);
            pairs += 1;
        }
    }
    let pad = room.saturating_sub(line.width());
    line.push_str(&" ".repeat(pad));
    line.push_str(WHICH_KEY_END);
    cut(&line, width, "")
}

/// Prompt row text for `w` columns and the terminal cursor column. The input scrolls so the
/// cursor stays visible; a left cut is marked with `ell`.
pub(super) fn prompt_view(label: &str, buf: &str, cur: usize, w: u16, ell: &str) -> (String, u16) {
    if w == 0 {
        return (String::new(), 0);
    }
    let text: Vec<char> = label.chars().chain(buf.chars()).collect();
    let cur_idx = label.chars().count() + cur.min(buf.chars().count());
    let room = (w as usize).saturating_sub(1);
    let width_from = |s: usize| -> usize {
        text[s..]
            .iter()
            .map(|c| c.width().unwrap_or(0))
            .sum::<usize>()
    };
    let mut start = 0;
    if width_from(0) > room {
        start = (0..text.len())
            .find(|&s| ell.width() + width_from(s) <= room)
            .unwrap_or(text.len());
    }
    start = start.min(cur_idx);
    let cut_left = start > 0;
    let mut shown = if cut_left {
        ell.to_string()
    } else {
        String::new()
    };
    shown.extend(text[start..].iter());
    let before_cursor: usize = text[start..cur_idx]
        .iter()
        .map(|c| c.width().unwrap_or(0))
        .sum();
    let col = if cut_left { ell.width() } else { 0 } + before_cursor;
    (shown, col.min(room) as u16)
}

/// Confirm row text: the question (cut at the end) followed by the `(y/n)` suffix.
pub(super) fn confirm_text(question: &str, w: u16, ell: &str) -> String {
    let room = w.saturating_sub(CONFIRM_SUFFIX.width() as u16);
    format!("{}{CONFIRM_SUFFIX}", cut(question, usize::from(room), ell))
}
