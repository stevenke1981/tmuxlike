use super::*;
use crate::commands::DEFAULT_BINDINGS;
use crate::layout::Node;
use crate::server::{Message, Window};
use crate::theme::{ASCII, UNICODE};
use crossterm::style::Color;
use std::collections::BTreeMap;

fn win(name: &str) -> WinItem {
    WinItem {
        name: name.into(),
        glyph: None,
        attention: false,
        zoomed: false,
    }
}

fn input(session: &str, wins: Vec<WinItem>, active: usize, right: Right) -> StatusInput {
    StatusInput {
        session: session.into(),
        wins,
        active,
        ellipsis: "…",
        right,
        lang: Lang::En,
    }
}

fn key_bar(key: &str) -> Right {
    Right::Bar {
        counts: vec![],
        tail: Tail::Help(key.into()),
    }
}

fn defaults() -> BTreeMap<String, String> {
    DEFAULT_BINDINGS
        .iter()
        .map(|(k, c)| (k.to_string(), c.to_string()))
        .collect()
}

/// Paint one row like `draw_status` does; return its visible text and hit ranges.
fn row(inp: &StatusInput, w: u16) -> (String, Vec<(usize, u16, u16)>) {
    let mut g = Grid {
        w,
        h: 1,
        cells: vec![Cell::default(); w as usize],
    };
    let layout = status_layout(inp, w);
    paint(&mut g, 0, &layout.segs, &Theme::terminal());
    let text = g
        .cells
        .iter()
        .filter(|c| !c.skip)
        .map(|c| c.s.as_str())
        .collect();
    (text, layout.hits)
}

fn twelve() -> Vec<WinItem> {
    (0..12).map(|i| win(&format!("w{i}"))).collect()
}

#[test]
fn cjk_session_name_uses_display_width() {
    let inp = input("測試", vec![win("cmd")], 0, key_bar("C-b"));
    let (text, hits) = row(&inp, 40);
    // chip " 測試 " is 6 columns, then a 2-column gap before the label
    assert_eq!(hits, vec![(0, 8, 14)]);
    assert!(text.starts_with(" 測試   0:cmd*"), "{text:?}");
    assert!(text.ends_with("C-b ? help"), "{text:?}");
}

#[test]
fn twelve_windows_at_sixty_columns_keep_the_active_label() {
    let inp = input("demo", twelve(), 5, key_bar("C-b"));
    let (text, hits) = row(&inp, 60);
    let active = hits
        .iter()
        .find(|h| h.0 == 5)
        .expect("active label is clickable");
    assert!(active.2 < 60 - " C-b".width() as u16, "{hits:?}");
    assert!(text.contains("5:w5*"), "{text:?}");
    // windows 0-3 are hidden on the left, so the `<` marker is shown
    assert!(text.contains("<  4:"), "{text:?}");
    // the help text is the first thing cut when the window list needs the room
    assert!(text.ends_with("C-b"), "{text:?}");
}

#[test]
fn overflow_windows_on_the_right_become_plus_n() {
    let inp = input("demo", twelve(), 0, key_bar("C-b"));
    let (text, hits) = row(&inp, 40);
    assert!(hits.iter().any(|h| h.0 == 0), "{hits:?}");
    assert!(text.contains("+"), "{text:?}");
    assert!(text.ends_with("C-b"), "{text:?}");
}

#[test]
fn counts_region_matches_the_drawn_counts_block() {
    let counts = vec![
        Count {
            glyph: "▲",
            status: Status::Blocked,
            n: 1,
        },
        Count {
            glyph: "◆",
            status: Status::Done,
            n: 1,
        },
        Count {
            glyph: "●",
            status: Status::Working,
            n: 1,
        },
    ];
    let right = Right::Bar {
        counts,
        tail: Tail::Help("C-b".into()),
    };
    let inp = input("demo", vec![win("cmd")], 0, right);
    let layout = status_layout(&inp, 80);
    // "▲1 ◆1 ●1" is 8 columns; then a 2-column gap and " C-b ? help" (11 columns)
    assert_eq!(layout.regions.counts, Some((59, 67)));
    assert_eq!(layout.regions.session, Some((0, 6)));
    let counts_text: String = {
        let mut g = Grid {
            w: 80,
            h: 1,
            cells: vec![Cell::default(); 80],
        };
        paint(&mut g, 0, &layout.segs, &Theme::terminal());
        g.cells[59..67].iter().map(|c| c.s.as_str()).collect()
    };
    assert_eq!(counts_text, "▲1 ◆1 ●1");
}

