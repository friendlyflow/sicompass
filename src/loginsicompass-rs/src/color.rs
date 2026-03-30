//! Hex color parsing.
//!
//! Accepts `#RGB`, `#RRGGBB`, `#RGBA`, `#RRGGBBAA` (same as `color.c`).

/// Linear RGBA color with components in [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// Black (0, 0, 0, 1).
    pub const BLACK: Color = Color::new(0.0, 0.0, 0.0, 1.0);
    /// White (1, 1, 1, 1).
    pub const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);
    /// Transparent (0, 0, 0, 0).
    pub const TRANSPARENT: Color = Color::new(0.0, 0.0, 0.0, 0.0);

    /// Convert to a `u32` in `0xAARRGGBB` format (used by tiny-skia pixel buffers).
    pub fn to_argb32(self) -> u32 {
        let a = (self.a.clamp(0.0, 1.0) * 255.0) as u32;
        let r = (self.r.clamp(0.0, 1.0) * 255.0) as u32;
        let g = (self.g.clamp(0.0, 1.0) * 255.0) as u32;
        let b = (self.b.clamp(0.0, 1.0) * 255.0) as u32;
        (a << 24) | (r << 16) | (g << 8) | b
    }

    /// Convert to tiny-skia `Color`.
    pub fn to_tiny_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(self.r, self.g, self.b, self.a).unwrap_or(tiny_skia::Color::BLACK)
    }
}

/// Parse a hex color string.
///
/// Accepts:
/// - `#RGB` → expands each nibble to `#RRGGBB` + alpha 1.0
/// - `#RRGGBB` → alpha 1.0
/// - `#RGBA` → expands to `#RRGGBBAA`
/// - `#RRGGBBAA`
///
/// Returns `None` if the string doesn't match any supported format.
pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    let (r, g, b, a) = match s.len() {
        3 => {
            // #RGB → #RRGGBB
            let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
            (r, g, b, 255u8)
        }
        4 => {
            // #RGBA → #RRGGBBAA
            let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
            let a = u8::from_str_radix(&s[3..4].repeat(2), 16).ok()?;
            (r, g, b, a)
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.0 / 255.0
    }

    fn color_approx_eq(c: Color, r: f32, g: f32, b: f32, a: f32) -> bool {
        approx_eq(c.r, r) && approx_eq(c.g, g) && approx_eq(c.b, b) && approx_eq(c.a, a)
    }

    #[test]
    fn parse_rrggbb() {
        let c = parse_hex("#ff0080").unwrap();
        assert!(color_approx_eq(c, 1.0, 0.0, 0.502, 1.0));
    }

    #[test]
    fn parse_rgb_expands() {
        let c = parse_hex("#f08").unwrap();
        // #f08 → #ff0088
        assert!(color_approx_eq(c, 1.0, 0.0, 0.533, 1.0));
    }

    #[test]
    fn parse_rgba_expands() {
        let c = parse_hex("#f08f").unwrap();
        // #f08f → #ff0088ff
        assert!(color_approx_eq(c, 1.0, 0.0, 0.533, 1.0));
    }

    #[test]
    fn parse_rrggbbaa() {
        let c = parse_hex("#ff008080").unwrap();
        assert!(approx_eq(c.a, 0.502));
    }

    #[test]
    fn parse_white() {
        let c = parse_hex("#ffffff").unwrap();
        assert!(color_approx_eq(c, 1.0, 1.0, 1.0, 1.0));
    }

    #[test]
    fn parse_black() {
        let c = parse_hex("#000000").unwrap();
        assert!(color_approx_eq(c, 0.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn parse_missing_hash_returns_none() {
        assert!(parse_hex("ff0000").is_none());
    }

    #[test]
    fn parse_invalid_hex_returns_none() {
        assert!(parse_hex("#zzzzzz").is_none());
    }

    #[test]
    fn parse_wrong_length_returns_none() {
        assert!(parse_hex("#ff").is_none());
        assert!(parse_hex("#fffff").is_none());
    }

    #[test]
    fn to_argb32_white() {
        let c = Color::WHITE;
        assert_eq!(c.to_argb32(), 0xFFFFFFFF);
    }

    #[test]
    fn to_argb32_black() {
        let c = Color::BLACK;
        assert_eq!(c.to_argb32(), 0xFF000000);
    }

    #[test]
    fn to_argb32_red() {
        let c = Color::new(1.0, 0.0, 0.0, 1.0);
        assert_eq!(c.to_argb32(), 0xFFFF0000);
    }

    #[test]
    fn to_argb32_transparent() {
        let c = Color::TRANSPARENT;
        assert_eq!(c.to_argb32(), 0x00000000);
    }
}
