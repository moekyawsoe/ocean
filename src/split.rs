use alacritty_terminal::index::Side;

use crate::config::Config;
use crate::config::keybind::{SplitDirection, SplitFocusTarget};
use crate::font::FontSystem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    /// Side-by-side panes (`Left | Right`), separated by a vertical divider line.
    Horizontal,
    /// Stacked panes (`Top / Bottom`), separated by a horizontal divider line.
    Vertical,
}

#[derive(Debug, Clone)]
pub enum SplitNode {
    Leaf(u64),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

impl SplitNode {
    pub fn first_leaf(&self) -> u64 {
        match self {
            Self::Leaf(id) => *id,
            Self::Split { first, .. } => first.first_leaf(),
        }
    }

    pub fn last_leaf(&self) -> u64 {
        match self {
            Self::Leaf(id) => *id,
            Self::Split { second, .. } => second.last_leaf(),
        }
    }

    #[cfg(test)]
    pub fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    pub fn contains_leaf(&self, target_id: u64) -> bool {
        match self {
            Self::Leaf(id) => *id == target_id,
            Self::Split { first, second, .. } => {
                first.contains_leaf(target_id) || second.contains_leaf(target_id)
            }
        }
    }

    pub fn split_leaf(
        &mut self,
        target_id: u64,
        new_id: u64,
        direction: SplitDirection,
    ) -> bool {
        let (axis, put_second) = match direction {
            SplitDirection::Right | SplitDirection::Auto => (SplitAxis::Horizontal, true),
            SplitDirection::Left => (SplitAxis::Horizontal, false),
            SplitDirection::Down => (SplitAxis::Vertical, true),
            SplitDirection::Up => (SplitAxis::Vertical, false),
        };
        self.split_leaf_with_axis(target_id, new_id, axis, put_second)
    }

    fn split_leaf_with_axis(
        &mut self,
        target_id: u64,
        new_id: u64,
        axis: SplitAxis,
        put_second: bool,
    ) -> bool {
        match self {
            Self::Leaf(id) if *id == target_id => {
                let old_leaf = Box::new(Self::Leaf(target_id));
                let new_leaf = Box::new(Self::Leaf(new_id));
                let (first, second) = if put_second {
                    (old_leaf, new_leaf)
                } else {
                    (new_leaf, old_leaf)
                };
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first,
                    second,
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf_with_axis(target_id, new_id, axis, put_second)
                    || second.split_leaf_with_axis(target_id, new_id, axis, put_second)
            }
        }
    }

    /// Remove `target_id` from the tree, returning `(remaining_tree, suggested_focus_leaf)`.
    pub fn remove_leaf(self, target_id: u64) -> (Option<SplitNode>, Option<u64>) {
        match self {
            Self::Leaf(id) => {
                if id == target_id {
                    (None, None)
                } else {
                    (Some(Self::Leaf(id)), None)
                }
            }
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                if first.contains_leaf(target_id) {
                    let (rem_first, focus) = first.remove_leaf(target_id);
                    match rem_first {
                        Some(f) => (
                            Some(Self::Split {
                                axis,
                                ratio,
                                first: Box::new(f),
                                second,
                            }),
                            focus,
                        ),
                        None => {
                            let next_focus = second.first_leaf();
                            (Some(*second), Some(next_focus))
                        }
                    }
                } else if second.contains_leaf(target_id) {
                    let (rem_second, focus) = second.remove_leaf(target_id);
                    match rem_second {
                        Some(s) => (
                            Some(Self::Split {
                                axis,
                                ratio,
                                first,
                                second: Box::new(s),
                            }),
                            focus,
                        ),
                        None => {
                            let next_focus = first.last_leaf();
                            (Some(*first), Some(next_focus))
                        }
                    }
                } else {
                    (
                        Some(Self::Split {
                            axis,
                            ratio,
                            first,
                            second,
                        }),
                        None,
                    )
                }
            }
        }
    }