#[test]
fn hit_columns_are_exactly_the_drawn_labels() {
    let inp = input("demo", twelve(), 5, key_bar("C-b"));
    let (text, hits) = row(&inp, 60);
    let cells: Vec<char> = text.chars().collect();
    assert!(!hits.is_empty());
    for (idx, a, b) in &hits {
        let (a, b) = (*a as usize, *b as usize);
        let label: String = cells[a..b].iter().collect();
        assert!(label.starts_with(&format!("{idx}:")), "{label:?}");
        assert_eq!(hit_at(&hits, a as u16), Some(*idx));
        assert_eq!(hit_at(&hits, (b - 1) as u16), Some(*idx));
        assert_ne!(hit_at(&hits, b as u16), Some(*idx));
    }
}

fn session_with(name: &str, windows: &[&str]) -> Session {
    Session {
        name: name.into(),
        windows: windows
            .iter()
            .map(|w| Window {
                name: (*w).into(),
                manual_name: false,
                layout: Node::Leaf(0),
                active: 0,
                zoomed: false,
            })
            .collect(),
        active: 0,
        cols: 40,
        rows: 24,
        sidebar: false,
        cwd: None,
        last_used: std::time::Instant::now(),
    }
}

#[test]
fn status_hit_is_computed_from_the_session_without_global_state() {
    let sess = session_with("hit-s", &["a", "b"]);
    let inp = StatusInput {
        session: "hit-s".into(),
        wins: sess.windows.iter().map(|w| win(&w.name)).collect(),
        active: 0,
        ellipsis: "…",
        right: Right::None,
        lang: Lang::En,
    };
    let hits = status_layout(&inp, 40).hits;
    let (a0, b0) = (hits[0].1, hits[0].2);
    let a1 = hits[1].1;
    assert_eq!(status_hit(&sess, a0), Some(0));
    assert_eq!(status_hit(&sess, b0), None, "gap column is not a window");
    assert_eq!(status_hit(&sess, a1), Some(1));
    // same session name, different windows: the answer follows the input, nothing is cached.
    // The one-window session's label covers column a1, so it answers 0 there.
    let other = session_with("hit-s", &["only"]);
    assert_eq!(status_hit(&other, a1), Some(0));
    assert_eq!(status_hit(&other, a1 + 8), None);
    assert_eq!(status_hit(&sess, a1), Some(1));
}

#[test]
fn status_hit_without_windows_is_none() {
    assert_eq!(status_hit(&session_with("empty", &[]), 0), None);
}

#[test]
fn muted_hint_uses_bar_foreground_and_faint_attribute() {
    let t = Theme::terminal();
    let (fg, bg, attrs) = tone_style(&t, Tone::Muted);
    assert_eq!(fg, t.c(t.bar_fg));
    assert_eq!(bg, t.c(t.bar_bg));
    assert_eq!(attrs & DIM, DIM);
    assert_ne!(fg, Color::AnsiValue(8), "ANSI 8 on ANSI 0 can vanish");
}

#[test]
fn mono_muted_text_is_faint_without_color() {
    let t = Theme {
        mono: true,
        ..Theme::terminal()
    };
    let (fg, bg, attrs) = tone_style(&t, Tone::Muted);
    assert_eq!((fg, bg), (Color::Reset, Color::Reset));
    assert_eq!(attrs & DIM, DIM);
}

fn msg(text: &str, error: bool) -> Message {
    Message {
        text: text.into(),
        at: std::time::Instant::now(),
        error,
    }
}

#[test]
fn message_glyph_info_is_neutral_and_ok_is_for_copied_or_sourced() {
    let g = |text, error| message_glyph(&msg(text, error), &UNICODE);
    assert_eq!(
        g("cancelled", false),
        Glyph {
            text: "•",
            tone: Tone::Bar
        }
    );
    assert_eq!(
        g("copied 3 characters", false),
        Glyph {
            text: "✓",
            tone: Tone::Ok
        }
    );
    assert_eq!(
        g("sourced ~/.tmuxlike.conf", false),
        Glyph {
            text: "✓",
            tone: Tone::Ok
        }
    );
    assert_eq!(
        g("unknown command: x", true),
        Glyph {
            text: "✗",
            tone: Tone::Err
        }
    );
}

