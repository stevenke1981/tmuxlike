//! Layout requests: split, focus, zoom, rename and close.
use super::{pane_reply, resolve_pane, unexpected};
use crate::api::{
    ApiError, ApiResult, Direction, LABEL_MAX, Reply, Request, ZoomMode, clean_text, lock,
    pane_id_str,
};
use crate::layout::{Dir, Move, neighbor};
use crate::server::{Shared, State, Window};
use serde_json::json;

pub(super) fn layout_op(shared: &Shared, req: Request) -> ApiResult {
    match req {
        Request::PaneSplit {
            pane,
            direction,
            ratio,
            cwd,
            focus,
        } => split(shared, pane, direction, ratio, cwd, focus),
        Request::PaneFocus { pane } => focus(shared, pane),
        Request::PaneFocusDirection { pane, direction } => focus_direction(shared, pane, direction),
        Request::PaneZoom { pane, mode } => zoom(shared, pane, mode),
        Request::PaneRename { pane, label } => rename(shared, pane, label),
        Request::PaneClose { pane } => close(shared, pane),
        other => Err(unexpected(&other)),
    }
}

/// herdr splits go right or down only; the new pane is always the second child.
fn split_dir(direction: Direction) -> Result<Dir, ApiError> {
    match direction {
        Direction::Right => Ok(Dir::H),
        Direction::Down => Ok(Dir::V),
        Direction::Left | Direction::Up => {
            Err(ApiError::invalid("split direction must be right or down"))
        }
    }
}

/// herdr `--ratio` is the share of the NEW pane; `split_pane` takes the share of the existing one.
fn existing_share(ratio: Option<f32>) -> Result<Option<f32>, ApiError> {
    match ratio {
        None => Ok(None),
        Some(r) if r > 0.0 && r < 1.0 => Ok(Some(1.0 - r)),
        Some(r) => Err(ApiError::invalid(format!(
            "ratio must be between 0 and 1 exclusive, got {r}"
        ))),
    }
}

fn split(
    shared: &Shared,
    pane: Option<usize>,
    direction: Direction,
    ratio: Option<f32>,
    cwd: Option<String>,
    focus: bool,
) -> ApiResult {
    let dir = split_dir(direction)?;
    let share = existing_share(ratio)?;
    let mut st = lock(shared);
    let pid = resolve_pane(&st, pane)?;
    let new_id = st
        .split_pane(shared, pid, dir, share, cwd, focus)
        .map_err(|e| ApiError::new("spawn_failed", format!("{e:#}")))?;
    st.mark_dirty();
    pane_reply(&st, new_id)
}

fn focus(shared: &Shared, pane: usize) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    st.focus_pane(pid);
    st.mark_dirty();
    pane_reply(&st, pid)
}

fn move_of(direction: Direction) -> Move {
    match direction {
        Direction::Left => Move::Left,
        Direction::Right => Move::Right,
        Direction::Up => Move::Up,
        Direction::Down => Move::Down,
    }
}

fn neighbor_pane(st: &State, pid: usize, mv: Move) -> Result<usize, ApiError> {
    let (si, wi) = st
        .locate(pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    let s = &st.sessions[si];
    let rects = st.visible_rects(s, &s.windows[wi]);
    neighbor(&rects, pid, mv).ok_or_else(|| {
        ApiError::new(
            "no_neighbor",
            format!("no pane {mv:?} of {}", pane_id_str(pid)),
        )
    })
}

fn focus_direction(shared: &Shared, pane: Option<usize>, direction: Direction) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, pane)?;
    let target = neighbor_pane(&st, pid, move_of(direction))?;
    st.focus_pane(target);
    st.mark_dirty();
    pane_reply(&st, target)
}

/// Apply a zoom request to one window. Zooming a single-pane window is a no-op flag-wise.
fn apply_zoom(w: &mut Window, pid: usize, mode: ZoomMode) {
    let on = match mode {
        ZoomMode::On => true,
        ZoomMode::Off => false,
        ZoomMode::Toggle => !w.zoomed,
    };
    if !on {
        w.zoomed = false;
        return;
    }
    w.active = pid;
    w.zoomed = w.panes().len() > 1;
}

fn zoom(shared: &Shared, pane: Option<usize>, mode: ZoomMode) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, pane)?;
    let (si, wi) = st
        .locate(pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    apply_zoom(&mut st.sessions[si].windows[wi], pid, mode);
    st.relayout(si);
    st.mark_dirty();
    pane_reply(&st, pid)
}

/// Pane label: control characters stripped, trimmed and capped; empty means no label.
fn normalize_label(label: Option<&str>) -> Option<String> {
    label.and_then(|s| clean_text(s, LABEL_MAX))
}

fn rename(shared: &Shared, pane: usize, label: Option<String>) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    let p = st
        .panes
        .get_mut(&pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    p.label = normalize_label(label.as_deref());
    st.relayout_pane(pid);
    pane_reply(&st, pid)
}

fn close(shared: &Shared, pane: usize) -> ApiResult {
    let mut st = lock(shared);
    let pid = resolve_pane(&st, Some(pane))?;
    st.kill_pane(pid);
    st.mark_dirty();
    Ok(Reply::Json(json!({ "closed": pane_id_str(pid) })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Node;

    #[test]
    fn split_direction_and_ratio_mapping() {
        assert_eq!(split_dir(Direction::Right).unwrap(), Dir::H);
        assert_eq!(split_dir(Direction::Down).unwrap(), Dir::V);
        assert_eq!(
            split_dir(Direction::Left).unwrap_err().code,
            "invalid_argument"
        );
        assert_eq!(existing_share(None).unwrap(), None);
        assert_eq!(existing_share(Some(0.25)).unwrap(), Some(0.75));
        for bad in [0.0, 1.0, -0.5, 1.5, f32::NAN] {
            let err = existing_share(Some(bad)).unwrap_err();
            assert_eq!(err.code, "invalid_argument");
        }
    }

    #[test]
    fn zoom_on_toggle_off() {
        let mut w = Window {
            name: String::new(),
            manual_name: false,
            layout: Node::Split {
                dir: Dir::H,
                ratio: 0.5,
                a: Box::new(Node::Leaf(1)),
                b: Box::new(Node::Leaf(2)),
            },
            active: 1,
            zoomed: false,
        };
        apply_zoom(&mut w, 2, ZoomMode::On);
        assert!(w.zoomed);
        assert_eq!(w.active, 2);
        apply_zoom(&mut w, 2, ZoomMode::Toggle);
        assert!(!w.zoomed);
        apply_zoom(&mut w, 1, ZoomMode::On);
        apply_zoom(&mut w, 1, ZoomMode::Off);
        assert!(!w.zoomed);
    }

    #[test]
    fn zoom_single_pane_window_stays_unzoomed() {
        let mut w = Window {
            name: String::new(),
            manual_name: false,
            layout: Node::Leaf(7),
            active: 7,
            zoomed: false,
        };
        apply_zoom(&mut w, 7, ZoomMode::On);
        assert!(!w.zoomed);
    }

    #[test]
    fn label_is_cleaned_capped_and_empty_clears() {
        assert_eq!(normalize_label(Some("  api  ")), Some("api".into()));
        assert_eq!(normalize_label(Some("a\u{1b}b\n")), Some("ab".into()));
        assert_eq!(normalize_label(Some("   ")), None);
        assert_eq!(normalize_label(None), None);
        let long = "x".repeat(200);
        assert_eq!(
            normalize_label(Some(&long)).unwrap().chars().count(),
            LABEL_MAX
        );
    }
}
