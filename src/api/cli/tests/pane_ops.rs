//! Pane targets, split, zoom, focus, rename and read.
use super::{is_err, run, run_at};
use crate::api::{Direction, ReadSource, Request, Target, ZoomMode};

#[test]
fn current_flag_uses_caller_pane_or_fails() {
    assert_eq!(
        run_at(&["pane", "current", "--current"], Some(4)),
        Ok(Request::PaneCurrent { pane: Some(4) })
    );
    assert!(run_at(&["pane", "current", "--current"], None).is_err());
    assert_eq!(
        run_at(&["pane", "current"], Some(4)),
        Ok(Request::PaneCurrent { pane: Some(4) })
    );
    assert_eq!(
        run_at(&["pane", "current"], None),
        Ok(Request::PaneCurrent { pane: None })
    );
    assert_eq!(
        run_at(&["pane", "current", "--pane", "p2"], None),
        Ok(Request::PaneCurrent { pane: Some(2) })
    );
    assert!(run_at(&["pane", "current", "--pane", "p2", "--current"], Some(4)).is_err());
}

#[test]
fn split_defaults_and_all_options() {
    assert_eq!(
        run(&["pane", "split", "--direction", "right"]),
        Ok(Request::PaneSplit {
            pane: None,
            direction: Direction::Right,
            ratio: None,
            cwd: None,
            focus: false,
        })
    );
    assert_eq!(
        run_at(&["pane", "split", "--direction", "down"], Some(5)),
        Ok(Request::PaneSplit {
            pane: Some(5),
            direction: Direction::Down,
            ratio: None,
            cwd: None,
            focus: false,
        })
    );
    assert_eq!(
        run(&[
            "pane",
            "split",
            "p9",
            "--direction=down",
            "--ratio=0.3",
            "--cwd",
            "/tmp/x y",
            "--focus"
        ]),
        Ok(Request::PaneSplit {
            pane: Some(9),
            direction: Direction::Down,
            ratio: Some(0.3),
            cwd: Some("/tmp/x y".into()),
            focus: true,
        })
    );
    assert_eq!(
        run(&[
            "pane",
            "split",
            "--pane",
            "p2",
            "--direction",
            "right",
            "--no-focus"
        ]),
        Ok(Request::PaneSplit {
            pane: Some(2),
            direction: Direction::Right,
            ratio: None,
            cwd: None,
            focus: false,
        })
    );
}

#[test]
fn split_rejects_left_up_missing_direction_and_double_targets() {
    assert!(is_err(&["pane", "split"]));
    assert!(is_err(&["pane", "split", "--direction", "left"]));
    assert!(is_err(&["pane", "split", "--direction", "up"]));
    assert!(is_err(&["pane", "split", "--direction", "sideways"]));
    assert!(is_err(&[
        "pane",
        "split",
        "p1",
        "p2",
        "--direction",
        "right"
    ]));
    assert!(is_err(&[
        "pane",
        "split",
        "p1",
        "--pane",
        "p2",
        "--direction",
        "right"
    ]));
    assert!(is_err(&[
        "pane",
        "split",
        "--direction",
        "right",
        "--focus",
        "--no-focus"
    ]));
    assert!(is_err(&[
        "pane",
        "split",
        "--direction",
        "right",
        "--direction",
        "down"
    ]));
    assert!(
        run_at(
            &["pane", "split", "--direction", "right", "--current"],
            None
        )
        .is_err()
    );
}

#[test]
fn ratio_must_be_strictly_between_zero_and_one() {
    for good in ["0.001", "0.5", "0.999"] {
        assert!(
            run(&["pane", "split", "--direction", "right", "--ratio", good]).is_ok(),
            "{good}"
        );
    }
    for bad in ["0", "1", "-0.1", "1.5", "NaN", "inf", "half"] {
        assert!(
            is_err(&["pane", "split", "--direction", "right", "--ratio", bad]),
            "{bad}"
        );
    }
}

#[test]
fn zoom_modes_and_targets() {
    let zoom = |pane, mode| Ok(Request::PaneZoom { pane, mode });
    assert_eq!(run(&["pane", "zoom"]), zoom(None, ZoomMode::Toggle));
    assert_eq!(
        run(&["pane", "zoom", "p3", "--on"]),
        zoom(Some(3), ZoomMode::On)
    );
    assert_eq!(
        run(&["pane", "zoom", "--off", "--pane", "p5"]),
        zoom(Some(5), ZoomMode::Off)
    );
    assert_eq!(
        run(&["pane", "zoom", "--toggle"]),
        zoom(None, ZoomMode::Toggle)
    );
    assert_eq!(
        run_at(&["pane", "zoom", "--current", "--on"], Some(6)),
        zoom(Some(6), ZoomMode::On)
    );
    assert!(is_err(&["pane", "zoom", "--on", "--off"]));
    assert!(run_at(&["pane", "zoom", "p1", "--current"], Some(3)).is_err());
    assert!(run_at(&["pane", "zoom", "--current"], None).is_err());
}

