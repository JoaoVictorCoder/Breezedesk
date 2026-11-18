//! Snapping while a widget is dragged (platform independent, so it can be tested).
//!
//! The widget sticks to the edges of the screen and to *neighbouring* widgets:
//! side by side, stacked, or with aligned edges. Widgets far away on the other
//! axis are ignored, so a widget never gets "locked" to an edge it isn't near.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// Distance between two ranges (0 when they overlap).
fn gap(a0: i32, a1: i32, b0: i32, b1: i32) -> i32 {
    (b0 - a1).max(a0 - b1).max(0)
}

/// Position (x, y) for a widget the user is dragging to `r`.
/// `d` is the snap distance and `space` the gap kept between widgets, in pixels.
pub fn snap(r: Rect, work: Rect, others: &[Rect], d: i32, space: i32) -> (i32, i32) {
    let mut xs = vec![work.x, work.right() - r.w];
    let mut ys = vec![work.y, work.bottom() - r.h];
    for o in others {
        let h_gap = gap(r.x, r.right(), o.x, o.right());
        let v_gap = gap(r.y, r.bottom(), o.y, o.bottom());
        if v_gap <= d {
            // Beside each other: touch left/right edges...
            xs.extend([o.right() + space, o.x - r.w - space]);
            // ...and line up tops/bottoms when they are close.
            if h_gap <= 3 * d {
                ys.extend([o.y, o.bottom() - r.h]);
            }
        }
        if h_gap <= d {
            // Stacked: touch top/bottom edges, and line up left/right edges.
            ys.extend([o.bottom() + space, o.y - r.h - space]);
            if v_gap <= 3 * d {
                xs.extend([o.x, o.right() - r.w]);
            }
        }
    }
    let nearest = |cur: i32, list: &[i32]| {
        list.iter().copied().filter(|v| (v - cur).abs() <= d).min_by_key(|v| (v - cur).abs()).unwrap_or(cur)
    };
    (nearest(r.x, &xs), nearest(r.y, &ys))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 };

    fn r(x: i32, y: i32) -> Rect {
        Rect { x, y, w: 200, h: 100 }
    }

    #[test]
    fn free_and_screen_edges() {
        assert_eq!(snap(r(500, 400), WORK, &[], 12, 4), (500, 400));
        assert_eq!(snap(r(7, 400), WORK, &[], 12, 4), (0, 400));
        assert_eq!(snap(r(1715, 935), WORK, &[], 12, 4), (1720, 940));
    }

    #[test]
    fn far_widgets_do_not_lock_an_axis() {
        // Another widget whose left edge lines up, but 500 px lower: no snap.
        let other = Rect { x: 600, y: 900, w: 200, h: 100 };
        assert_eq!(snap(r(605, 200), WORK, &[other], 12, 4), (605, 200));
    }

    #[test]
    fn neighbours() {
        let other = Rect { x: 600, y: 300, w: 200, h: 100 };
        // Beside it, slightly off: snaps next to it and lines up the tops.
        assert_eq!(snap(r(808, 306), WORK, &[other], 12, 4), (804, 300));
        // Below it: stacks under it and lines up the left edges.
        assert_eq!(snap(r(595, 410), WORK, &[other], 12, 4), (600, 404));
    }

    #[test]
    fn dragging_never_gets_stuck() {
        // Moving 1 px at a time past a neighbour: the widget always follows
        // the mouse (within the snap distance) and ends exactly where aimed.
        let other = Rect { x: 600, y: 300, w: 200, h: 100 };
        for x in 700..=1000 {
            let (sx, sy) = snap(r(x, 320), WORK, &[other], 12, 4);
            assert!((sx - x).abs() <= 12, "x {x} -> {sx}");
            assert!((sy - 320).abs() <= 12);
        }
        assert_eq!(snap(r(1000, 520), WORK, &[other], 12, 4), (1000, 520));
    }
}
