//! Window layout: which window goes where on the output.
//!
//! This module is deliberately pure. It knows about `WindowId`s and
//! rectangles and nothing else — no Wayland objects, no compositor state — so
//! the geometry can be unit-tested without a running server, the same way
//! [`crate::focus`] tests the focus stack.
//!
//! ## Two orders, not one
//!
//! [`crate::focus::FocusStack`] is a most-recently-used order: it answers
//! "who should get focus now that the focused window is gone". That is the
//! wrong order for tiling, where the arrangement has to stay put instead of
//! reshuffling every time the user glances at a window. So the `Tiler` keeps
//! its own stable insertion order, and the two are consulted for different
//! questions.

use smithay::utils::{Logical, Rectangle};

use crate::focus::WindowId;

/// How the mapped windows are arranged on the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// Side by side, in stable order, each column the full height.
    #[default]
    Columns,
    /// Every window gets the whole output; only the focused one is visible.
    Monocle,
}

/// A spatial direction for neighbour lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
}

/// The stable layout order plus the arrangement rule.
#[derive(Debug, Default)]
pub struct Tiler {
    order: Vec<WindowId>,
    layout: Layout,
    /// Space left around each tile, in logical pixels.
    ///
    /// Zero by default. Gaps are decoration, and on a screen-reader-first
    /// shell the pixels are better spent on content.
    gap: i32,
}

impl Tiler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn toggle_layout(&mut self) -> Layout {
        self.layout = match self.layout {
            Layout::Columns => Layout::Monocle,
            Layout::Monocle => Layout::Columns,
        };
        self.layout
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn order(&self) -> &[WindowId] {
        &self.order
    }

    pub fn index_of(&self, id: WindowId) -> Option<usize> {
        self.order.iter().position(|&w| w == id)
    }

    /// Insert `id` directly after the focused window, or at the end when
    /// nothing is focused.
    ///
    /// Opening a window next to the one you were working in is less
    /// disorienting than having it appear at the far end of the row, which
    /// matters more here than usual: a screen reader user cannot glance at
    /// the screen to find where the new window went.
    pub fn insert_after_focused(&mut self, id: WindowId, focused: Option<WindowId>) {
        if self.order.contains(&id) {
            return;
        }
        match focused.and_then(|f| self.index_of(f)) {
            Some(i) => self.order.insert(i + 1, id),
            None => self.order.push(id),
        }
    }

    pub fn remove(&mut self, id: WindowId) {
        self.order.retain(|&w| w != id);
    }

    /// Exchange the positions of two windows. Returns false if either is
    /// unknown.
    pub fn swap(&mut self, a: WindowId, b: WindowId) -> bool {
        match (self.index_of(a), self.index_of(b)) {
            (Some(i), Some(j)) => {
                self.order.swap(i, j);
                true
            }
            _ => false,
        }
    }

    /// The next window in layout order, wrapping at the end.
    pub fn next(&self, from: WindowId) -> Option<WindowId> {
        let i = self.index_of(from)?;
        self.order.get((i + 1) % self.order.len()).copied()
    }

    /// The previous window in layout order, wrapping at the start.
    pub fn prev(&self, from: WindowId) -> Option<WindowId> {
        let i = self.index_of(from)?;
        let n = self.order.len();
        self.order.get((i + n - 1) % n).copied()
    }

    /// The window physically left or right of `from`.
    ///
    /// Does not wrap: walking off the end of the row should stop, not jump to
    /// the other side, because "left" is a claim about the screen. Returns
    /// `None` in [`Layout::Monocle`], where the windows are stacked and
    /// neither is to the side of the other.
    pub fn neighbour(&self, from: WindowId, dir: Dir) -> Option<WindowId> {
        if self.layout == Layout::Monocle {
            return None;
        }
        let i = self.index_of(from)?;
        let target = match dir {
            Dir::Left => i.checked_sub(1)?,
            Dir::Right => i + 1,
        };
        self.order.get(target).copied()
    }

