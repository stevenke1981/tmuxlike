//! Unit tests for the chooser tree, filters, kill targets and scroll math.
use super::*;
use crate::theme::UNICODE;

fn pane(status: Status, who: &str) -> NodePane {
    NodePane {
        status,
        who: who.into(),
    }
}

fn win(name: &str, active: bool, panes: Vec<NodePane>) -> NodeWindow {
    NodeWindow {
        name: name.into(),
        active,
        panes,
    }
}

/// demo: cmd (blocked claude, working codex) + logs (idle); work: a (done pi) + b (working).
fn sample() -> Vec<NodeSession> {
    vec![
        NodeSession {
            name: "demo".into(),
            attached: true,
            windows: vec![
                win(
                    "cmd",
                    true,
                    vec![
                        pane(Status::Blocked, "claude"),
                        pane(Status::Working, "codex"),
                    ],
                ),
                win("logs", false, vec![pane(Status::Idle, "logs")]),
            ],
        },
        NodeSession {
            name: "work".into(),
            attached: false,
            windows: vec![
                win("a", false, vec![pane(Status::Done, "pi")]),
                win("b", true, vec![pane(Status::Working, "x")]),
            ],
        },
    ]
}

#[test]
fn entries_are_session_window_pane_in_order() {
    let e = entries_of(&sample());
    assert_eq!(e[0], (0, None, None));
    assert_eq!(e[1], (0, Some(0), None));
    assert_eq!(e[2], (0, Some(0), Some(0)));
    assert_eq!(e[3], (0, Some(0), Some(1)));
    assert_eq!(e[4], (0, Some(1), None));
    assert_eq!(e[6], (1, None, None));
    assert_eq!(e.len(), 11);
}

#[test]
fn filter_blocked_keeps_only_blocked_pane_and_its_ancestors() {
    let got = filter_entries(&sample(), Filter::Only(Status::Blocked));
    assert_eq!(
        got,
        vec![(0, None, None), (0, Some(0), None), (0, Some(0), Some(0))]
    );
}

#[test]
fn filter_working_spans_sessions() {
    let got = filter_entries(&sample(), Filter::Only(Status::Working));
    assert!(got.contains(&(0, Some(0), Some(1))));
    assert!(got.contains(&(1, Some(1), Some(0))));
    assert!(
        !got.contains(&(0, Some(1), None)),
        "logs window holds no working pane"
    );
}

#[test]
fn filter_without_matches_is_empty() {
    assert!(filter_entries(&sample(), Filter::Only(Status::Unknown)).is_empty());
}

#[test]
fn all_filter_returns_every_row() {
    assert_eq!(
        filter_entries(&sample(), Filter::All),
        entries_of(&sample())
    );
}

#[test]
fn rollup_precedence_blocked_done_working_unknown_idle() {
    use Status::*;
    assert_eq!(rollup([Idle, Working, Done].into_iter()), Done);
    assert_eq!(rollup([Done, Blocked, Working].into_iter()), Blocked);
    assert_eq!(rollup([Idle, Unknown].into_iter()), Unknown);
    assert_eq!(rollup(std::iter::empty()), Idle);
}

#[test]
fn session_row_shows_rollup_status() {
    assert_eq!(status_of(&sample(), (0, None, None)), Status::Blocked);
    assert_eq!(status_of(&sample(), (1, None, None)), Status::Done);
}

#[test]
fn last_child_uses_corner_and_others_tee() {
    let rows = chooser_rows(&sample(), Filter::All, &UNICODE, Lang::En);
    assert_eq!(rows[1].prefix, "├ ", "cmd has a later sibling");
    assert_eq!(rows[4].prefix, "└ ", "logs is the last window");
    assert_eq!(
        rows[2].prefix, "│ ├ ",
        "claude: parent continues, pane not last"
    );
    assert_eq!(rows[3].prefix, "│ └ ", "codex: last pane");
    assert_eq!(rows[5].prefix, "  └ ", "logs pane under a last window");
    assert_eq!(rows[0].prefix, "");
}

#[test]
fn filtered_rows_recompute_connectors() {
    let rows = chooser_rows(&sample(), Filter::Only(Status::Blocked), &UNICODE, Lang::En);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].prefix, "└ ", "only window left is the last one");
    assert_eq!(rows[2].prefix, "  └ ");
}

#[test]
fn session_text_uses_plural_and_attached_tag() {
    assert_eq!(session_text(Lang::En, "work", 2, false), "work: 2 windows");
    assert_eq!(
        session_text(Lang::En, "solo", 1, true),
        "solo: 1 window (attached)"
    );
    assert_eq!(
        session_text(Lang::En, "empty", 0, false),
        "empty: 0 windows"
    );
}

