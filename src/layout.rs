//! Binary split layout tree for panes.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Dir {
    /// side by side (left | right)
    H,
    /// stacked (top / bottom)
    V,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub enum Node {
    Leaf(usize),
    Split {
        dir: Dir,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

impl Node {
    pub fn layout(&self, r: Rect, out: &mut Vec<(usize, Rect)>) {
        match self {
            Node::Leaf(id) => out.push((*id, r)),
            Node::Split { dir, ratio, a, b } => {
                let total = if *dir == Dir::H { r.w } else { r.h };
                if total < 3 {
                    a.layout(r, out);
                    b.layout(Rect { w: 0, h: 0, ..r }, out);
                    return;
                }
                let avail = total - 1;
                let first = ((avail as f32 * ratio).round() as u16).clamp(1, avail - 1);
                let second = avail - first;
                if *dir == Dir::H {
                    a.layout(Rect { w: first, ..r }, out);
                    b.layout(
                        Rect {
                            x: r.x + first + 1,
                            w: second,
                            ..r
                        },
                        out,
                    );
                } else {
                    a.layout(Rect { h: first, ..r }, out);
                    b.layout(
                        Rect {
                            y: r.y + first + 1,
                            h: second,
                            ..r
                        },
                        out,
                    );
                }
            }
        }
    }

    pub fn contains(&self, id: usize) -> bool {
        match self {
            Node::Leaf(i) => *i == id,
            Node::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    pub fn leaves(&self, out: &mut Vec<usize>) {
        match self {
            Node::Leaf(i) => out.push(*i),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }

    /// Split `target`; `ratio` is the share of the first (existing) pane.
    pub fn split_with(&mut self, target: usize, new: usize, dir: Dir, ratio: f32) -> bool {
        match self {
            Node::Leaf(i) if *i == target => {
                *self = Node::Split {
                    dir,
                    ratio,
                    a: Box::new(Node::Leaf(target)),
                    b: Box::new(Node::Leaf(new)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => {
                a.split_with(target, new, dir, ratio) || b.split_with(target, new, dir, ratio)
            }
        }
    }

    /// Remove a leaf. Returns true if removed. The root being the leaf itself is not handled here.
    pub fn remove(&mut self, target: usize) -> bool {
        if let Node::Split { a, b, .. } = self {
            if matches!(**a, Node::Leaf(i) if i == target) {
                let rest = std::mem::replace(&mut **b, Node::Leaf(usize::MAX));
                *self = rest;
                return true;
            }
            if matches!(**b, Node::Leaf(i) if i == target) {
                let rest = std::mem::replace(&mut **a, Node::Leaf(usize::MAX));
                *self = rest;
                return true;
            }
            return a.remove(target) || b.remove(target);
        }
        false
    }

    /// Move the border of the nearest enclosing split along `dir` by `delta` (fraction; + = right/down).
    pub fn resize(&mut self, target: usize, dir: Dir, delta: f32) -> bool {
        if let Node::Split {
            dir: d,
            ratio,
            a,
            b,
        } = self
        {
            let in_a = a.contains(target);
            let in_b = !in_a && b.contains(target);
            if !in_a && !in_b {
                return false;
            }
            let child = if in_a { a } else { b };
            if child.resize(target, dir, delta) {
                return true;
            }
            if *d == dir {
                *ratio = (*ratio + delta).clamp(0.05, 0.95);
                return true;
            }
        }
        false
    }
}

/// Find the neighbouring pane in a direction using geometry.
pub fn neighbor(rects: &[(usize, Rect)], from: usize, mv: Move) -> Option<usize> {
    let cur = rects.iter().find(|(i, _)| *i == from)?.1;
    let overlap = |a0: u16, a1: u16, b0: u16, b1: u16| a1.min(b1) as i32 - a0.max(b0) as i32;
    rects
        .iter()
        .filter(|(i, r)| *i != from && r.w > 0 && r.h > 0)
        .filter_map(|(i, r)| {
            let (dist, ov) = match mv {
                Move::Left if r.x + r.w <= cur.x => (
                    cur.x - (r.x + r.w),
                    overlap(r.y, r.y + r.h, cur.y, cur.y + cur.h),
                ),
                Move::Right if r.x >= cur.x + cur.w => (
                    r.x - (cur.x + cur.w),
                    overlap(r.y, r.y + r.h, cur.y, cur.y + cur.h),
                ),
                Move::Up if r.y + r.h <= cur.y => (
                    cur.y - (r.y + r.h),
                    overlap(r.x, r.x + r.w, cur.x, cur.x + cur.w),
                ),
                Move::Down if r.y >= cur.y + cur.h => (
                    r.y - (cur.y + cur.h),
                    overlap(r.x, r.x + r.w, cur.x, cur.x + cur.w),
                ),
                _ => return None,
            };
            Some((*i, dist as i32, ov))
        })
        .min_by_key(|(_, d, ov)| (*d, if *ov > 0 { 0 } else { 1 }, -ov))
        .map(|(i, _, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    const R: Rect = Rect {
        x: 0,
        y: 0,
        w: 81,
        h: 24,
    };

    #[test]
    fn split_and_layout() {
        let mut n = Node::Leaf(1);
        assert!(n.split_with(1, 2, Dir::H, 0.5));
        let mut out = vec![];
        n.layout(R, &mut out);
        assert_eq!(
            out[0],
            (
                1,
                Rect {
                    x: 0,
                    y: 0,
                    w: 40,
                    h: 24
                }
            )
        );
        assert_eq!(
            out[1],
            (
                2,
                Rect {
                    x: 41,
                    y: 0,
                    w: 40,
                    h: 24
                }
            )
        );
        assert!(n.split_with(2, 3, Dir::V, 0.5));
        let mut out = vec![];
        n.layout(R, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].1.y, 13);
        assert_eq!(neighbor(&out, 1, Move::Right), Some(2));
        assert_eq!(neighbor(&out, 2, Move::Down), Some(3));
        assert_eq!(neighbor(&out, 3, Move::Left), Some(1));
        assert_eq!(neighbor(&out, 1, Move::Left), None);
    }

    #[test]
    fn remove_and_resize() {
        let mut n = Node::Leaf(1);
        n.split_with(1, 2, Dir::H, 0.5);
        n.split_with(2, 3, Dir::V, 0.5);
        assert!(n.resize(1, Dir::H, 0.1));
        let mut out = vec![];
        n.layout(R, &mut out);
        assert_eq!(out[0].1.w, 48);
        assert!(n.remove(2));
        let mut l = vec![];
        n.leaves(&mut l);
        assert_eq!(l, vec![1, 3]);
        assert!(n.remove(3));
        assert!(matches!(n, Node::Leaf(1)));
    }
}
