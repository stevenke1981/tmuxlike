use super::*;
use crate::api::Status;
use crate::i18n::{tr, trf};
use crate::theme::{ClientEnv, Theme, ThemeCfg};
use std::fmt::Display;
use unicode_width::UnicodeWidthStr;

fn ascii_theme() -> Theme {
    let cfg = ThemeCfg {
        ascii_glyphs: "true".into(),
        ..ThemeCfg::default()
    };
    Theme::resolve(&cfg, &ClientEnv::default())
}

fn unicode_theme() -> Theme {
    Theme::resolve(&ThemeCfg::default(), &ClientEnv::default())
}

#[test]
fn tokenize_quotes() {
    assert_eq!(
        tokenize(r#"rename-window "my win"  x"#),
        vec!["rename-window", "my win", "x"]
    );
    assert_eq!(
        tokenize(r#"display 'a b' c\ d # comment"#),
        vec!["display", "a b", "c d"]
    );
    assert_eq!(tokenize(r#"set x """#), vec!["set", "x", ""]);
}

#[test]
fn parse_commands() {
    assert_eq!(
        parse("splitw -h").unwrap(),
        Cmd::SplitWindow { horizontal: true }
    );
    assert_eq!(
        parse("split-window").unwrap(),
        Cmd::SplitWindow { horizontal: false }
    );
    assert_eq!(
        parse("resize-pane -L 10").unwrap(),
        Cmd::ResizePane(Move::Left, 10)
    );
    assert_eq!(parse("resize-pane -Z").unwrap(), Cmd::Zoom);
    assert_eq!(parse("select-pane -U").unwrap(), Cmd::SelectPane(Move::Up));
    assert_eq!(parse("select-pane -t :.+").unwrap(), Cmd::NextPane);
    assert_eq!(parse("select-window -t :3").unwrap(), Cmd::SelectWindow(3));
    assert_eq!(
        parse("rename-window hello world").unwrap(),
        Cmd::RenameWindow(Some("hello world".into()))
    );
    assert_eq!(parse("rename-window").unwrap(), Cmd::RenameWindow(None));
    assert_eq!(
        parse("set -g mouse off").unwrap(),
        Cmd::Set("mouse".into(), "off".into())
    );
    assert_eq!(
        parse("new-window -n logs").unwrap(),
        Cmd::NewWindow {
            name: Some("logs".into())
        }
    );
    assert_eq!(
        parse("bind-key C-k display \"hi there\"").unwrap(),
        Cmd::BindKey("C-k".into(), "display \"hi there\"".into())
    );
    assert_eq!(
        parse("bind -r M-h resize-pane -L 2").unwrap(),
        Cmd::BindKey("M-h".into(), "resize-pane -L 2".into())
    );
    assert!(parse("bind x bogus").is_err());
    assert_eq!(
        parse("unbind PPage").unwrap(),
        Cmd::UnbindKey("PageUp".into())
    );
    assert!(parse("frobnicate").is_err());
    for (k, c) in DEFAULT_BINDINGS {
        assert!(parse(c).is_ok(), "{c}");
        assert_eq!(&normalize_key(k).unwrap(), k);
    }
}

#[test]
fn send_keys() {
    assert_eq!(send_key_bytes("Enter", false), b"\r");
    assert_eq!(send_key_bytes("C-c", false), vec![3]);
    assert_eq!(send_key_bytes("echo hi", false), b"echo hi");
    assert_eq!(send_key_bytes("Enter", true), b"Enter");
    assert_eq!(send_key_bytes("x", false), b"x");
    assert_eq!(
        parse("send-keys -t s1 'echo ok' Enter").unwrap(),
        Cmd::SendKeys {
            target: Some("s1".into()),
            literal: false,
            keys: vec!["echo ok".into(), "Enter".into()]
        }
    );
    assert_eq!(
        parse("capture-pane -p -t x").unwrap(),
        Cmd::CapturePane {
            target: Some("x".into()),
            print: true
        }
    );
}

#[test]
fn attention_prefers_blocked_over_done() {
    // Arrange: pane 1 done comes first in order, pane 3 blocked later
    let items = [(1, Status::Done), (2, Status::Idle), (3, Status::Blocked)];
    // Act
    let got = pick_attention(&items, Some(2), false);
    // Assert
    assert_eq!(got, Some((3, Status::Blocked)));
}

#[test]
fn attention_is_cyclic_from_origin_and_skips_origin() {
    let items = [
        (1, Status::Blocked),
        (2, Status::Blocked),
        (3, Status::Idle),
    ];
    assert_eq!(pick_attention(&items, Some(2), false).unwrap().0, 1);
    assert_eq!(pick_attention(&items, Some(1), false).unwrap().0, 2);
    // backward from 1 wraps to 2 (pane 1 itself is never picked)
    assert_eq!(pick_attention(&items, Some(1), true).unwrap().0, 2);
    // the only candidate is the origin: nothing else needs attention
    assert_eq!(pick_attention(&items[..1], Some(1), false), None);
}

#[test]
fn attention_falls_back_to_done_and_reports_none() {
    let items = [(1, Status::Done), (2, Status::Idle), (3, Status::Working)];
    assert_eq!(
        pick_attention(&items, Some(3), false),
        Some((1, Status::Done))
    );
    let idle = [(1, Status::Idle), (2, Status::Working)];
    assert_eq!(pick_attention(&idle, Some(1), false), None);
    assert_eq!(pick_attention(&[], None, false), None);
}

#[test]
fn attention_without_origin_starts_at_the_ends() {
    let items = [(4, Status::Blocked), (5, Status::Blocked)];
    assert_eq!(pick_attention(&items, None, false).unwrap().0, 4);
    assert_eq!(pick_attention(&items, None, true).unwrap().0, 5);
}

#[test]
fn debug_frame_flags_compose() {
    let Cmd::DebugFrame { target, opts } = parse(
        "debug-frame -t demo --text --prefix --prompt confirm:\"kill pane?\" \
         --chooser=2 --message hi --error --copy --popup",
    )
    .unwrap() else {
        panic!("not debug-frame");
    };
    assert_eq!(target.as_deref(), Some("demo"));
    assert!(opts.text && opts.prefix && opts.error && opts.copy && opts.popup);
    assert_eq!(
        opts.prompt,
        Some((PromptKind::Confirm, "kill pane?".to_string()))
    );
    assert_eq!(opts.chooser, Some(Some(2)));
    assert_eq!(opts.message.as_deref(), Some("hi"));
}

#[test]
fn debug_frame_defaults_and_bad_flags() {
    let Cmd::DebugFrame { opts, .. } = parse("debug-frame").unwrap() else {
        panic!("not debug-frame");
    };
    assert_eq!(opts, FrameOpts::default());
    let Cmd::DebugFrame { opts, .. } = parse("debug-frame --chooser").unwrap() else {
        panic!("not debug-frame");
    };
    assert_eq!(opts.chooser, Some(None));
    assert!(parse("debug-frame --bogus").is_err());
    assert!(parse("debug-frame --prompt nokind").is_err());
    assert!(parse("debug-frame --prompt wat:x").is_err());
}

fn frame_opts_of(line: &str) -> Result<FrameOpts, String> {
    match parse(line)? {
        Cmd::DebugFrame { opts, .. } => Ok(opts),
        other => panic!("not debug-frame: {other:?}"),
    }
}

#[test]
fn debug_frame_lang_flag_accepts_en_and_zh_tw_in_both_forms() {
    assert_eq!(frame_opts_of("debug-frame").unwrap().lang, None);
    assert_eq!(
        frame_opts_of("debug-frame --lang zh-TW").unwrap().lang,
        Some(Lang::ZhTw)
    );
    assert_eq!(
        frame_opts_of("debug-frame --lang=en").unwrap().lang,
        Some(Lang::En)
    );
    // combines with the other flags and with -t
    let Cmd::DebugFrame { target, opts } =
        parse("debug-frame -t S --lang zh-TW --text --prefix").unwrap()
    else {
        panic!("not debug-frame");
    };
    assert_eq!(target.as_deref(), Some("S"));
    assert!(opts.text && opts.prefix);
    assert_eq!(opts.lang, Some(Lang::ZhTw));
}

#[test]
fn debug_frame_lang_flag_rejects_other_languages_and_missing_values() {
    let err = frame_opts_of("debug-frame --lang fr").unwrap_err();
    assert!(err.contains("--lang wants en or zh-TW"), "{err}");
    assert!(frame_opts_of("debug-frame --lang zh_TW").is_err());
    assert!(frame_opts_of("debug-frame --lang").is_err());
}

#[test]
fn screen_text_is_plain_rows_of_exact_width() {
    // Arrange: "ab" on row 1, a wide char at column 3 of row 1 (index 2)
    let ansi = "\x1b[?2026h\x1b[2J\x1b[1;1Hab\x1b[1;3H中\x1b[?2026l";
    // Act
    let text = screen_text(ansi, 6, 3);
    // Assert
    assert!(!text.contains('\x1b'));
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(row.width(), 6, "{row:?}");
    }
    assert!(rows[0].starts_with("ab"), "{:?}", rows[0]);
    assert!(rows[0].contains('中'));
    assert_eq!(rows[2].trim(), "");
}

#[test]
fn confirm_before_parses_prompt_and_command() {
    assert_eq!(
        parse("confirm-before -p \"kill pane?\" kill-pane").unwrap(),
        Cmd::ConfirmBefore {
            prompt: Some("kill pane?".into()),
            command: "kill-pane".into()
        }
    );
    // several words are re-quoted into one command line
    assert_eq!(
        parse("confirm-before kill-window -t demo:1").unwrap(),
        Cmd::ConfirmBefore {
            prompt: None,
            command: "kill-window -t demo:1".into()
        }
    );
    // a single word is taken as a whole command line
    assert_eq!(
        parse("confirm-before \"kill-window -t demo:1\"").unwrap(),
        Cmd::ConfirmBefore {
            prompt: None,
            command: "kill-window -t demo:1".into()
        }
    );
    assert!(parse("confirm-before").is_err());
    assert!(parse("confirm-before bogus").is_err());
    assert!(parse("confirm-before -p").is_err());
}

#[test]
fn kill_targets_and_next_attention_parse() {
    assert_eq!(
        parse("kill-window -t demo:2").unwrap(),
        Cmd::KillWindow {
            target: Some("demo:2".into())
        }
    );
    assert_eq!(
        parse("kill-session -t work").unwrap(),
        Cmd::KillSession {
            target: Some("work".into())
        }
    );
    assert_eq!(
        parse("next-attention").unwrap(),
        Cmd::NextAttention { backward: false }
    );
    assert_eq!(
        parse("next-attention -p").unwrap(),
        Cmd::NextAttention { backward: true }
    );
    assert_eq!(parse("resize-pane -Z").unwrap().name(), "resize-pane");
    assert_eq!(parse("kill-pane").unwrap().name(), "kill-pane");
}

#[test]
fn attention_and_confirm_bindings_are_defaults() {
    let has = |k: &str, c: &str| DEFAULT_BINDINGS.iter().any(|(a, b)| *a == k && *b == c);
    assert!(has("Tab", "next-attention"));
    assert!(has("BTab", "next-attention -p"));
    assert!(has("g", "choose-tree"));
    assert!(has("x", "confirm-before kill-pane"));
    assert!(has("&", "confirm-before kill-window"));
}

#[test]
fn kill_window_pending_names_session_and_index() {
    assert_eq!(kill_window_command("demo", 2), "kill-window -t demo:2");
    // the quoted form must parse back to the same window target
    assert_eq!(
        parse(&kill_window_command("my work", 0)).unwrap(),
        Cmd::KillWindow {
            target: Some("my work:0".into())
        }
    );
}

#[test]
fn kill_session_pending_names_the_session() {
    assert_eq!(kill_session_command("work"), "kill-session -t work");
    assert_eq!(
        kill_session_command("my work"),
        "kill-session -t \"my work\""
    );
    assert_eq!(
        parse(&kill_session_command("my work")).unwrap(),
        Cmd::KillSession {
            target: Some("my work".into())
        }
    );
}

#[test]
fn kill_pane_pending_is_the_bare_command() {
    // the pane pin travels in ConfirmTarget::expect_pane, not in the command text
    assert_eq!(kill_pane_command(), "kill-pane");
    assert_eq!(parse(kill_pane_command()).unwrap(), Cmd::KillPane);
}

#[test]
fn attention_line_uses_theme_glyphs() {
    let place = Some((0, 1));
    let uni = attention_line(
        &unicode_theme(),
        Lang::En,
        Status::Blocked,
        "claude",
        place,
        ": need approval",
    );
    assert_eq!(uni, "▲ claude 0.1 blocked: need approval");
    let done = attention_line(&unicode_theme(), Lang::En, Status::Done, "codex", None, "");
    assert_eq!(done, "◆ codex done");
}

#[test]
fn attention_line_is_ascii_under_ascii_theme() {
    let line = attention_line(
        &ascii_theme(),
        Lang::En,
        Status::Blocked,
        "claude",
        Some((0, 1)),
        ": need approval",
    );
    assert_eq!(line, "! claude 0.1 blocked: need approval");
    assert!(line.is_ascii(), "{line:?}");
    let done = attention_line(&ascii_theme(), Lang::En, Status::Done, "codex", None, "");
    assert!(done.is_ascii(), "{done:?}");
}

#[test]
fn attention_line_is_translated_for_zh_tw() {
    let line = attention_line(
        &unicode_theme(),
        Lang::ZhTw,
        Status::Blocked,
        "claude",
        Some((0, 1)),
        ": need approval",
    );
    assert_eq!(line, "▲ claude 0.1 待回應: need approval");
    let done = attention_line(
        &unicode_theme(),
        Lang::ZhTw,
        Status::Done,
        "codex",
        None,
        "",
    );
    assert_eq!(done, "◆ codex 已完成");
}

#[test]
fn blocked_toast_is_ascii_under_ascii_theme() {
    let toast = blocked_toast(
        &ascii_theme(),
        Lang::En,
        "claude",
        ("work", 1, 2),
        ": need approval",
        "C-b",
    );
    assert_eq!(
        toast,
        "! claude blocked in work 1.2: need approval (C-b Tab)"
    );
    assert!(toast.is_ascii(), "{toast:?}");
    let uni = blocked_toast(
        &unicode_theme(),
        Lang::En,
        "claude",
        ("work", 1, 2),
        "",
        "C-b",
    );
    assert!(uni.starts_with("▲ claude blocked"), "{uni:?}");
}

#[test]
fn blocked_toast_is_translated_for_zh_tw() {
    let toast = blocked_toast(
        &unicode_theme(),
        Lang::ZhTw,
        "claude",
        ("work", 1, 2),
        ": need approval",
        "C-b",
    );
    assert_eq!(
        toast,
        "▲ claude 在 work 1.2 待回應: need approval（C-b Tab）"
    );
}

#[test]
fn attach_hint_separator_follows_theme() {
    let ascii = attach_hint_text(&ascii_theme(), Lang::En, "C-b");
    assert_eq!(ascii, "C-b ? keys . C-b a agents . C-b Tab next alert");
    assert!(ascii.is_ascii(), "{ascii:?}");
    assert!(attach_hint_text(&unicode_theme(), Lang::En, "C-b").contains(" · "));
    let zh = attach_hint_text(&unicode_theme(), Lang::ZhTw, "C-b");
    assert_eq!(zh, "C-b ? 按鍵 · C-b a 代理 · C-b Tab 下一個提醒");
}

#[test]
fn session_ended_notice_dash_follows_theme() {
    let ascii = session_ended_text(&ascii_theme(), Lang::En, "work", "main");
    assert_eq!(ascii, "session work ended - now in main");
    assert!(ascii.is_ascii(), "{ascii:?}");
    assert!(session_ended_text(&unicode_theme(), Lang::En, "work", "main").contains('—'));
    let zh = session_ended_text(&unicode_theme(), Lang::ZhTw, "work", "main");
    assert_eq!(zh, "工作階段 work 已結束 — 已切換到 main");
}

#[test]
fn status_words_keep_api_values_in_english() {
    for s in [
        Status::Idle,
        Status::Working,
        Status::Blocked,
        Status::Done,
        Status::Unknown,
    ] {
        assert_eq!(status_word(Lang::En, s), s.as_str());
        assert_ne!(status_word(Lang::ZhTw, s), s.as_str(), "{s:?}");
    }
    assert_eq!(status_word(Lang::ZhTw, Status::Working), "執行中");
    assert_eq!(status_word(Lang::ZhTw, Status::Blocked), "待回應");
}

#[test]
fn counts_and_blocked_note_follow_language() {
    let n = |lang, key| trf(lang, key, &[("n", &3 as &dyn Display)]);
    assert_eq!(n(Lang::En, "messages.count.pane.other"), "3 panes");
    assert_eq!(n(Lang::ZhTw, "messages.count.pane.other"), "3 個窗格");
    assert_eq!(n(Lang::En, "messages.count.window.one"), "3 window");
    assert_eq!(n(Lang::ZhTw, "messages.count.window.one"), "3 個視窗");
    assert_eq!(
        trf(
            Lang::En,
            "messages.blocked_note",
            &[("n", &2 as &dyn Display)]
        ),
        ", 2 blocked"
    );
    assert_eq!(
        trf(
            Lang::ZhTw,
            "messages.blocked_note",
            &[("n", &2 as &dyn Display)]
        ),
        "，2 個待回應"
    );
}

#[test]
fn key_header_is_translated_and_keeps_key_names() {
    let en = trf(
        Lang::En,
        "messages.key_header",
        &[("prefix", &"C-b" as &dyn Display)],
    );
    assert_eq!(en, "prefix = C-b   (j/k/Space scroll, q closes)");
    let zh = trf(
        Lang::ZhTw,
        "messages.key_header",
        &[("prefix", &"C-b" as &dyn Display)],
    );
    assert_eq!(zh, "前綴鍵 = C-b   （j/k/Space 捲動，q 關閉）");
}

/// (key, arguments, English, zh-TW) of one message.
type MessageCase<'a> = (&'a str, Vec<(&'a str, &'a str)>, &'a str, &'a str);

#[test]
fn tui_messages_format_in_english_and_zh_tw() {
    // the English must stay exactly as before the i18n work
    let cases: Vec<MessageCase> = vec![
        ("messages.cancelled", vec![], "cancelled", "已取消"),
        (
            "messages.target_changed",
            vec![],
            "target changed - not killed",
            "目標已變更，未關閉",
        ),
        (
            "messages.no_attention",
            vec![],
            "no pane needs attention",
            "沒有需要關注的窗格",
        ),
        (
            "messages.pane_close_hint",
            vec![],
            "panes close with C-b x (kill-pane)",
            "窗格請用 C-b x 關閉（kill-pane）",
        ),
        (
            "messages.copied",
            vec![("n", "5")],
            "copied 5 characters",
            "已複製 5 個字元",
        ),
        (
            "messages.sourced",
            vec![("path", "/c/t.conf")],
            "sourced /c/t.conf",
            "已重新載入設定 /c/t.conf",
        ),
        (
            "messages.detached",
            vec![("session", "work")],
            "detached (from session work)",
            "已離開（工作階段 work）",
        ),
        (
            "messages.session_ended",
            vec![("session", "work"), ("dash", "-"), ("other", "main")],
            "session work ended - now in main",
            "工作階段 work 已結束 - 已切換到 main",
        ),
        (
            "messages.key_header",
            vec![("prefix", "C-b")],
            "prefix = C-b   (j/k/Space scroll, q closes)",
            "前綴鍵 = C-b   （j/k/Space 捲動，q 關閉）",
        ),
        (
            "messages.unbound",
            vec![("prefix", "C-b"), ("key", "Q")],
            "C-b Q is not bound (C-b ? lists keys)",
            "C-b Q 未綁定（C-b ? 列出按鍵）",
        ),
        ("messages.exited", vec![], "exited", "已結束"),
        (
            "messages.session_killed",
            vec![],
            "session killed",
            "工作階段已關閉",
        ),
    ];
    for (key, args, en, zh) in cases {
        let args: Vec<(&str, &dyn Display)> =
            args.iter().map(|(n, v)| (*n, v as &dyn Display)).collect();
        assert_eq!(trf(Lang::En, key, &args), en, "{key} en");
        assert_eq!(trf(Lang::ZhTw, key, &args), zh, "{key} zh-TW");
    }
    // the status word is translated per language before it is passed in
    let kill_pane = |lang| {
        let status = status_word(lang, Status::Working);
        trf(
            lang,
            "messages.kill_pane",
            &[
                ("pane", &"p2" as &dyn Display),
                ("who", &"codex"),
                ("status", &status),
            ],
        )
    };
    assert_eq!(kill_pane(Lang::En), "kill pane p2 (codex, working)?");
    assert_eq!(kill_pane(Lang::ZhTw), "關閉窗格 p2（codex，執行中）？");
    // counts and the blocked note are formatted per language, as `confirm_prompt_text` does
    let kill_window = |lang| {
        let count = count(
            lang,
            2,
            "messages.count.pane.one",
            "messages.count.pane.other",
        );
        let blocked = trf(lang, "messages.blocked_note", &[("n", &1 as &dyn Display)]);
        trf(
            lang,
            "messages.kill_window",
            &[
                ("name", &"build" as &dyn Display),
                ("count", &count),
                ("blocked", &blocked),
            ],
        )
    };
    assert_eq!(
        kill_window(Lang::En),
        "kill window build (2 panes, 1 blocked)?"
    );
    assert_eq!(
        kill_window(Lang::ZhTw),
        "關閉視窗 build（2 個窗格，1 個待回應）？"
    );
    let kill_session = |lang| {
        let count = count(
            lang,
            3,
            "messages.count.window.one",
            "messages.count.window.other",
        );
        trf(
            lang,
            "messages.kill_session",
            &[
                ("name", &"work" as &dyn Display),
                ("count", &count),
                ("blocked", &""),
            ],
        )
    };
    assert_eq!(kill_session(Lang::En), "kill session work (3 windows)?");
    assert_eq!(kill_session(Lang::ZhTw), "關閉工作階段 work（3 個視窗）？");
}

#[test]
fn localize_leaves_english_unchanged() {
    for text in [
        "unknown command: frob",
        "can't find session: work",
        "lang: expected auto|en|zh-TW, got fr",
        "copied 3 characters",
        "sourced /x/y.conf",
        "something unknown",
    ] {
        assert_eq!(localize(Lang::En, text), text);
    }
}

#[test]
fn localize_translates_known_errors_and_keeps_values() {
    assert_eq!(
        localize(Lang::ZhTw, "unknown command: frob"),
        "未知指令：frob"
    );
    assert_eq!(
        localize(Lang::ZhTw, "can't find session: my.work"),
        "找不到工作階段：my.work"
    );
    assert_eq!(
        localize(Lang::ZhTw, "lang: expected auto|en|zh-TW, got fr"),
        "lang：應為 auto、en 或 zh-TW，收到 fr"
    );
    assert_eq!(
        localize(Lang::ZhTw, "sourced /x/y.conf"),
        "已重新載入設定 /x/y.conf"
    );
    assert_eq!(localize(Lang::ZhTw, "exited"), "已結束");
    assert_eq!(
        localize(Lang::ZhTw, "usage: set OPTION VALUE"),
        "用法：set OPTION VALUE"
    );
}

#[test]
fn localize_translates_each_part_of_joined_errors_and_nested_values() {
    assert_eq!(
        localize(Lang::ZhTw, "no sessions; unknown option: nope"),
        "沒有工作階段; 未知選項：nope"
    );
    assert_eq!(
        localize(Lang::ZhTw, "bind C-k: unknown key: ab"),
        "bind C-k：未知按鍵：ab"
    );
}

#[test]
fn localize_passes_unmatched_text_through() {
    assert_eq!(
        localize(Lang::ZhTw, "no such thing here"),
        "no such thing here"
    );
}

#[test]
fn every_localized_error_key_has_a_zh_tw_text_with_the_same_placeholders() {
    for key in LOCALIZED_ERRORS {
        let en = tr(Lang::En, key);
        let zh = tr(Lang::ZhTw, key);
        assert_ne!(en, *key, "{key} missing in catalog");
        assert_ne!(zh, *key, "{key} missing zh-TW text");
        assert!(en.contains('{') == zh.contains('{'), "{key}");
    }
}

#[test]
fn shared_code_errors_are_all_matched_by_their_templates() {
    // sample errors as the parser and server build them; each must be translated
    let samples = [
        parse("frobnicate").unwrap_err(),
        parse("set").unwrap_err(),
        parse("confirm-before").unwrap_err(),
        parse("bind-key x").unwrap_err(),
        parse("unbind-key").unwrap_err(),
        parse("resize-pane").unwrap_err(),
        parse("select-window -t x").unwrap_err(),
        parse("bind-key ab display").unwrap_err(),
        parse("").unwrap_err(),
    ];
    for s in samples {
        assert_ne!(localize(Lang::ZhTw, &s), s, "not translated: {s}");
    }
}

#[test]
fn keys() {
    assert_eq!(normalize_key("C-a").unwrap(), "C-a");
    assert_eq!(normalize_key("^B").unwrap(), "C-b");
    assert_eq!(normalize_key("m-left").unwrap(), "M-Left");
    assert_eq!(normalize_key("C-M-x").unwrap(), "C-M-x");
    assert_eq!(normalize_key("npage").unwrap(), "PageDown");
    assert_eq!(normalize_key("-").unwrap(), "-");
    assert_eq!(normalize_key("F5").unwrap(), "F5");
    let ev = |c, m| KeyEvent::new(c, m);
    assert_eq!(
        key_name(&ev(KeyCode::Char('a'), KeyModifiers::CONTROL)).unwrap(),
        "C-a"
    );
    assert_eq!(
        key_name(&ev(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap(),
        "A"
    );
    assert_eq!(
        key_name(&ev(KeyCode::Char('%'), KeyModifiers::SHIFT)).unwrap(),
        "%"
    );
    assert_eq!(
        key_name(&ev(KeyCode::Left, KeyModifiers::ALT)).unwrap(),
        "M-Left"
    );
    assert_eq!(
        key_name(&ev(KeyCode::Char('\u{2}'), KeyModifiers::NONE)).unwrap(),
        "C-b"
    );
}