    /// Geometry for every window, in layout order.
    ///
    /// A total function of `(order, layout, area)`: same inputs, same tiles,
    /// no hidden state.
    pub fn arrange(&self, area: Rectangle<i32, Logical>) -> Vec<(WindowId, Rectangle<i32, Logical>)> {
        let n = self.order.len();
        if n == 0 {
            return Vec::new();
        }

        match self.layout {
            Layout::Monocle => self
                .order
                .iter()
                .map(|&id| (id, inset(area, self.gap)))
                .collect(),

            Layout::Columns => (0..n)
                .map(|k| {
                    // Integer interpolation rather than a divided width: the
                    // boundaries are computed from the area itself, so the
                    // columns tile it exactly and no rounding remainder is
                    // left as a dead stripe on the right edge.
                    let x0 = area.loc.x + scale(area.size.w, k, n);
                    let x1 = area.loc.x + scale(area.size.w, k + 1, n);
                    let rect = Rectangle::new((x0, area.loc.y).into(), (x1 - x0, area.size.h).into());
                    (self.order[k], inset(rect, self.gap))
                })
                .collect(),
        }
    }
}

/// `value * numerator / denominator`, in i64 so a 4K width cannot overflow.
fn scale(value: i32, numerator: usize, denominator: usize) -> i32 {
    ((value as i64) * (numerator as i64) / (denominator as i64)) as i32
}