    pub fn set_ratio_at_path(&mut self, path: &[bool], new_ratio: f32) {
        let clamped = new_ratio.clamp(0.12, 0.88);
        match (self, path.split_first()) {
            (Self::Split { ratio, .. }, None) => {
                *ratio = clamped;
            }
            (Self::Split { first, second, .. }, Some((&go_second, rest))) => {
                if go_second {
                    second.set_ratio_at_path(rest, clamped);
                } else {
                    first.set_ratio_at_path(rest, clamped);
                }
            }
            _ => {}
        }
    }

    pub fn equalize(&mut self) {
        if let Self::Split {
            ratio,
            first,
            second,
            ..
        } = self
        {
            *ratio = 0.5;
            first.equalize();
            second.equalize();
        }
    }

    /// Resize the innermost matching-axis split ancestor containing `target_id`.
    pub fn resize_for_leaf(
        &mut self,
        target_id: u64,
        direction: SplitDirection,
        delta: f32,
    ) -> bool {
        let step = delta.abs().clamp(0.01, 0.40);
        let (target_axis, signed_delta) = match direction {
            SplitDirection::Left => (SplitAxis::Horizontal, -step),
            SplitDirection::Right | SplitDirection::Auto => (SplitAxis::Horizontal, step),
            SplitDirection::Up => (SplitAxis::Vertical, -step),
            SplitDirection::Down => (SplitAxis::Vertical, step),
        };
        self.resize_axis_innermost(target_id, target_axis, signed_delta)
    }

    fn resize_axis_innermost(
        &mut self,
        target_id: u64,
        target_axis: SplitAxis,
        signed_delta: f32,
    ) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let in_first = first.contains_leaf(target_id);
                let in_second = !in_first && second.contains_leaf(target_id);
                if !in_first && !in_second {
                    return false;
                }
                let handled_below = if in_first {
                    first.resize_axis_innermost(target_id, target_axis, signed_delta)
                } else {
                    second.resize_axis_innermost(target_id, target_axis, signed_delta)
                };
                if handled_below {
                    return true;
                }
                if *axis == target_axis {
                    let next = (*ratio + signed_delta).clamp(0.12, 0.88);
                    if (next - *ratio).abs() > 0.0005 {
                        *ratio = next;
                        return true;
                    }
                }
                false
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PaneLayout {
    pub pane_id: u64,
    pub bounds_x: f32,
    pub bounds_y: f32,
    pub bounds_w: f32,
    pub bounds_h: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub cols: u16,
    pub rows: u16,
}

impl PaneLayout {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.bounds_x
            && px <= self.bounds_x + self.bounds_w
            && py >= self.bounds_y
            && py <= self.bounds_y + self.bounds_h
    }

    pub fn distance_sq_to(&self, px: f32, py: f32) -> f32 {
        let cx = px.clamp(self.bounds_x, self.bounds_x + self.bounds_w);
        let cy = py.clamp(self.bounds_y, self.bounds_y + self.bounds_h);
        let dx = px - cx;
        let dy = py - cy;
        dx * dx + dy * dy
    }

    pub fn pixel_to_cell(&self, px: f32, py: f32, cell_w: f32, cell_h: f32) -> (usize, usize, Side) {
        let cw = cell_w.max(1.0);
        let ch = cell_h.max(1.0);
        let rx = (px - self.origin_x).max(0.0);
        let ry = (py - self.origin_y).max(0.0);
        let col_f = rx / cw;
        let col = (col_f.floor() as usize).min(self.cols.saturating_sub(1) as usize);
        let row = ((ry / ch).floor() as usize).min(self.rows.saturating_sub(1) as usize);
        let side = if col_f.fract() > 0.5 {
            Side::Right
        } else {
            Side::Left
        };
        (col, row, side)
    }
}

