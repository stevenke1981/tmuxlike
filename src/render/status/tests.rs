use super::*;
use crate::commands::DEFAULT_BINDINGS;

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

#[test]
fn status_hit_reads_the_last_drawn_ranges() {
    let inp = input("hit-cache-s", vec![win("a"), win("b")], 0, key_bar("C-b"));
    let layout = status_layout(&inp, 40);
    remember_hits("hit-cache-s", layout.hits.clone());
    let sess = Session {
        name: "hit-cache-s".into(),
        windows: vec![],
        active: 0,
        cols: 40,
        rows: 24,
        sidebar: false,
        cwd: None,
        last_used: std::time::Instant::now(),
    };
    let (a0, b0) = (layout.hits[0].1, layout.hits[0].2);
    let (a1, _) = (layout.hits[1].1, layout.hits[1].2);
    assert_eq!(status_hit(&sess, a0), Some(0));
    assert_eq!(status_hit(&sess, b0), None, "gap column is not a window");
    assert_eq!(status_hit(&sess, a1), Some(1));
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
        glyph: "✗",
        text: "unknown command: a-very-long-error-message-that-keeps-going".into(),
        error: true,
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
fn info_message_uses_ok_glyph() {
    let msg = Right::Message {
        glyph: "✓",
        text: "copied 42 chars".into(),
        error: false,
    };
    let (text, _) = row(&input("demo", vec![win("cmd")], 0, msg), 80);
    assert!(text.trim_end().ends_with("✓ copied 42 chars"), "{text:?}");
}

#[test]
fn which_key_width_40_keeps_the_reserved_end() {
    let line = which_key(&defaults(), 40);
    assert_eq!(line.width(), 40, "{line:?}");
    assert!(line.ends_with("? all  Esc cancel"), "{line:?}");
    assert!(line.starts_with(" PREFIX c new"), "{line:?}");
}

#[test]
fn which_key_full_line_at_120_columns() {
    let line = which_key(&defaults(), 120);
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
    let line = which_key(&bindings, 200);
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
    let line = which_key(&bindings, 200);
    assert!(!line.contains("x kill"), "{line:?}");
    assert!(!line.contains("alert"), "{line:?}");
}

#[test]
fn which_key_sees_through_confirm_before() {
    let mut bindings = BTreeMap::new();
    bindings.insert("k".into(), "confirm-before -p \"kill? \" kill-pane".into());
    let line = which_key(&bindings, 120);
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