#[test]
fn message_glyph_follows_the_ascii_set_and_keeps_errors_on_error_token() {
    let g = |text, error| message_glyph(&msg(text, error), &ASCII);
    assert_eq!(g("cancelled", false).text, "*");
    assert_eq!(g("copied", false).text, "+");
    assert_eq!(
        g("boom", true),
        Glyph {
            text: "x",
            tone: Tone::Err
        }
    );
}

#[test]
fn info_message_is_drawn_in_the_bar_color() {
    let right = Right::Message {
        glyph: Glyph {
            text: "•",
            tone: Tone::Bar,
        },
        text: "cancelled".into(),
    };
    let layout = status_layout(&input("demo", vec![win("cmd")], 0, right), 80);
    let seg = layout
        .segs
        .iter()
        .find(|s| s.text.contains("cancelled"))
        .expect("message is drawn");
    assert_eq!(seg.tone, Tone::Bar);
    assert_eq!(seg.text, " • cancelled");
}

#[test]
fn ascii_bar_cuts_with_the_theme_ellipsis_only() {
    let mut inp = input("demo-with-a-long-session-name", twelve(), 5, key_bar("C-b"));
    inp.ellipsis = "~";
    for w in 8..=90u16 {
        let (text, _) = row(&inp, w);
        assert!(text.is_ascii(), "w={w} {text:?}");
    }
    let right = Right::Message {
        glyph: Glyph {
            text: "x",
            tone: Tone::Err,
        },
        text: "a long error message that has to be cut somewhere".into(),
    };
    let mut err = input("demo", vec![win("cmd")], 0, right);
    err.ellipsis = "~";
    let (text, _) = row(&err, 40);
    assert!(text.is_ascii() && text.contains('~'), "{text:?}");
}

#[test]
fn which_key_bar_is_ascii_when_bindings_are_default() {
    assert!(which_key(&defaults(), 80, Lang::En).is_ascii());
    assert!(which_key(&defaults(), 40, Lang::En).is_ascii());
}

#[test]
fn counts_show_blocked_done_working_and_hide_zero() {
    let counts = vec![
        Count {
            glyph: "▲",
            status: Status::Blocked,
            n: 1,
        },
        Count {
            glyph: "◆",
            status: Status::Done,
            n: 1,
        },
        Count {
            glyph: "●",
            status: Status::Working,
            n: 1,
        },
    ];
    let right = Right::Bar {
        counts,
        tail: Tail::Help("C-b".into()),
    };
    let (text, _) = row(&input("demo", vec![win("cmd")], 0, right), 80);
    assert!(text.ends_with("▲1 ◆1 ●1   C-b ? help"), "{text:?}");
}

#[test]
fn right_block_is_never_overpainted() {
    let counts = vec![Count {
        glyph: "▲",
        status: Status::Blocked,
        n: 2,
    }];
    for w in 8..=100u16 {
        let right = Right::Bar {
            counts: counts.clone(),
            tail: Tail::Help("C-b".into()),
        };
        let (text, _) = row(&input("demo", twelve(), 3, right), w);
        let end = text.trim_end();
        assert!(
            end.ends_with("C-b") || end.ends_with("C-b ? help"),
            "w={w} {text:?}"
        );
    }
}

#[test]
fn hints_off_show_only_the_key() {
    let right = Right::Bar {
        counts: vec![],
        tail: Tail::Key("C-b".into()),
    };
    let (text, _) = row(&input("demo", vec![win("cmd")], 0, right), 60);
    assert!(text.ends_with(" C-b"), "{text:?}");
}

#[test]
fn attention_and_zoom_flags_follow_the_active_window() {
    let mut zoomed = win("cmd");
    zoomed.zoomed = true;
    zoomed.glyph = Some(("▲", Status::Blocked));
    let mut busy = win("logs");
    busy.attention = true;
    let (text, _) = row(&input("demo", vec![zoomed, busy], 0, key_bar("C-b")), 60);
    assert!(text.contains("0:cmd*Z▲"), "{text:?}");
    assert!(text.contains("1:logs!"), "{text:?}");
}

