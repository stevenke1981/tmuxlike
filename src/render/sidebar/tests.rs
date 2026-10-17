use super::*;
use crate::theme::ASCII;

/// Session index of the attached group in [`sixteen_groups`].
const ATTACHED: usize = 9;

fn pv(pid: usize, status: Status, name: &str, message: Option<&str>, focused: bool) -> PaneView {
    PaneView {
        pid,
        status,
        name: name.into(),
        message: message.map(String::from),
        focused,
    }
}

fn group(si: usize, name: &str, open: bool, panes: Vec<PaneView>) -> GroupView {
    GroupView {
        si,
        name: name.into(),
        open,
        panes,
    }
}

fn blank(w: u16, h: u16) -> Grid {
    Grid {
        w,
        h,
        cells: vec![Cell::default(); w as usize * h as usize],
    }
}

fn cell(g: &Grid, x: u16, y: u16) -> &Cell {
    &g.cells[y as usize * g.w as usize + x as usize]
}

fn row_text(g: &Grid, y: u16) -> String {
    (0..g.w)
        .map(|x| cell(g, x, y))
        .filter(|c| !c.skip)
        .map(|c| c.s.as_str())
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Lay out and draw the groups into a `width` x `height` grid, as `draw_sidebar` does.
fn draw_with(
    t: &Theme,
    groups: &[GroupView],
    any: bool,
    hints: bool,
    width: u16,
    height: u16,
) -> Grid {
    let rows = layout_rows(groups, any, hints, width, height);
    let lane = Lane {
        t,
        tx: 0,
        tw: width,
        prefix: "C-b".into(),
    };
    let mut g = blank(width, height);
    for r in &rows {
        draw_row(&mut g, &lane, r, groups);
    }
    g
}

fn draw(groups: &[GroupView], any: bool, hints: bool, width: u16, height: u16) -> Grid {
    draw_with(&Theme::terminal(), groups, any, hints, width, height)
}

fn y_of(rows: &[SidebarRow], kind: SidebarKind) -> Option<u16> {
    rows.iter().find(|r| r.kind == kind).map(|r| r.y)
}

fn more_count(rows: &[SidebarRow]) -> Option<usize> {
    rows.iter().find_map(|r| match r.kind {
        SidebarKind::More(n) => Some(n),
        _ => None,
    })
}

fn closed_sessions(rows: &[SidebarRow]) -> Vec<usize> {
    rows.iter()
        .filter(|r| r.kind == SidebarKind::SessionClosed)
        .filter_map(|r| r.session)
        .collect()
}

/// Sixteen sessions; the attached one has two panes and its second pane is focused.
fn sixteen_groups() -> Vec<GroupView> {
    (0..16)
        .map(|si| {
            if si == ATTACHED {
                group(
                    si,
                    "s9",
                    true,
                    vec![
                        pv(91, Status::Idle, "a", None, false),
                        pv(92, Status::Working, "b", None, true),
                    ],
                )
            } else {
                group(
                    si,
                    &format!("s{si}"),
                    false,
                    vec![pv(100 + si, Status::Working, "x", None, false)],
                )
            }
        })
        .collect()
}

/// One attached session with `n` idle panes (pids 1..=n); `focus` is the focused pid.
fn many_panes(n: usize, focus: usize) -> Vec<GroupView> {
    let panes = (1..=n)
        .map(|i| pv(i, Status::Idle, &format!("a{i}"), None, i == focus))
        .collect();
    vec![group(0, "demo", true, panes)]
}

#[test]
fn focused_row_has_pointer_and_bold() {
    let groups = vec![group(
        0,
        "demo",
        true,
        vec![
            pv(1, Status::Idle, "pi", None, false),
            pv(2, Status::Blocked, "claude", Some("need approval"), true),
        ],
    )];
    let rows = layout_rows(&groups, true, true, 25, 12);
    let g = draw(&groups, true, true, 25, 12);
    let focused = rows
        .iter()
        .find(|r| r.kind == SidebarKind::Pane && r.pane == Some(2))
        .unwrap();
    let plain = rows
        .iter()
        .find(|r| r.kind == SidebarKind::Pane && r.pane == Some(1))
        .unwrap();
    assert!(
        row_text(&g, focused.y).starts_with("› ▲ claude"),
        "{:?}",
        row_text(&g, focused.y)
    );
    assert!(
        row_text(&g, plain.y).starts_with("  ○ pi"),
        "{:?}",
        row_text(&g, plain.y)
    );
    let pointer = cell(&g, 0, focused.y);
    assert_eq!(pointer.attrs & BOLD, BOLD, "focused pointer is bold");
}

#[test]
fn blocked_message_sits_on_indented_detail_line() {
    let groups = vec![group(
        0,
        "demo",
        true,
        vec![pv(
            2,
            Status::Blocked,
            "claude",
            Some("need approval"),
            false,
        )],
    )];
    let rows = layout_rows(&groups, true, true, 25, 12);
    let pane_y = y_of(&rows, SidebarKind::Pane).unwrap();
    let g = draw(&groups, true, true, 25, 12);
    assert_eq!(y_of(&rows, SidebarKind::Detail), Some(pane_y + 1));
    assert_eq!(row_text(&g, pane_y + 1), "    need approval");
}

#[test]
fn legend_fits_a_25_column_slot() {
    let t = Theme::terminal();
    let mut a = Theme::terminal();
    a.g = ASCII;
    assert_eq!(legend_text(&t, false), "●work ▲block ◆done ○idle");
    assert_eq!(legend_text(&t, true), "● work ▲ block ◆ done ○ idle");
    assert!(legend_text(&t, false).width() <= 25);
    assert!(legend_text(&a, false).width() <= 25);
    assert!(legend_text(&t, true).width() <= 28);
}

#[test]
fn overflow_ends_with_more_marker_and_keeps_footer() {
    let panes: Vec<PaneView> = (1..=40)
        .map(|i| pv(i, Status::Idle, &format!("a{i}"), None, false))
        .collect();
    let rows = layout_rows(&[group(0, "demo", true, panes)], true, true, 25, 30);
    let more = rows
        .iter()
        .find(|r| matches!(r.kind, SidebarKind::More(_)))
        .unwrap();
    assert_eq!(more.kind, SidebarKind::More(18));
    assert_eq!(more.y, 25);
    assert_eq!(y_of(&rows, SidebarKind::Legend), Some(27));
    assert_eq!(y_of(&rows, SidebarKind::Hint), Some(28));
    assert!(
        rows.iter().all(|r| r.y <= 25 || r.y >= 27),
        "no row under the footer"
    );
}

#[test]
fn sixteen_sessions_at_height_ten_keep_attached_group_and_focus() {
    let groups = sixteen_groups();
    let rows = layout_rows(&groups, true, true, 25, 10);
    assert_eq!(more_count(&rows), Some(15), "all 15 closed groups hidden");
    assert!(y_of(&rows, SidebarKind::SessionOpen).is_some());
    assert!(closed_sessions(&rows).is_empty());
    let focused_y = rows
        .iter()
        .find(|r| r.pane == Some(92))
        .expect("focused pane row")
        .y;
    let g = draw(&groups, true, true, 25, 10);
    assert!(row_text(&g, y_of(&rows, SidebarKind::SessionOpen).unwrap()).starts_with("▾ s9"));
    assert!(
        row_text(&g, focused_y).starts_with("› ● b"),
        "{:?}",
        row_text(&g, focused_y)
    );
}

#[test]
fn closed_groups_nearest_the_attached_one_are_kept_first() {
    let groups = sixteen_groups();
    let rows = layout_rows(&groups, true, true, 25, 14);
    assert_eq!(closed_sessions(&rows), vec![8, 10]);
    assert_eq!(more_count(&rows), Some(13));
    let open_y = y_of(&rows, SidebarKind::SessionOpen).unwrap();
    let closed_y: Vec<u16> = rows
        .iter()
        .filter(|r| r.kind == SidebarKind::SessionClosed)
        .map(|r| r.y)
        .collect();
    assert!(
        closed_y[0] < open_y && open_y < closed_y[1],
        "{closed_y:?} {open_y}"
    );
}

#[test]
fn many_panes_in_open_group_keep_header_focus_and_marker_count() {
    let groups = many_panes(40, 25);
    let rows = layout_rows(&groups, true, true, 25, 12);
    assert_eq!(more_count(&rows), Some(36), "41 rows, 5 kept");
    let pane_ids: Vec<usize> = rows
        .iter()
        .filter(|r| r.kind == SidebarKind::Pane)
        .filter_map(|r| r.pane)
        .collect();
    assert_eq!(
        pane_ids,
        vec![23, 24, 25, 26],
        "window centred on the focus"
    );
    assert!(y_of(&rows, SidebarKind::SessionOpen).is_some());
    let g = draw(&groups, true, true, 25, 12);
    let focus_y = rows.iter().find(|r| r.pane == Some(25)).unwrap().y;
    assert!(
        row_text(&g, focus_y).starts_with("› ○ a25"),
        "{:?}",
        row_text(&g, focus_y)
    );
}

#[test]
fn focus_on_the_last_pane_moves_the_window_to_the_end() {
    let groups = many_panes(40, 40);
    let rows = layout_rows(&groups, true, true, 25, 12);
    let pane_ids: Vec<usize> = rows
        .iter()
        .filter(|r| r.kind == SidebarKind::Pane)
        .filter_map(|r| r.pane)
        .collect();
    assert_eq!(pane_ids, vec![37, 38, 39, 40]);
    assert_eq!(more_count(&rows), Some(36));
}

#[test]
fn blocked_detail_moves_with_its_pane_when_trimmed() {
    let mut panes: Vec<PaneView> = (1..=6)
        .map(|i| pv(i, Status::Idle, &format!("a{i}"), None, false))
        .collect();
    panes[2] = pv(3, Status::Blocked, "claude", Some("need approval"), true);
    let groups = vec![group(0, "demo", true, panes)];
    let rows = layout_rows(&groups, true, true, 25, 9);
    let pane_y = rows
        .iter()
        .find(|r| r.pane == Some(3) && r.kind == SidebarKind::Pane)
        .unwrap()
        .y;
    assert_eq!(y_of(&rows, SidebarKind::Detail), Some(pane_y + 1));
    assert_eq!(more_count(&rows), Some(5));
    let g = draw(&groups, true, true, 25, 9);
    assert_eq!(row_text(&g, pane_y + 1), "    need approval");
}

#[test]
fn collapsed_session_shows_counts_and_hides_zeros() {
    let groups = vec![
        group(0, "demo", true, vec![pv(1, Status::Idle, "pi", None, true)]),
        group(
            1,
            "build",
            false,
            vec![
                pv(2, Status::Working, "w1", None, false),
                pv(3, Status::Working, "w2", None, false),
                pv(4, Status::Blocked, "b1", Some("x"), false),
            ],
        ),
        group(
            2,
            "idle-only",
            false,
            vec![
                pv(5, Status::Idle, "i", None, false),
                pv(6, Status::Idle, "j", None, false),
            ],
        ),
    ];
    assert_eq!(
        counts(&groups[1].panes),
        vec![(Status::Blocked, 1), (Status::Working, 2)]
    );
    assert_eq!(counts(&groups[2].panes), vec![(Status::Idle, 2)]);
    let rows = layout_rows(&groups, true, true, 25, 14);
    let closed: Vec<u16> = rows
        .iter()
        .filter(|r| r.kind == SidebarKind::SessionClosed)
        .map(|r| r.y)
        .collect();
    let g = draw(&groups, true, true, 25, 14);
    let build = row_text(&g, closed[0]);
    assert!(build.starts_with("▸ build"), "{build:?}");
    assert!(build.ends_with("▲1 ●2"), "{build:?}");
    assert_eq!(build.width(), 25);
    assert!(row_text(&g, closed[1]).ends_with("○2"));
}

#[test]
fn state_word_drops_when_name_budget_is_small() {
    let groups = vec![group(
        0,
        "demo",
        true,
        vec![pv(2, Status::Blocked, "claude-long-name", None, false)],
    )];
    let rows = layout_rows(&groups, true, false, 9, 6);
    let g = draw(&groups, true, false, 9, 6);
    let y = y_of(&rows, SidebarKind::Pane).unwrap();
    let text = row_text(&g, y);
    assert!(!text.contains("blocked"), "{text:?}");
    assert!(text.starts_with("  ▲ "), "{text:?}");
    assert!(
        text.ends_with('…'),
        "name is cut with an ellipsis: {text:?}"
    );
    assert!(text.width() <= 9);
}

#[test]
fn empty_state_shows_only_without_agents_and_with_hints() {
    let groups = vec![group(
        0,
        "demo",
        true,
        vec![pv(1, Status::Idle, "sh", None, false)],
    )];
    assert_eq!(
        empty_lines(25, "—"),
        vec![
            "no agent reports — see:",
            "tmuxlike pane",
            "report-agent --help"
        ]
    );
    let count = |rows: &[SidebarRow]| {
        rows.iter()
            .filter(|r| matches!(r.kind, SidebarKind::Empty(_)))
            .count()
    };
    assert_eq!(count(&layout_rows(&groups, false, true, 25, 20)), 3);
    assert_eq!(count(&layout_rows(&groups, false, false, 25, 20)), 0);
    assert_eq!(count(&layout_rows(&groups, true, true, 25, 20)), 0);
}

#[test]
fn empty_hint_wraps_to_the_same_height_in_both_glyph_sets() {
    for w in [9u16, 12, 16, 25, 40] {
        assert_eq!(
            empty_lines(w, "—").len(),
            empty_lines(w, ASCII.dash).len(),
            "width {w}"
        );
    }
}

#[test]
fn footer_is_dropped_before_the_list_is_squeezed() {
    assert_eq!(footer_plan(true, 29), (true, true));
    assert_eq!(footer_plan(true, 4), (true, false));
    assert_eq!(footer_plan(true, 2), (false, false));
    assert_eq!(footer_plan(false, 29), (true, false));
}

#[test]
fn wrap_splits_long_words_and_respects_width() {
    assert_eq!(wrap("abcdefghij kl", 4), vec!["abcd", "efgh", "ij", "kl"]);
    assert!(wrap("x", 0).is_empty());
}

#[test]
fn clean_replaces_control_characters() {
    assert_eq!(clean("a\tb\nc"), "a b c");
}

/// Mono keeps muted chrome faint (DIM); colour mode must not gain any DIM.
#[test]
fn mono_muted_chrome_is_dim_and_colour_mode_is_not() {
    let groups = vec![
        group(
            0,
            "demo",
            true,
            vec![pv(
                2,
                Status::Blocked,
                "claude",
                Some("need approval"),
                true,
            )],
        ),
        group(
            1,
            "build",
            false,
            vec![
                pv(3, Status::Working, "w", None, false),
                pv(4, Status::Working, "w2", None, false),
            ],
        ),
    ];
    let rows = layout_rows(&groups, true, true, 25, 12);
    let row_y = |kind| y_of(&rows, kind).unwrap();
    let mut mono = Theme::terminal();
    mono.mono = true;
    let g = draw_with(&mono, &groups, true, true, 25, 12);
    let all_dim = |y: u16| {
        (0..25).all(|x| {
            let c = cell(&g, x, y);
            c.s == " " || c.attrs & DIM != 0
        })
    };
    assert!(all_dim(row_y(SidebarKind::Legend)), "legend");
    assert!(all_dim(row_y(SidebarKind::Hint)), "hint");
    assert!(all_dim(row_y(SidebarKind::Detail)), "blocked detail");
    let pane_y = row_y(SidebarKind::Pane);
    assert!(cell(&g, 24, pane_y).attrs & DIM != 0, "state word");
    let closed_y = row_y(SidebarKind::SessionClosed);
    assert!(cell(&g, 24, closed_y).attrs & DIM != 0, "collapsed count");

    let colour = draw(&groups, true, true, 25, 12);
    assert!(colour.cells.iter().all(|c| c.attrs & DIM == 0));
}

#[test]
fn mono_empty_state_and_more_marker_are_dim() {
    let groups = sixteen_groups();
    let mut mono = Theme::terminal();
    mono.mono = true;
    let g = draw_with(&mono, &groups, true, true, 25, 10);
    let more_y = layout_rows(&groups, true, true, 25, 10)
        .iter()
        .find(|r| matches!(r.kind, SidebarKind::More(_)))
        .unwrap()
        .y;
    assert!(cell(&g, 0, more_y).attrs & DIM != 0, "more marker");
    let empty = draw_with(
        &mono,
        &[group(0, "demo", true, vec![])],
        false,
        true,
        25,
        12,
    );
    let hint_y = layout_rows(&[group(0, "demo", true, vec![])], false, true, 25, 12)
        .iter()
        .find(|r| matches!(r.kind, SidebarKind::Empty(0)))
        .unwrap()
        .y;
    assert!(cell(&empty, 0, hint_y).attrs & DIM != 0, "empty state");
}

/// ASCII mode: every sidebar line is ASCII, including the overflow marker and the empty hint.
#[test]
fn ascii_sidebar_rows_are_ascii() {
    let mut t = Theme::terminal();
    t.g = ASCII;
    t.ascii = true;
    let mixed = vec![
        group(
            0,
            "demo",
            true,
            vec![pv(
                2,
                Status::Blocked,
                "claude",
                Some("need approval"),
                true,
            )],
        ),
        group(
            1,
            "build",
            false,
            vec![pv(3, Status::Done, "d", None, false)],
        ),
    ];
    let cases: [(Vec<GroupView>, bool, u16); 3] = [
        (sixteen_groups(), true, 10),
        (mixed.clone(), true, 12),
        (mixed, false, 12),
    ];
    for (groups, any, h) in &cases {
        let g = draw_with(&t, groups, *any, true, 25, *h);
        for y in 0..*h {
            let text = row_text(&g, y);
            assert!(text.is_ascii(), "row {y}: {text:?}");
        }
    }
    let overflow = draw_with(&t, &sixteen_groups(), true, true, 25, 10);
    assert!(
        row_text(&overflow, 5).contains("~ +15 more"),
        "{:?}",
        row_text(&overflow, 5)
    );
    // Tall enough that the hint is not shed, so its dash is visible.
    let empty = draw_with(&t, &cases[2].0, false, true, 25, 20);
    assert!(
        (0..20).any(|y| row_text(&empty, y).contains("reports - see:")),
        "dash comes from the ASCII table"
    );
}
