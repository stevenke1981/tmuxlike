use super::*;
use crate::api::Status;
use unicode_width::UnicodeWidthStr;

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
