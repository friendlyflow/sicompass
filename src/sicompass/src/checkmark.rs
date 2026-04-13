//! Checkmark geometry. Equivalent to `checkmark.c`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment { pub x0: f32, pub y0: f32, pub x1: f32, pub y1: f32 }

/// Generate the two line segments forming a checkmark inside `size×size` pixels at `(cx, cy)`.
pub fn checkmark_segments(cx: f32, cy: f32, size: f32) -> [Segment; 2] {
    let half = size * 0.5;
    let third = size / 3.0;
    [
        Segment { x0: cx - half, y0: cy, x1: cx - third, y1: cy + third },
        Segment { x0: cx - third, y0: cy + third, x1: cx + half, y1: cy - half + third },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn returns_two_segments() { assert_eq!(checkmark_segments(10.0, 10.0, 12.0).len(), 2); }
    #[test] fn short_ends_where_long_begins() {
        let s = checkmark_segments(10.0, 10.0, 12.0);
        assert!((s[0].x1 - s[1].x0).abs() < f32::EPSILON);
        assert!((s[0].y1 - s[1].y0).abs() < f32::EPSILON);
    }
    #[test] fn zero_size_collapses() {
        for s in checkmark_segments(5.0, 5.0, 0.0) {
            assert!((s.x0 - s.x1).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn size_scaling_doubles_extent() {
        let s1 = checkmark_segments(0.0, 0.0, 24.0);
        let s2 = checkmark_segments(0.0, 0.0, 48.0);
        // All coordinates should scale proportionally
        let approx_eq = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(approx_eq(s2[0].x0, s1[0].x0 * 2.0));
        assert!(approx_eq(s2[1].x1, s1[1].x1 * 2.0));
    }

    #[test]
    fn offset_position_shifts_all_coords() {
        let base = checkmark_segments(0.0, 0.0, 24.0);
        let offset = checkmark_segments(100.0, 200.0, 24.0);
        let approx_eq = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(approx_eq(offset[0].x0, base[0].x0 + 100.0));
        assert!(approx_eq(offset[0].y0, base[0].y0 + 200.0));
        assert!(approx_eq(offset[1].x1, base[1].x1 + 100.0));
        assert!(approx_eq(offset[1].y1, base[1].y1 + 200.0));
    }

    #[test]
    fn segments_within_bounding_box() {
        let x = 10.0_f32;
        let y = 20.0_f32;
        let size = 48.0_f32;
        let margin = size * 0.2;
        let s = checkmark_segments(x, y, size);
        for seg in &s {
            for &px in &[seg.x0, seg.x1] {
                assert!(px >= x - size * 0.5 - margin);
                assert!(px <= x + size * 0.5 + margin);
            }
            for &py in &[seg.y0, seg.y1] {
                assert!(py >= y - size * 0.5 - margin);
                assert!(py <= y + size * 0.5 + margin + size / 3.0);
            }
        }
    }

    #[test]
    fn short_segment_goes_down_right() {
        let s = checkmark_segments(0.0, 0.0, 24.0);
        // Short stroke: starts top-left, goes down-right
        assert!(s[0].x1 > s[0].x0, "short seg should go rightward");
        assert!(s[0].y1 > s[0].y0, "short seg should go downward");
    }

    #[test]
    fn long_segment_goes_up_right() {
        let s = checkmark_segments(0.0, 0.0, 24.0);
        // Long stroke: from valley, goes up-right
        assert!(s[1].x1 > s[1].x0, "long seg should go rightward");
        assert!(s[1].y1 < s[1].y0, "long seg should go upward");
    }
}