#[test]
fn zoom_without_target_falls_back_to_current_pane() {
    let zoom = |pane, mode| Ok(Request::PaneZoom { pane, mode });
    assert_eq!(
        run_at(&["pane", "zoom"], Some(4)),
        zoom(Some(4), ZoomMode::Toggle)
    );
    assert_eq!(
        run_at(&["pane", "zoom", "--on"], Some(4)),
        zoom(Some(4), ZoomMode::On)
    );
    assert_eq!(
        run_at(&["pane", "zoom"], None),
        zoom(None, ZoomMode::Toggle)
    );
    // an explicit target still wins over the caller's pane
    assert_eq!(
        run_at(&["pane", "zoom", "p3"], Some(4)),
        zoom(Some(3), ZoomMode::Toggle)
    );
    assert_eq!(
        run_at(&["pane", "zoom", "--pane", "p5", "--off"], Some(4)),
        zoom(Some(5), ZoomMode::Off)
    );
}

#[test]
fn focus_by_direction_and_by_id() {
    assert_eq!(
        run(&["pane", "focus", "--direction", "left"]),
        Ok(Request::PaneFocusDirection {
            pane: None,
            direction: Direction::Left
        })
    );
    assert_eq!(
        run_at(
            &["pane", "focus", "--direction", "up", "--current"],
            Some(2)
        ),
        Ok(Request::PaneFocusDirection {
            pane: Some(2),
            direction: Direction::Up
        })
    );
    assert_eq!(
        run(&["pane", "focus", "--direction", "down", "--pane", "p5"]),
        Ok(Request::PaneFocusDirection {
            pane: Some(5),
            direction: Direction::Down
        })
    );
    assert!(is_err(&["pane", "focus", "--direction", "diagonal"]));
    assert!(is_err(&[
        "pane",
        "focus",
        "--direction",
        "left",
        "--current"
    ]));
    assert!(is_err(&["pane", "focus", "--pane", "p1"]));
    assert!(is_err(&["pane", "focus", "p1", "--direction", "left"]));
}

#[test]
fn focus_by_direction_falls_back_to_current_pane() {
    assert_eq!(
        run_at(&["pane", "focus", "--direction", "left"], Some(4)),
        Ok(Request::PaneFocusDirection {
            pane: Some(4),
            direction: Direction::Left
        })
    );
    assert_eq!(
        run_at(&["pane", "focus", "--direction", "left"], None),
        Ok(Request::PaneFocusDirection {
            pane: None,
            direction: Direction::Left
        })
    );
    assert_eq!(
        run_at(
            &["pane", "focus", "--direction", "right", "--pane", "p6"],
            Some(4)
        ),
        Ok(Request::PaneFocusDirection {
            pane: Some(6),
            direction: Direction::Right
        })
    );
    // a caller pane is required for --current, as before
    assert!(run_at(&["pane", "focus", "--direction", "up", "--current"], None).is_err());
}

#[test]
fn rename_sets_label_or_clears_it() {
    assert_eq!(
        run(&["pane", "rename", "p1", "工作 區 my label"]),
        Ok(Request::PaneRename {
            pane: 1,
            label: Some("工作 區 my label".into())
        })
    );
    assert_eq!(
        run(&["pane", "rename", "p1", "--clear"]),
        Ok(Request::PaneRename {
            pane: 1,
            label: None
        })
    );
    assert!(is_err(&["pane", "rename", "p1"]));
    assert!(is_err(&["pane", "rename", "p1", "--clear", "x"]));
    assert!(is_err(&["pane", "rename", "p1", ""]));
    assert!(is_err(&["pane", "rename", "p1", "a", "b"]));
}

#[test]
fn read_defaults_source_and_lines() {
    assert_eq!(
        run(&["pane", "read", "p1"]),
        Ok(Request::PaneRead {
            pane: 1,
            source: ReadSource::Recent,
            lines: None
        })
    );
    assert_eq!(
        run(&["pane", "read", "p1", "--source", "visible", "--lines", "5"]),
        Ok(Request::PaneRead {
            pane: 1,
            source: ReadSource::Visible,
            lines: Some(5)
        })
    );
    assert_eq!(
        run(&["agent", "read", "claude-1", "--lines=3"]),
        Ok(Request::AgentRead {
            target: Target::Name("claude-1".into()),
            source: ReadSource::Recent,
            lines: Some(3),
        })
    );
    assert!(is_err(&["pane", "read", "p1", "--lines", "0"]));
    assert!(is_err(&["pane", "read", "p1", "--lines=abc"]));
    assert!(is_err(&["pane", "read", "p1", "--source", "bogus"]));
    assert!(is_err(&[
        "pane", "read", "p1", "--source", "recent", "--source", "visible"
    ]));
}