#[test]
fn copy_mode_shows_the_copy_chip_and_hints() {
    let (wide, _) = row(&input("demo", vec![win("cmd")], 0, Right::Copy), 120);
    assert!(
        wide.contains(" COPY ") && wide.trim_end().ends_with("y copy  q quit"),
        "{wide:?}"
    );
    let (narrow, _) = row(&input("demo", vec![win("cmd")], 0, Right::Copy), 40);
    assert!(narrow.contains(" COPY "), "{narrow:?}");
    assert!(narrow.contains("0:cmd"), "{narrow:?}");
}

#[test]
fn message_chip_is_right_aligned_and_cut() {
    let long = Right::Message {
        glyph: Glyph {
            text: "✗",
            tone: Tone::Err,
        },
        text: "unknown command: a-very-long-error-message-that-keeps-going".into(),
    };
    let (text, hits) = row(&input("demo", vec![win("cmd")], 0, long), 60);
    assert!(text.contains("✗ unknown"), "{text:?}");
    assert!(text.trim_end().ends_with('…'), "{text:?}");
    assert!(
        text.contains("0:cmd"),
        "window list stays visible: {text:?}"
    );
    assert_eq!(hits.len(), 1);
}

#[test]
fn copied_message_uses_ok_glyph() {
    let right = Right::Message {
        glyph: Glyph {
            text: "✓",
            tone: Tone::Ok,
        },
        text: "copied 42 chars".into(),
    };
    let (text, _) = row(&input("demo", vec![win("cmd")], 0, right), 80);
    assert!(text.trim_end().ends_with("✓ copied 42 chars"), "{text:?}");
}

#[test]
fn which_key_width_40_keeps_the_reserved_end() {
    let line = which_key(&defaults(), 40, Lang::En);
    assert_eq!(line.width(), 40, "{line:?}");
    assert!(line.ends_with("? all  Esc cancel"), "{line:?}");
    assert!(line.starts_with(" PREFIX c new"), "{line:?}");
}

#[test]
fn which_key_full_line_at_120_columns() {
    let line = which_key(&defaults(), 120, Lang::En);
    let expected = format!(
        " PREFIX c new  n/p win  %/\" split  h/j/k/l move  z zoom  x kill  Tab alert  s tree  [ copy  d detach{} ? all  Esc cancel",
        " ".repeat(2)
    );
    assert_eq!(line, expected);
    assert_eq!(line.width(), 120);
}

#[test]
fn which_key_lists_live_bindings_in_priority_order() {
    let bindings = defaults();
    let line = which_key(&bindings, 200, Lang::En);
    for want in [
        "c new",
        "n/p win",
        "%/\" split",
        "h/j/k/l move",
        "z zoom",
        "x kill",
        "Tab alert",
        "s tree",
        "[ copy",
        "d detach",
    ] {
        assert!(line.contains(want), "missing {want:?} in {line:?}");
    }
    assert!(line.find("c new") < line.find("x kill"), "{line:?}");
    assert!(line.ends_with("? all  Esc cancel"), "{line:?}");
}

#[test]
fn which_key_follows_unbinding_and_drops_alert_without_binding() {
    let mut bindings = defaults();
    bindings.remove("x");
    bindings.remove("Tab");
    bindings.remove("BTab");
    let line = which_key(&bindings, 200, Lang::En);
    assert!(!line.contains("x kill"), "{line:?}");
    assert!(!line.contains("alert"), "{line:?}");
}

#[test]
fn which_key_sees_through_confirm_before() {
    let mut bindings = BTreeMap::new();
    bindings.insert("k".into(), "confirm-before -p \"kill? \" kill-pane".into());
    let line = which_key(&bindings, 120, Lang::En);
    assert!(line.contains("k kill"), "{line:?}");
}

#[test]
fn prompt_scrolls_to_keep_the_cursor_visible() {
    let buf = "abcdefghijklmnopqrstuvwxyz";
    let (text, col) = prompt_view(":", buf, buf.chars().count(), 10, "…");
    assert!(text.starts_with('…'), "{text:?}");
    assert!(text.ends_with("xyz"), "{text:?}");
    assert!(col <= 9, "{col}");
    let (start, c0) = prompt_view(":", buf, 0, 10, "…");
    assert!(start.starts_with("…a"), "{start:?}");
    assert_eq!(c0, 1);
}

#[test]
fn prompt_cursor_column_counts_display_width() {
    let (_, col) = prompt_view(":", "中文", 1, 80, "…");
    assert_eq!(col, 1 + 2);
}