#[derive(Debug, Clone)]
pub struct SplitDivider {
    pub axis: SplitAxis,
    pub path: Vec<bool>,
    pub container_start: f32,
    pub container_span: f32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl SplitDivider {
    pub fn hit_test(&self, px: f32, py: f32, slop: f32) -> bool {
        match self.axis {
            SplitAxis::Horizontal => {
                px >= self.x - slop
                    && px <= self.x + self.w + slop
                    && py >= self.y
                    && py <= self.y + self.h
            }
            SplitAxis::Vertical => {
                py >= self.y - slop
                    && py <= self.y + self.h + slop
                    && px >= self.x
                    && px <= self.x + self.w
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn compute_split_layout(
    root: &SplitNode,
    config: &Config,
    font_sys: &FontSystem,
    win_w: u32,
    win_h: u32,
    scale_factor: f64,
    tab_bar_h: f32,
    status_bar_h: f32,
) -> (Vec<PaneLayout>, Vec<SplitDivider>) {
    let sf = scale_factor as f32;
    let pad_l = config.window_padding_x.0 as f32 * sf;
    let pad_r = config.window_padding_x.1 as f32 * sf;
    let pad_t = config.window_padding_y.0 as f32 * sf + tab_bar_h;
    let pad_b = config.window_padding_y.1 as f32 * sf + status_bar_h;

    let cw = font_sys.metrics.cell_width.max(1.0);
    let ch = font_sys.metrics.cell_height.max(1.0);

    let avail_w = (win_w as f32 - pad_l - pad_r).max(cw * 2.0);
    let avail_h = (win_h as f32 - pad_t - pad_b).max(ch);

    if let SplitNode::Leaf(pane_id) = root {
        let cols = (avail_w / cw).floor().clamp(2.0, 1000.0) as u16;
        let rows = (avail_h / ch).floor().clamp(1.0, 500.0) as u16;
        let (ox, oy) = if config.window_padding_balance {
            let rem_w = (avail_w - cols as f32 * cw).max(0.0);
            let rem_h = (avail_h - rows as f32 * ch).max(0.0);
            ((pad_l + rem_w * 0.5).floor(), (pad_t + rem_h * 0.5).floor())
        } else {
            (pad_l.floor(), pad_t.floor())
        };
        return (
            vec![PaneLayout {
                pane_id: *pane_id,
                bounds_x: 0.0,
                bounds_y: tab_bar_h,
                bounds_w: win_w as f32,
                bounds_h: (win_h as f32 - tab_bar_h - status_bar_h).max(1.0),
                origin_x: ox,
                origin_y: oy,
                cols,
                rows,
            }],
            Vec::new(),
        );
    }

    let mut panes = Vec::new();
    let mut dividers = Vec::new();
    let mut path = Vec::new();
    let gap = (10.0 * sf).round().max(4.0);
    let line_thick = (1.0 * sf).round().max(1.0);

    #[allow(clippy::too_many_arguments)]
    fn walk(
        node: &SplitNode,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        cw: f32,
        ch: f32,
        gap: f32,
        line_thick: f32,
        balance: bool,
        path: &mut Vec<bool>,
        panes: &mut Vec<PaneLayout>,
        dividers: &mut Vec<SplitDivider>,
    ) {
        match node {
            SplitNode::Leaf(pane_id) => {
                let cols = (w / cw).floor().clamp(2.0, 1000.0) as u16;
                let rows = (h / ch).floor().clamp(1.0, 500.0) as u16;
                let (ox, oy) = if balance {
                    let rem_w = (w - cols as f32 * cw).max(0.0);
                    let rem_h = (h - rows as f32 * ch).max(0.0);
                    ((x + rem_w * 0.5).floor(), (y + rem_h * 0.5).floor())
                } else {
                    (x.floor(), y.floor())
                };
                panes.push(PaneLayout {
                    pane_id: *pane_id,
                    bounds_x: x,
                    bounds_y: y,
                    bounds_w: w.max(1.0),
                    bounds_h: h.max(1.0),
                    origin_x: ox,
                    origin_y: oy,
                    cols,
                    rows,
                });
            }
            SplitNode::Split {
                axis,
                ratio,
                first,
                second,
            } => match axis {
                SplitAxis::Horizontal => {
                    let split_x = (x + w * ratio).round();
                    let w1 = (split_x - x - gap * 0.5).max(cw * 2.0);
                    let x2 = split_x + gap * 0.5;
                    let w2 = (x + w - x2).max(cw * 2.0);

                    dividers.push(SplitDivider {
                        axis: SplitAxis::Horizontal,
                        path: path.clone(),
                        container_start: x,
                        container_span: w.max(1.0),
                        x: (split_x - line_thick * 0.5).floor(),
                        y,
                        w: line_thick,
                        h,
                    });

                    path.push(false);
                    walk(
                        first, x, y, w1, h, cw, ch, gap, line_thick, balance, path, panes, dividers,
                    );
                    path.pop();

                    path.push(true);
                    walk(
                        second, x2, y, w2, h, cw, ch, gap, line_thick, balance, path, panes,
                        dividers,
                    );
                    path.pop();
                }
                SplitAxis::Vertical => {
                    let split_y = (y + h * ratio).round();
                    let h1 = (split_y - y - gap * 0.5).max(ch);
                    let y2 = split_y + gap * 0.5;
                    let h2 = (y + h - y2).max(ch);

                    dividers.push(SplitDivider {
                        axis: SplitAxis::Vertical,
                        path: path.clone(),
                        container_start: y,
                        container_span: h.max(1.0),
                        x,
                        y: (split_y - line_thick * 0.5).floor(),
                        w,
                        h: line_thick,
                    });

                    path.push(false);
                    walk(
                        first, x, y, w, h1, cw, ch, gap, line_thick, balance, path, panes, dividers,
                    );
                    path.pop();

                    path.push(true);
                    walk(
                        second, x, y2, w, h2, cw, ch, gap, line_thick, balance, path, panes,
                        dividers,
                    );
                    path.pop();
                }
            },
        }
    }

    walk(
        root,
        pad_l,
        pad_t,
        avail_w,
        avail_h,
        cw,
        ch,
        gap,
        line_thick,
        config.window_padding_balance,
        &mut path,
        &mut panes,
        &mut dividers,
    );

    (panes, dividers)
}

pub fn pane_at_point(layouts: &[PaneLayout], px: f32, py: f32) -> Option<&PaneLayout> {
    if let Some(exact) = layouts.iter().find(|l| l.contains(px, py)) {
        return Some(exact);
    }
    layouts
        .iter()
        .min_by(|a, b| a.distance_sq_to(px, py).total_cmp(&b.distance_sq_to(px, py)))
}

pub fn find_neighbor_pane(
    layouts: &[PaneLayout],
    active_id: u64,
    target: SplitFocusTarget,
) -> Option<u64> {
    if layouts.len() <= 1 {
        return None;
    }
    let cur_idx = layouts.iter().position(|l| l.pane_id == active_id)?;
    match target {
        SplitFocusTarget::Next => {
            return Some(layouts[(cur_idx + 1) % layouts.len()].pane_id);
        }
        SplitFocusTarget::Previous => {
            return Some(layouts[(cur_idx + layouts.len() - 1) % layouts.len()].pane_id);
        }
        _ => {}
    }

    let cur = &layouts[cur_idx];
    let cur_cx = cur.bounds_x + cur.bounds_w * 0.5;
    let cur_cy = cur.bounds_y + cur.bounds_h * 0.5;

    let mut best: Option<(f32, u64)> = None;
    for cand in layouts {
        if cand.pane_id == active_id {
            continue;
        }
        let cand_cx = cand.bounds_x + cand.bounds_w * 0.5;
        let cand_cy = cand.bounds_y + cand.bounds_h * 0.5;

        let score = match target {
            SplitFocusTarget::Left if cand_cx < cur.bounds_x => {
                let dx = cur.bounds_x - (cand.bounds_x + cand.bounds_w);
                let overlap_y = overlap_len(
                    cur.bounds_y,
                    cur.bounds_y + cur.bounds_h,
                    cand.bounds_y,
                    cand.bounds_y + cand.bounds_h,
                );
                let dy_penalty = if overlap_y > 0.0 {
                    0.0
                } else {
                    (cur_cy - cand_cy).abs() * 4.0
                };
                Some(dx.max(0.0) + dy_penalty)
            }
            SplitFocusTarget::Right if cand_cx > cur.bounds_x + cur.bounds_w => {
                let dx = cand.bounds_x - (cur.bounds_x + cur.bounds_w);
                let overlap_y = overlap_len(
                    cur.bounds_y,
                    cur.bounds_y + cur.bounds_h,
                    cand.bounds_y,
                    cand.bounds_y + cand.bounds_h,
                );
                let dy_penalty = if overlap_y > 0.0 {
                    0.0
                } else {
                    (cur_cy - cand_cy).abs() * 4.0
                };
                Some(dx.max(0.0) + dy_penalty)
            }
            SplitFocusTarget::Up if cand_cy < cur.bounds_y => {
                let dy = cur.bounds_y - (cand.bounds_y + cand.bounds_h);
                let overlap_x = overlap_len(
                    cur.bounds_x,
                    cur.bounds_x + cur.bounds_w,
                    cand.bounds_x,
                    cand.bounds_x + cand.bounds_w,
                );
                let dx_penalty = if overlap_x > 0.0 {
                    0.0
                } else {
                    (cur_cx - cand_cx).abs() * 4.0
                };
                Some(dy.max(0.0) + dx_penalty)
            }
            SplitFocusTarget::Down if cand_cy > cur.bounds_y + cur.bounds_h => {
                let dy = cand.bounds_y - (cur.bounds_y + cur.bounds_h);
                let overlap_x = overlap_len(
                    cur.bounds_x,
                    cur.bounds_x + cur.bounds_w,
                    cand.bounds_x,
                    cand.bounds_x + cand.bounds_w,
                );
                let dx_penalty = if overlap_x > 0.0 {
                    0.0
                } else {
                    (cur_cx - cand_cx).abs() * 4.0
                };
                Some(dy.max(0.0) + dx_penalty)
            }
            _ => None,
        };

        if let Some(s) = score
            && best.is_none_or(|(best_s, _)| s < best_s)
        {
            best = Some((s, cand.pane_id));
        }
    }

    best.map(|(_, id)| id)
}

fn overlap_len(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_remove_leaves_preserve_tree_integrity() {
        let mut tree = SplitNode::Leaf(1);
        assert!(tree.split_leaf(1, 2, SplitDirection::Right));
        assert_eq!(tree.leaf_count(), 2);
        assert!(tree.split_leaf(2, 3, SplitDirection::Down));
        assert_eq!(tree.leaf_count(), 3);

        let (rem, focus) = tree.remove_leaf(2);
        let tree = rem.expect("tree still has panes 1 and 3");
        assert_eq!(tree.leaf_count(), 2);
        assert_eq!(focus, Some(3));

        let (rem, focus) = tree.remove_leaf(3);
        let tree = rem.expect("tree still has pane 1");
        assert_eq!(tree.leaf_count(), 1);
        assert_eq!(focus, Some(1));

        let (rem, _) = tree.remove_leaf(1);
        assert!(rem.is_none());
    }

    #[test]
    fn keyboard_resize_split_adjusts_innermost_matching_axis() {
        let mut tree = SplitNode::Leaf(1);
        assert!(tree.split_leaf(1, 2, SplitDirection::Right));
        assert!(tree.split_leaf(2, 3, SplitDirection::Down));

        assert!(tree.resize_for_leaf(3, SplitDirection::Up, 0.06));
        assert!(tree.resize_for_leaf(3, SplitDirection::Left, 0.08));

        if let SplitNode::Split { ratio, second, .. } = &tree {
            assert!((*ratio - 0.42).abs() < 1e-4);
            if let SplitNode::Split { ratio: v_ratio, .. } = second.as_ref() {
                assert!((*v_ratio - 0.44).abs() < 1e-4);
            } else {
                panic!("expected nested vertical split");
            }
        } else {
            panic!("expected root horizontal split");
        }
    }
}
