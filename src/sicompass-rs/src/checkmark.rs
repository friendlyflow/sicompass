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
}