/// Shrink a rectangle by `gap` on every side, never below 1x1.
fn inset(rect: Rectangle<i32, Logical>, gap: i32) -> Rectangle<i32, Logical> {
    if gap <= 0 {
        return rect;
    }
    let w = (rect.size.w - gap * 2).max(1);
    let h = (rect.size.h - gap * 2).max(1);
    Rectangle::new((rect.loc.x + gap, rect.loc.y + gap).into(), (w, h).into())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1280, 720).into())
    }

    fn tiler(n: usize) -> Tiler {
        let mut t = Tiler::new();
        for i in 0..n {
            t.insert_after_focused(WindowId(i), None);
        }
        t
    }

    // ---- ordering ----

    #[test]
    fn insert_appends_when_nothing_focused() {
        let t = tiler(3);
        assert_eq!(t.order(), &[WindowId(0), WindowId(1), WindowId(2)]);
    }

    #[test]
    fn insert_lands_next_to_the_focused_window() {
        let mut t = tiler(3);
        t.insert_after_focused(WindowId(9), Some(WindowId(0)));
        assert_eq!(
            t.order(),
            &[WindowId(0), WindowId(9), WindowId(1), WindowId(2)]
        );
    }

    #[test]
    fn insert_is_idempotent() {
        let mut t = tiler(2);
        t.insert_after_focused(WindowId(0), None);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn remove_unknown_is_a_noop() {
        let mut t = tiler(2);
        t.remove(WindowId(42));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn swap_exchanges_positions() {
        let mut t = tiler(3);
        assert!(t.swap(WindowId(0), WindowId(2)));
        assert_eq!(
            t.order(),
            &[WindowId(2), WindowId(1), WindowId(0)]
        );
    }

    #[test]
    fn swap_with_unknown_fails_and_changes_nothing() {
        let mut t = tiler(2);
        assert!(!t.swap(WindowId(0), WindowId(7)));
        assert_eq!(t.order(), &[WindowId(0), WindowId(1)]);
    }

    // ---- traversal ----

    #[test]
    fn next_and_prev_wrap() {
        let t = tiler(3);
        assert_eq!(t.next(WindowId(2)), Some(WindowId(0)));
        assert_eq!(t.prev(WindowId(0)), Some(WindowId(2)));
    }

    #[test]
    fn next_on_single_window_is_itself() {
        let t = tiler(1);
        assert_eq!(t.next(WindowId(0)), Some(WindowId(0)));
    }

    #[test]
    fn neighbour_does_not_wrap() {
        let t = tiler(3);
        assert_eq!(t.neighbour(WindowId(0), Dir::Right), Some(WindowId(1)));
        assert_eq!(t.neighbour(WindowId(0), Dir::Left), None);
        assert_eq!(t.neighbour(WindowId(2), Dir::Right), None);
    }

    #[test]
    fn monocle_has_no_side_neighbours() {
        let mut t = tiler(3);
        t.toggle_layout();
        assert_eq!(t.layout(), Layout::Monocle);
        assert_eq!(t.neighbour(WindowId(0), Dir::Right), None);
    }

    // ---- geometry ----

    #[test]
    fn empty_arranges_to_nothing() {
        assert!(Tiler::new().arrange(area()).is_empty());
    }

    #[test]
    fn one_window_gets_the_whole_area() {
        let t = tiler(1);
        assert_eq!(t.arrange(area()), vec![(WindowId(0), area())]);
    }

    #[test]
    fn every_window_gets_exactly_one_tile() {
        for n in 1..8 {
            let t = tiler(n);
            let tiles = t.arrange(area());
            assert_eq!(tiles.len(), n);
            for i in 0..n {
                assert_eq!(tiles.iter().filter(|(id, _)| *id == WindowId(i)).count(), 1);
            }
        }
    }

    #[test]
    fn columns_cover_the_area_exactly() {
        for n in 1..13 {
            let t = tiler(n);
            let covered: i32 = t.arrange(area()).iter().map(|(_, r)| r.size.w).sum();
            assert_eq!(covered, area().size.w, "n = {n}");
        }
    }

    #[test]
    fn columns_never_overlap() {
        for n in 1..13 {
            let t = tiler(n);
            let tiles = t.arrange(area());
            for (i, (_, a)) in tiles.iter().enumerate() {
                for (_, b) in tiles.iter().skip(i + 1) {
                    assert!(!a.overlaps(*b), "n = {n}, {a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn column_widths_differ_by_at_most_one_pixel() {
        for n in 1..13 {
            let t = tiler(n);
            let widths: Vec<i32> = t.arrange(area()).iter().map(|(_, r)| r.size.w).collect();
            let min = *widths.iter().min().unwrap();
            let max = *widths.iter().max().unwrap();
            assert!(max - min <= 1, "n = {n}, widths {widths:?}");
        }
    }

    #[test]
    fn columns_are_full_height_and_inside_the_area() {
        let t = tiler(4);
        for (_, r) in t.arrange(area()) {
            assert_eq!(r.size.h, area().size.h);
            assert!(r.loc.x >= area().loc.x);
            assert!(r.loc.x + r.size.w <= area().loc.x + area().size.w);
        }
    }

    #[test]
    fn a_narrow_area_still_gives_every_window_a_positive_size() {
        // 3 windows across 2 logical pixels: the arithmetic must not hand
        // anyone a zero width. A zero-size configure is what hangs an
        // SDL/Vulkan client's swapchain loop for good.
        let t = tiler(3);
        let tiles = t.arrange(Rectangle::new((0, 0).into(), (2, 100).into()));
        assert_eq!(tiles.len(), 3);
        assert_eq!(tiles.iter().map(|(_, r)| r.size.w).sum::<i32>(), 2);
    }

    #[test]
    fn monocle_gives_everyone_the_whole_area() {
        let mut t = tiler(3);
        t.toggle_layout();
        for (_, r) in t.arrange(area()) {
            assert_eq!(r, area());
        }
    }

    #[test]
    fn the_area_origin_is_respected() {
        let mut t = tiler(2);
        t.toggle_layout();
        t.toggle_layout(); // back to Columns
        let offset = Rectangle::new((100, 50).into(), (400, 300).into());
        let tiles = t.arrange(offset);
        assert_eq!(tiles[0].1.loc.x, 100);
        assert_eq!(tiles[0].1.loc.y, 50);
        assert_eq!(tiles[1].1.loc.x + tiles[1].1.size.w, 500);
    }
}