#[test]
fn window_and_pane_text_plurals_and_markers() {
    assert_eq!(window_text(Lang::En, 0, "cmd", true, 1), "0: cmd*  1 pane");
    assert_eq!(
        window_text(Lang::En, 2, "logs", false, 3),
        "2: logs  3 panes"
    );
    assert_eq!(pane_text(0, 1, "codex"), "0.1 codex");
    assert_eq!(count_text(Lang::En, 1, Noun::Window), "1 window");
    assert_eq!(count_text(Lang::En, 0, Noun::Pane), "0 panes");
}

#[test]
fn row_text_for_session_window_pane() {
    let rows = chooser_rows(&sample(), Filter::All, &UNICODE, Lang::En);
    assert_eq!(rows[0].text, "demo: 2 windows (attached)");
    assert!(rows[0].bold);
    assert_eq!(rows[0].word, None);
    assert_eq!(rows[1].text, "0: cmd*  2 panes");
    assert_eq!(rows[1].word, None);
    assert_eq!(rows[2].text, "0.0 claude");
    assert_eq!(
        rows[2].word,
        Some("blocked"),
        "state word is its own muted run"
    );
    assert_eq!(rows[2].status, Status::Blocked);
}

#[test]
fn muted_chrome_is_faint_in_mono_and_muted_color_otherwise() {
    let color = Theme::terminal();
    assert_eq!(muted_style(&color), (color.muted, 0));
    let mono = Theme {
        mono: true,
        ..Theme::terminal()
    };
    assert_eq!(muted_style(&mono), (Color::Reset, DIM));
}

#[test]
fn chooser_footer_in_ascii_theme_is_ascii() {
    let ascii = Theme {
        ascii: true,
        g: ASCII,
        ..Theme::terminal()
    };
    let footer = chooser_hints(&ascii.g, Lang::En);
    assert!(footer.is_ascii(), "{footer:?}");
    assert!(footer.contains("esc close"));
}

#[test]
fn ascii_chooser_rows_and_titles_stay_ascii() {
    let rows = chooser_rows(&sample(), Filter::All, &ASCII, Lang::En);
    for r in &rows {
        assert!(r.prefix.is_ascii() && r.text.is_ascii(), "{r:?}");
    }
    assert!(chooser_title(Lang::En, Filter::Only(Status::Done)).is_ascii());
}

#[test]
fn kill_target_session_names_windows_and_blocked_count() {
    let t = kill_target_in(&sample(), (0, None, None), Lang::En).unwrap();
    assert_eq!(t.question, "kill session demo (2 windows, 1 blocked)?");
    assert_eq!(t.command, "kill-session -t \"demo\"");
}

#[test]
fn kill_target_window_uses_session_colon_index() {
    let t = kill_target_in(&sample(), (0, Some(0), None), Lang::En).unwrap();
    assert_eq!(t.question, "kill window 0 cmd (2 panes, 1 blocked)?");
    assert_eq!(t.command, "kill-window -t \"demo:0\"");
}

#[test]
fn kill_target_pane_has_none() {
    assert!(kill_target_in(&sample(), (0, Some(0), Some(0)), Lang::En).is_none());
}

