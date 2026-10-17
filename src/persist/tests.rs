use super::snapshot::*;
use super::*;
use crate::layout::{Dir, Node};
use crate::server::{Session, Window};
use std::collections::HashSet;

fn plain_leaf(id: usize) -> LeafSnap {
    LeafSnap {
        cwd: Some(format!("/work/{id}")),
        ..LeafSnap::default()
    }
}

/// Pane 2 is an agent with a label and a resume command.
fn lookup(id: usize) -> LeafSnap {
    if id == 2 {
        LeafSnap {
            cwd: Some("/work/api".into()),
            label: Some("api".into()),
            agent: Some("claude".into()),
            agent_name: Some("reviewer".into()),
            resume: vec!["claude".into(), "--resume".into(), "abc123".into()],
        }
    } else {
        plain_leaf(id)
    }
}

fn three_pane_session() -> Session {
    let layout = Node::Split {
        dir: Dir::H,
        ratio: 0.4,
        a: Box::new(Node::Leaf(1)),
        b: Box::new(Node::Split {
            dir: Dir::V,
            ratio: 0.5,
            a: Box::new(Node::Leaf(2)),
            b: Box::new(Node::Leaf(3)),
        }),
    };
    Session {
        name: "main".into(),
        windows: vec![Window {
            name: "sh".into(),
            manual_name: false,
            layout,
            active: 3,
            zoomed: true,
        }],
        active: 0,
        cols: 120,
        rows: 40,
        sidebar: true,
        cwd: Some("/work".into()),
        last_used: Instant::now(),
    }
}

fn sample() -> Snapshot {
    snapshot_of(&[three_pane_session()], &lookup)
}

fn set_first_window(snap: &mut Snapshot, f: impl FnOnce(&mut WindowSnap)) {
    f(&mut snap.sessions[0].windows[0]);
}

fn chain(splits: usize) -> LayoutSnap {
    let mut node = LayoutSnap::Leaf(LeafSnap::default());
    for _ in 0..splits {
        node = LayoutSnap::Split {
            dir: Dir::H,
            ratio: 0.5,
            a: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
            b: Box::new(node),
        };
    }
    node
}

/// Fresh directory for one test, so parallel tests never share files.
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tmuxlike-persist-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn json_roundtrip_of_three_pane_tree() {
    let snap = sample();
    let text = serialize(&snap).unwrap();
    let back = parse_snapshot(&text).unwrap();
    assert_eq!(back, snap);
    let w = &back.sessions[0].windows[0];
    assert_eq!(w.active, 2, "pane 3 is leaf index 2");
    assert!(w.zoomed);
    assert_eq!(
        back.sessions[0].windows[0].layout,
        snap.sessions[0].windows[0].layout
    );
}

#[test]
fn validate_rejects_bad_version() {
    let mut snap = sample();
    snap.version = 2;
    assert!(validate(&snap).is_err());
}

#[test]
fn validate_rejects_empty_windows() {
    let mut snap = sample();
    snap.sessions[0].windows.clear();
    assert!(validate(&snap).is_err());
}

#[test]
fn validate_rejects_out_of_range_indices() {
    let mut snap = sample();
    set_first_window(&mut snap, |w| w.active = 3);
    assert!(
        validate(&snap).is_err(),
        "3 leaves, index 3 is out of range"
    );

    let mut snap = sample();
    snap.sessions[0].active_window = 1;
    assert!(validate(&snap).is_err());
}

#[test]
fn validate_rejects_duplicate_and_empty_names() {
    let mut snap = sample();
    let dup = snap.sessions[0].clone();
    snap.sessions.push(dup);
    assert!(validate(&snap).is_err());

    let mut snap = sample();
    snap.sessions[0].name.clear();
    assert!(validate(&snap).is_err());
}

#[test]
fn validate_rejects_non_finite_ratio() {
    let mut snap = sample();
    set_first_window(&mut snap, |w| {
        w.layout = LayoutSnap::Split {
            dir: Dir::V,
            ratio: f32::NAN,
            a: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
            b: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
        };
    });
    assert!(validate(&snap).is_err());
}

#[test]
fn validate_limits_depth_to_64() {
    let ok = |n| {
        let mut snap = sample();
        set_first_window(&mut snap, |w| {
            w.layout = chain(n);
            w.active = 0;
        });
        validate(&snap)
    };
    assert!(ok(63).is_ok(), "64 levels");
    assert!(ok(64).is_err(), "65 levels");
}

#[test]
fn rebuild_assigns_leaf_ids_in_order() {
    let mut next = 9;
    let node = rebuild(
        &sample().sessions[0].windows[0].layout,
        &mut |_: &LeafSnap| {
            next += 1;
            Ok(next)
        },
    )
    .unwrap();
    let mut leaves = vec![];
    node.leaves(&mut leaves);
    assert_eq!(leaves, vec![10, 11, 12]);
}

#[test]
fn rebuild_aborts_on_first_closure_error() {
    let mut calls = 0;
    let layout = &sample().sessions[0].windows[0].layout;
    let result = rebuild(layout, &mut |_: &LeafSnap| {
        calls += 1;
        if calls == 2 {
            Err("boom".to_string())
        } else {
            Ok(calls)
        }
    });
    assert_eq!(result.unwrap_err(), "boom");
    assert_eq!(calls, 2, "no leaves after the failing one");
}

