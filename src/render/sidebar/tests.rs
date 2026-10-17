use super::*;
use crate::theme::ASCII;

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

fn row_text(g: &Grid, y: u16) -> String {
    (0..g.w)
        .map(|x| &g.cells[y as usize * g.w as usize + x as usize])
        .filter(|c| !c.skip)
        .map(|c| c.s.as_str())
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Lay out and draw the groups into a `width` x `height` grid, as `draw_sidebar` does.
fn draw(groups: &[GroupView], any: bool, hints: bool, width: u16, height: u16) -> Grid {
    let rows = layout_rows(groups, any, hints, width, height);
    let t = Theme::terminal();
    let lane = Lane {
        t: &t,
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

fn y_of(rows: &[SidebarRow], kind: SidebarKind) -> Option<u16> {
    rows.iter().find(|r| r.kind == kind).map(|r| r.y)
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
    let pointer = &g.cells[focused.y as usize * 25];
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
        wrap(EMPTY_TEXT, 25),
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
