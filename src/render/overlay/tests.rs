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
    let rows = chooser_rows(&sample(), Filter::All, &UNICODE);
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
    let rows = chooser_rows(&sample(), Filter::Only(Status::Blocked), &UNICODE);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].prefix, "└ ", "only window left is the last one");
    assert_eq!(rows[2].prefix, "  └ ");
}

#[test]
fn session_text_uses_plural_and_attached_tag() {
    assert_eq!(session_text("work", 2, false), "work: 2 windows");
    assert_eq!(session_text("solo", 1, true), "solo: 1 window (attached)");
    assert_eq!(session_text("empty", 0, false), "empty: 0 windows");
}

#[test]
fn window_and_pane_text_plurals_and_markers() {
    assert_eq!(window_text(0, "cmd", true, 1), "0: cmd*  1 pane");
    assert_eq!(window_text(2, "logs", false, 3), "2: logs  3 panes");
    assert_eq!(
        pane_text(0, 1, "codex", Status::Working),
        "0.1 codex working"
    );
    assert_eq!(count_noun(1, "window", "windows"), "1 window");
    assert_eq!(count_noun(0, "pane", "panes"), "0 panes");
}

#[test]
fn row_text_for_session_window_pane() {
    let rows = chooser_rows(&sample(), Filter::All, &UNICODE);
    assert_eq!(rows[0].text, "demo: 2 windows (attached)");
    assert!(rows[0].bold);
    assert_eq!(rows[1].text, "0: cmd*  2 panes");
    assert_eq!(rows[2].text, "0.0 claude blocked");
    assert_eq!(rows[2].status, Status::Blocked);
}

#[test]
fn kill_target_session_names_windows_and_blocked_count() {
    let t = kill_target_in(&sample(), (0, None, None)).unwrap();
    assert_eq!(t.question, "kill session demo (2 windows, 1 blocked)?");
    assert_eq!(t.command, "kill-session -t \"demo\"");
}

#[test]
fn kill_target_window_uses_session_colon_index() {
    let t = kill_target_in(&sample(), (0, Some(0), None)).unwrap();
    assert_eq!(t.question, "kill window 0 cmd (2 panes, 1 blocked)?");
    assert_eq!(t.command, "kill-window -t \"demo:0\"");
}

#[test]
fn kill_target_pane_has_none() {
    assert!(kill_target_in(&sample(), (0, Some(0), Some(0))).is_none());
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
    assert_eq!(chooser_title(Filter::All), "Sessions");
    assert_eq!(
        chooser_title(Filter::Only(Status::Blocked)),
        "Sessions [blocked]"
    );
    assert_eq!(empty_note(Filter::Only(Status::Done)), "no done panes");
}

#[test]
fn footer_hints_name_the_keys_in_both_glyph_sets() {
    let u = chooser_hints(&UNICODE);
    assert!(u.contains("b/w/d/i filter") && u.ends_with("esc close"));
    assert!(!chooser_hints(&ASCII).contains('↑'));
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
