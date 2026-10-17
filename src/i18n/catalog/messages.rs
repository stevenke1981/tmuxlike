//! messages strings: (key, en, zh-TW). Keys start with "messages.".
//!
//! Two kinds of entry: texts the TUI builds itself (`trf` with arguments at the call site), and
//! `messages.err.*` / `messages.sourced` / `messages.exited` templates that map English text
//! built by shared code (command parser, server state, config) to the client's language. The
//! keys of the second kind are listed in `commands::LOCALIZED_ERRORS`. Key names, command names
//! and option names stay English in every language.
use super::super::Entry;

pub const ENTRIES: &[Entry] = &[
    ("messages.cancelled", "cancelled", "已取消"),
    (
        "messages.target_changed",
        "target changed - not killed",
        "目標已變更，未關閉",
    ),
    (
        "messages.unbound",
        "{prefix} {key} is not bound ({prefix} ? lists keys)",
        "{prefix} {key} 未綁定（{prefix} ? 列出按鍵）",
    ),
    (
        "messages.pane_close_hint",
        "panes close with C-b x (kill-pane)",
        "窗格請用 C-b x 關閉（kill-pane）",
    ),
    (
        "messages.no_attention",
        "no pane needs attention",
        "沒有需要關注的窗格",
    ),
    (
        "messages.copied",
        "copied {n} characters",
        "已複製 {n} 個字元",
    ),
    (
        "messages.sourced",
        "sourced {path}",
        "已重新載入設定 {path}",
    ),
    (
        "messages.unknown_command",
        "unknown command: {name}",
        "未知指令：{name}",
    ),
    (
        "messages.blocked_toast",
        "{who} blocked in {session} {wi}.{pi}{note} ({prefix} Tab)",
        "{who} 在 {session} {wi}.{pi} 待回應{note}（{prefix} Tab）",
    ),
    (
        "messages.attach_hint",
        "{prefix} ? keys {dot} {prefix} a agents {dot} {prefix} Tab next alert",
        "{prefix} ? 按鍵 {dot} {prefix} a 代理 {dot} {prefix} Tab 下一個提醒",
    ),
    (
        "messages.session_ended",
        "session {session} ended {dash} now in {other}",
        "工作階段 {session} 已結束 {dash} 已切換到 {other}",
    ),
    (
        "messages.detached",
        "detached (from session {session})",
        "已離開（工作階段 {session}）",
    ),
    ("messages.exited", "exited", "已結束"),
    (
        "messages.session_killed",
        "session killed",
        "工作階段已關閉",
    ),
    (
        "messages.key_header",
        "prefix = {prefix}   (j/k/Space scroll, q closes)",
        "前綴鍵 = {prefix}   （j/k/Space 捲動，q 關閉）",
    ),
    ("messages.status.working", "working", "執行中"),
    ("messages.status.blocked", "blocked", "待回應"),
    ("messages.status.done", "done", "已完成"),
    ("messages.status.idle", "idle", "閒置"),
    ("messages.status.unknown", "unknown", "未知"),
    (
        "messages.kill_pane",
        "kill pane {pane} ({who}, {status})?",
        "關閉窗格 {pane}（{who}，{status}）？",
    ),
    (
        "messages.kill_window",
        "kill window {name} ({count}{blocked})?",
        "關閉視窗 {name}（{count}{blocked}）？",
    ),
    (
        "messages.kill_session",
        "kill session {name} ({count}{blocked})?",
        "關閉工作階段 {name}（{count}{blocked}）？",
    ),
    ("messages.count.pane.one", "{n} pane", "{n} 個窗格"),
    ("messages.count.pane.other", "{n} panes", "{n} 個窗格"),
    ("messages.count.window.one", "{n} window", "{n} 個視窗"),
    ("messages.count.window.other", "{n} windows", "{n} 個視窗"),
    ("messages.blocked_note", ", {n} blocked", "，{n} 個待回應"),
    // --- shared-code errors and notices (templates for commands::localize) ---
    ("messages.err.empty_command", "empty command", "沒有指令"),
    (
        "messages.err.usage_set",
        "usage: set OPTION VALUE",
        "用法：set OPTION VALUE",
    ),
    (
        "messages.err.usage_confirm",
        "usage: confirm-before [-p TEXT] CMD",
        "用法：confirm-before [-p TEXT] CMD",
    ),
    (
        "messages.err.confirm_p",
        "confirm-before: -p needs TEXT",
        "confirm-before：-p 需要 TEXT",
    ),
    (
        "messages.err.usage_bind",
        "usage: bind-key KEY COMMAND",
        "用法：bind-key KEY COMMAND",
    ),
    (
        "messages.err.usage_unbind",
        "usage: unbind-key KEY",
        "用法：unbind-key KEY",
    ),
    (
        "messages.err.resize_need",
        "resize-pane: need -L/-R/-U/-D or -Z",
        "resize-pane：需要 -L/-R/-U/-D 或 -Z",
    ),
    (
        "messages.err.select_window_need",
        "select-window: need -t N",
        "select-window：需要 -t N",
    ),
    (
        "messages.err.bad_window_index",
        "bad window index: {t}",
        "視窗編號錯誤：{t}",
    ),
    (
        "messages.err.unknown_key",
        "unknown key: {spec}",
        "未知按鍵：{spec}",
    ),
    ("messages.err.empty_key", "empty key", "沒有按鍵"),
    (
        "messages.err.kill_window_need",
        "kill-window: needs -t SESSION:IDX",
        "kill-window：需要 -t SESSION:IDX",
    ),
    (
        "messages.err.cant_find_session",
        "can't find session: {name}",
        "找不到工作階段：{name}",
    ),
    (
        "messages.err.cant_find_window",
        "can't find window: {t}",
        "找不到視窗：{t}",
    ),
    ("messages.err.no_sessions", "no sessions", "沒有工作階段"),
    ("messages.err.no_pane", "no pane", "沒有窗格"),
    ("messages.err.no_buffer", "no buffer", "沒有緩衝內容"),
    (
        "messages.err.no_such_window",
        "no such window",
        "沒有這個視窗",
    ),
    (
        "messages.err.invalid_session_name",
        "invalid session name",
        "工作階段名稱無效",
    ),
    (
        "messages.err.duplicate_session",
        "duplicate session: {n}",
        "工作階段重複：{n}",
    ),
    (
        "messages.err.no_config_path",
        "no config path",
        "沒有設定檔路徑",
    ),
    (
        "messages.err.expected_number",
        "expected number, got {v}",
        "需要數字，收到 {v}",
    ),
    (
        "messages.err.expected_onoff",
        "expected on/off, got {v}",
        "需要 on/off，收到 {v}",
    ),
    (
        "messages.err.unknown_option",
        "unknown option: {other}",
        "未知選項：{other}",
    ),
    (
        "messages.err.lang_value",
        "lang: expected auto|en|zh-TW, got {v}",
        "lang：應為 auto、en 或 zh-TW，收到 {v}",
    ),
    (
        "messages.err.pane_titles_value",
        "pane-titles: expected off|auto|always, got {val}",
        "pane-titles：應為 off、auto 或 always，收到 {val}",
    ),
    (
        "messages.err.theme_value",
        "theme: expected terminal|dark|light|mono, got {val}",
        "theme：應為 terminal、dark、light 或 mono，收到 {val}",
    ),
    (
        "messages.err.ascii_glyphs_value",
        "ascii-glyphs: expected auto|on|off, got {val}",
        "ascii-glyphs：應為 auto、on 或 off，收到 {val}",
    ),
    (
        "messages.err.bind_key",
        "bind {key}: {err}",
        "bind {key}：{err}",
    ),
];