#[test]
fn confirm_text_keeps_the_suffix() {
    let text = confirm_text("kill pane p2 (codex, working)?", 20, "…");
    assert!(text.ends_with(" (y/n) "), "{text:?}");
    assert!(text.width() <= 20, "{text:?}");
}

fn zh_input(session: &str, wins: Vec<WinItem>, active: usize, right: Right) -> StatusInput {
    StatusInput {
        lang: Lang::ZhTw,
        ..input(session, wins, active, right)
    }
}

#[test]
fn zh_tw_which_key_ends_with_the_reserved_end_at_every_width() {
    let bindings = defaults();
    for w in 25..=120u16 {
        let line = which_key(&bindings, w, Lang::ZhTw);
        assert_eq!(line.width(), usize::from(w), "w={w} {line:?}");
        assert!(line.ends_with(" ? 全部  Esc 取消"), "w={w} {line:?}");
        assert!(line.starts_with(" 前綴鍵 "), "w={w} {line:?}");
    }
}

#[test]
fn zh_tw_which_key_at_80_columns_lists_whole_pairs() {
    let line = which_key(&defaults(), 80, Lang::ZhTw);
    // priority order: the first pairs that fit are shown whole, the rest are dropped whole
    for want in ["c 新增", "n/p 視窗", "%/\" 分割", "h/j/k/l 移動", "z 放大"] {
        assert!(line.contains(want), "missing {want:?} in {line:?}");
    }
    for gone in ["x 關閉", "Tab 提醒", "d 離開"] {
        assert!(
            !line.contains(gone),
            "{gone:?} should be dropped in {line:?}"
        );
    }
    assert!(line.ends_with(" ? 全部  Esc 取消"), "{line:?}");
}

#[test]
fn zh_tw_which_key_drops_pairs_that_do_not_fit() {
    let line = which_key(&defaults(), 40, Lang::ZhTw);
    assert_eq!(line.width(), 40, "{line:?}");
    assert!(line.starts_with(" 前綴鍵 c 新增"), "{line:?}");
    assert!(line.ends_with(" ? 全部  Esc 取消"), "{line:?}");
    // no label is cut in the middle: every word left of the end is a whole pair
    let body = line.trim_end_matches(" ? 全部  Esc 取消").trim();
    assert!(!body.ends_with('視') && !body.ends_with('分'), "{body:?}");
}

#[test]
fn zh_tw_help_hint_and_counts_share_the_right_block() {
    let counts = vec![Count {
        glyph: "▲",
        status: Status::Blocked,
        n: 1,
    }];
    let right = Right::Bar {
        counts,
        tail: Tail::Help("C-b".into()),
    };
    let (text, _) = row(&zh_input("demo", vec![win("cmd")], 0, right), 80);
    assert!(text.ends_with("▲1   C-b ? 說明"), "{text:?}");
}

#[test]
fn zh_tw_right_block_is_never_overpainted_and_keeps_the_key() {
    for w in 20..=80u16 {
        let right = Right::Bar {
            counts: vec![],
            tail: Tail::Help("C-b".into()),
        };
        let (text, _) = row(&zh_input("demo", twelve(), 3, right), w);
        assert!(text.width() <= usize::from(w), "w={w} {text:?}");
        let end = text.trim_end();
        assert!(
            end.ends_with("C-b") || end.ends_with("C-b ? 說明"),
            "w={w} {text:?}"
        );
    }
}

#[test]
fn zh_tw_copy_mode_chip_and_hints() {
    let (wide, _) = row(&zh_input("demo", vec![win("cmd")], 0, Right::Copy), 120);
    assert!(
        wide.contains(" 複製模式 ") && wide.trim_end().ends_with("y 複製  q 結束"),
        "{wide:?}"
    );
    let (narrow, _) = row(&zh_input("demo", vec![win("cmd")], 0, Right::Copy), 40);
    assert!(narrow.contains(" 複製模式 "), "{narrow:?}");
    assert!(narrow.contains("0:cmd"), "{narrow:?}");
}

#[test]
fn zh_tw_status_row_keeps_the_windows_when_narrow() {
    let inp = zh_input("工作", vec![win("cmd")], 0, key_bar("C-b"));
    let (text, hits) = row(&inp, 30);
    assert!(text.contains("0:cmd"), "{text:?}");
    assert!(text.width() <= 30, "{text:?}");
    assert!(!hits.is_empty());
}