#[test]
fn arg_quote_escapes_quote_and_backslash() {
    assert_eq!(arg_quote(r#"a"b\c"#), r#""a\"b\\c""#);
}

#[test]
fn pack_roundtrip_and_plain_selection_is_all() {
    for f in [
        Filter::All,
        Filter::Only(Status::Blocked),
        Filter::Only(Status::Working),
        Filter::Only(Status::Done),
        Filter::Only(Status::Idle),
    ] {
        assert_eq!(unpack_chooser(pack_chooser(7, f)), (7, f));
    }
    assert_eq!(unpack_chooser(7), (7, Filter::All));
}

#[test]
fn filter_keys_map_and_others_do_not() {
    assert_eq!(Filter::from_key('b'), Some(Filter::Only(Status::Blocked)));
    assert_eq!(Filter::from_key('w'), Some(Filter::Only(Status::Working)));
    assert_eq!(Filter::from_key('d'), Some(Filter::Only(Status::Done)));
    assert_eq!(Filter::from_key('i'), Some(Filter::Only(Status::Idle)));
    assert_eq!(Filter::from_key('a'), Some(Filter::All));
    assert_eq!(Filter::from_key('z'), None);
}

#[test]
fn titles_show_active_filter() {
    assert_eq!(chooser_title(Lang::En, Filter::All), "Sessions");
    assert_eq!(
        chooser_title(Lang::En, Filter::Only(Status::Blocked)),
        "Sessions [blocked]"
    );
    assert_eq!(
        empty_note(Lang::En, Filter::Only(Status::Done)),
        "no done panes"
    );
}

#[test]
fn footer_hints_name_the_keys_in_both_glyph_sets() {
    let u = chooser_hints(&UNICODE, Lang::En);
    assert!(u.contains("b/w/d/i filter") && u.ends_with("esc close"));
    assert!(!chooser_hints(&ASCII, Lang::En).contains('↑'));
}

#[test]
fn cut_respects_width_and_ellipsis() {
    assert_eq!(cut("hello", 5, "…"), "hello");
    assert_eq!(cut("hello", 3, "~"), "he~");
    let c = cut("中文名稱很長", 7, "…");
    assert_eq!(c, "中文名…");
    assert!(c.width() <= 7);
    assert_eq!(cut("abc", 0, "…"), "");
}

#[test]
fn window_start_keeps_selection_visible() {
    for len in 1..12 {
        for inner in 1..6 {
            for sel in 0..len {
                let s = window_start(len, inner, Some(sel), 0);
                assert!(
                    s <= sel && sel < s + inner,
                    "len={len} inner={inner} sel={sel} s={s}"
                );
                assert!(s + inner <= len.max(inner));
            }
        }
    }
}

#[test]
fn window_start_clamps_scroll_without_selection() {
    assert_eq!(window_start(10, 4, None, 99), 6);
    assert_eq!(window_start(3, 4, None, 2), 0);
}

#[test]
fn zh_tw_chooser_labels_use_glossary_terms() {
    assert_eq!(session_text(Lang::ZhTw, "work", 2, false), "work：2 個視窗");
    assert_eq!(
        session_text(Lang::ZhTw, "solo", 1, true),
        "solo：1 個視窗（已連線）"
    );
    assert_eq!(
        window_text(Lang::ZhTw, 0, "cmd", true, 1),
        "0：cmd*  1 個窗格"
    );
    assert_eq!(count_text(Lang::ZhTw, 3, Noun::Pane), "3 個窗格");
    let rows = chooser_rows(&sample(), Filter::All, &UNICODE, Lang::ZhTw);
    assert_eq!(rows[2].word, Some("待回應"));
    assert_eq!(rows[3].word, Some("執行中"));
    assert_eq!(rows[5].word, Some("閒置"));
    assert_eq!(rows[0].text, "demo：2 個視窗（已連線）");
}

#[test]
fn zh_tw_titles_filters_and_footers() {
    assert_eq!(chooser_title(Lang::ZhTw, Filter::All), "工作階段");
    assert_eq!(
        chooser_title(Lang::ZhTw, Filter::Only(Status::Blocked)),
        "工作階段 [待回應]"
    );
    assert_eq!(
        empty_note(Lang::ZhTw, Filter::Only(Status::Done)),
        "沒有已完成的窗格"
    );
    assert_eq!(empty_note(Lang::ZhTw, Filter::All), "沒有工作階段");
    let footer = chooser_hints(&UNICODE, Lang::ZhTw);
    assert!(
        footer.contains("篩選") && footer.ends_with("關閉"),
        "{footer}"
    );
    assert!(chooser_hints(&ASCII, Lang::ZhTw).contains("開啟"));
}

#[test]
fn zh_tw_kill_questions_name_the_target_and_blocked_count() {
    let s = kill_target_in(&sample(), (0, None, None), Lang::ZhTw).unwrap();
    assert_eq!(s.question, "關閉工作階段 demo（2 個視窗，待回應 1）？");
    assert_eq!(s.command, "kill-session -t \"demo\"");
    let w = kill_target_in(&sample(), (0, Some(0), None), Lang::ZhTw).unwrap();
    assert_eq!(w.question, "關閉視窗 0 cmd（2 個窗格，待回應 1）？");
    assert_eq!(w.command, "kill-window -t \"demo:0\"");
}

#[test]
fn zh_tw_scroll_markers_and_popup_chrome_are_translated() {
    assert_eq!(tr(Lang::ZhTw, "render.scroll.up"), "▲ 更多");
    assert_eq!(tr(Lang::ZhTw, "render.scroll.down_ascii"), "v 更多");
    assert_eq!(tr(Lang::En, "render.scroll.up"), "▲ more");
    assert_eq!(tr(Lang::ZhTw, "render.popup.title"), "按鍵");
    assert_eq!(tr(Lang::ZhTw, "render.popup.hint"), "j/k 捲動  esc 關閉");
}