#[test]
fn rebuild_clamps_finite_ratios() {
    let split = |ratio: f32| LayoutSnap::Split {
        dir: Dir::H,
        ratio,
        a: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
        b: Box::new(LayoutSnap::Leaf(LeafSnap::default())),
    };
    let mut id = 0;
    let mut next = |_: &LeafSnap| {
        id += 1;
        Ok(id)
    };
    let high = rebuild(&split(2.0), &mut next).unwrap();
    let low = rebuild(&split(-1.0), &mut next).unwrap();
    assert!(matches!(high, Node::Split { ratio, .. } if ratio == 0.95));
    assert!(matches!(low, Node::Split { ratio, .. } if ratio == 0.05));
    assert!(rebuild(&split(f32::INFINITY), &mut next).is_err());
}

#[test]
fn sanitize_keeps_only_safe_chars() {
    assert_eq!(
        sanitize_file_part("tmuxlike-user.sock"),
        "tmuxlike-user.sock"
    );
    assert_eq!(sanitize_file_part("a b/c\\d:e"), "a_b_c_d_e");
    assert_eq!(sanitize_file_part("..\\x"), ".._x");
    assert_eq!(sanitize_file_part("ü"), "_");
}

#[test]
fn snapshot_maps_focus_and_skips_empty_sessions() {
    let mut empty = three_pane_session();
    empty.name = "empty".into();
    empty.windows.clear();
    let snap = snapshot_of(&[empty, three_pane_session()], &lookup);
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].windows[0].active, 2);
    assert!(validate(&snap).is_ok());
}

#[test]
fn parse_rejects_invalid_json() {
    assert!(parse_snapshot("{not json").is_err());
}

#[test]
fn write_atomic_replaces_existing_file() {
    let dir = scratch_dir("atomic");
    let path = dir.join("state.json");
    write_atomic(&path, "one").unwrap();
    write_atomic(&path, "two").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "two");
    assert!(!with_suffix(&path, ".tmp").exists());
    let _ = fs::remove_dir_all(&dir);
}

// M3: the state file is read as bytes; anything that is not a valid snapshot is an error.

#[test]
fn decode_rejects_non_utf8_bytes() {
    assert!(decode_snapshot(&[0xff, 0xfe, b'{']).is_err());
}

#[test]
fn decode_accepts_valid_snapshot_bytes() {
    let bytes = serialize(&sample()).unwrap().into_bytes();
    assert_eq!(decode_snapshot(&bytes).unwrap(), sample());
}

#[test]
fn decode_rejects_truncated_json_bytes() {
    assert!(decode_snapshot(b"{\"version\": 1, \"sessions\": [").is_err());
}

#[test]
fn quarantine_moves_file_to_bad_suffix() {
    let dir = scratch_dir("quarantine");
    let path = dir.join("state.json");
    fs::write(&path, b"broken").unwrap();
    let msg = quarantine(&path, "parse: bad");
    assert!(!path.exists(), "original moved away");
    assert_eq!(fs::read(with_suffix(&path, ".bad")).unwrap(), b"broken");
    assert!(msg.contains("parse: bad"));
    let _ = fs::remove_dir_all(&dir);
}

// M2: a partial restore keeps the original file as `<file>.partial.bad`.

#[test]
fn partial_restore_means_some_but_not_all_came_back() {
    assert!(is_partial(3, 1));
    assert!(is_partial(2, 1));
    assert!(!is_partial(3, 3), "full restore");
    assert!(!is_partial(3, 0), "zero restored is handled as a failure");
    assert!(!is_partial(0, 0), "nothing attempted");
}

#[test]
fn keep_partial_copy_preserves_original_bytes() {
    let dir = scratch_dir("partial");
    let path = dir.join("state.json");
    fs::write(&path, b"original snapshot").unwrap();
    keep_partial_copy(&path);
    let kept = with_suffix(&path, ".partial.bad");
    assert_eq!(fs::read(&kept).unwrap(), b"original snapshot");
    assert!(path.exists(), "the state file itself stays in place");
    let _ = fs::remove_dir_all(&dir);
}

// M1: restored agent names must be valid and unique.

#[test]
fn keep_agent_name_accepts_valid_unique_name() {
    let mut taken = HashSet::new();
    assert_eq!(
        keep_agent_name(Some("reviewer"), &mut taken).as_deref(),
        Some("reviewer")
    );
    assert!(taken.contains("reviewer"));
}

#[test]
fn keep_agent_name_drops_invalid_names() {
    let mut taken = HashSet::new();
    assert_eq!(keep_agent_name(None, &mut taken), None);
    assert_eq!(keep_agent_name(Some("p1"), &mut taken), None, "pane-shaped");
    assert_eq!(keep_agent_name(Some("%7"), &mut taken), None, "pane-shaped");
    assert_eq!(keep_agent_name(Some("Bad Name"), &mut taken), None);
    assert_eq!(keep_agent_name(Some(""), &mut taken), None);
    let too_long = "a".repeat(33);
    assert_eq!(keep_agent_name(Some(too_long.as_str()), &mut taken), None);
    assert!(taken.is_empty(), "rejected names claim nothing");
}

#[test]
fn keep_agent_name_drops_duplicates() {
    let mut taken = HashSet::new();
    assert!(keep_agent_name(Some("reviewer"), &mut taken).is_some());
    assert_eq!(keep_agent_name(Some("reviewer"), &mut taken), None);
    assert_eq!(taken.len(), 1);
}
