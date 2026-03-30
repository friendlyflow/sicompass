//! Software renderer — draws the login screen into a pixel buffer.
//!
//! Replaces the EGL + OpenGL ES 2.0 + Cairo + Pango stack from
//! `src/loginsicompass/` with a pure-Rust pipeline:
//!
//! * [`tiny_skia`] for 2D vector drawing (background fill, rounded rects,
//!   password dots, border)
//! * [`image`] crate for loading PNG/JPEG background images
//! * Output: a `Vec<u32>` in `0xAARRGGBB` format suitable for `wl_shm`

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use crate::color::Color;
use crate::entry::{InputMode, PasswordEntry};

// ---------------------------------------------------------------------------
// RenderConfig
// ---------------------------------------------------------------------------

/// All styling options for a single frame.  Mirrors the CLI flags from
/// `src/loginsicompass/main.c`.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// Window dimensions in pixels.
    pub width: u32,
    pub height: u32,

    /// Background fill colour (used when no image is loaded).
    pub background_color: Color,

    /// Pre-decoded RGBA background image (optional).
    pub background_image: Option<image::RgbaImage>,

    // --- Entry box ---
    pub entry_background: Color,
    pub entry_foreground: Color,
    pub border_color: Color,
    pub border_width: u32,
    pub outline_color: Color,
    pub outline_width: u32,
    pub padding: u32,

    /// Radius of each password dot.
    pub dot_radius: f32,

    /// Number of character slots shown in the entry box.
    pub num_characters: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            width: 640,
            height: 480,
            background_color: Color::new(0.89, 0.80, 0.824, 1.0),
            background_image: None,
            entry_background: Color::new(0.106, 0.114, 0.118, 1.0),
            entry_foreground: Color::WHITE,
            border_color: Color::new(0.976, 0.149, 0.447, 1.0),
            border_width: 6,
            outline_color: Color::new(0.031, 0.031, 0.0, 1.0),
            outline_width: 2,
            padding: 8,
            dot_radius: 8.0,
            num_characters: 12,
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render a complete login screen frame into a `Vec<u32>` pixel buffer.
///
/// Pixels are in `0xAARRGGBB` (little-endian BGRA on disk, but `u32` integer
/// value matches `0xAARRGGBB`) — the format expected by `wl_shm WL_SHM_FORMAT_ARGB8888`.
pub fn render_frame(cfg: &RenderConfig, entry: &PasswordEntry) -> Vec<u32> {
    let mut pixmap = Pixmap::new(cfg.width, cfg.height).unwrap_or_else(|| {
        Pixmap::new(1, 1).unwrap()
    });

    // ---- 1. Background ----
    draw_background(&mut pixmap, cfg);

    // ---- 2. Entry box centred in the lower third ----
    let box_width = entry_box_width(cfg);
    let box_height = entry_box_height(cfg);
    let box_x = (cfg.width.saturating_sub(box_width)) / 2;
    let box_y = cfg.height * 2 / 3;

    draw_entry_box(&mut pixmap, cfg, entry, box_x, box_y, box_width, box_height);

    // ---- 3. Convert tiny-skia RGBA to wl_shm ARGB32 ----
    pixmap
        .pixels()
        .iter()
        .map(|p| {
            let a = p.alpha() as u32;
            let r = p.red() as u32;
            let g = p.green() as u32;
            let b = p.blue() as u32;
            (a << 24) | (r << 16) | (g << 8) | b
        })
        .collect()
}

fn draw_background(pixmap: &mut Pixmap, cfg: &RenderConfig) {
    if let Some(img) = &cfg.background_image {
        // Scale image to fill the window.
        let scaled = image::imageops::resize(
            img,
            cfg.width,
            cfg.height,
            image::imageops::FilterType::Triangle,
        );
        let pixels = pixmap.pixels_mut();
        for (x, y, px) in scaled.enumerate_pixels() {
            let idx = (y * cfg.width + x) as usize;
            if idx < pixels.len() {
                let c = tiny_skia::ColorU8::from_rgba(px[0], px[1], px[2], px[3]);
                pixels[idx] = c.premultiply();
            }
        }
    } else {
        let mut paint = Paint::default();
        paint.set_color(cfg.background_color.to_tiny_skia());
        pixmap.fill_rect(
            tiny_skia::Rect::from_xywh(0.0, 0.0, cfg.width as f32, cfg.height as f32).unwrap(),
            &paint,
            Transform::identity(),
            None,
        );
    }
}

fn draw_entry_box(
    pixmap: &mut Pixmap,
    cfg: &RenderConfig,
    entry: &PasswordEntry,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) {
    let xf = x as f32;
    let yf = y as f32;
    let wf = w as f32;
    let hf = h as f32;

    // Outer outline
    let ow = cfg.outline_width as f32;
    draw_rect(pixmap, cfg.outline_color, xf, yf, wf, hf);

    // Border
    let bw = cfg.border_width as f32;
    draw_rect(
        pixmap,
        cfg.border_color,
        xf + ow,
        yf + ow,
        wf - 2.0 * ow,
        hf - 2.0 * ow,
    );

    // Inner fill
    let inner_off = ow + bw;
    draw_rect(
        pixmap,
        cfg.entry_background,
        xf + inner_off,
        yf + inner_off,
        wf - 2.0 * inner_off,
        hf - 2.0 * inner_off,
    );

    // Password dots
    let filled = entry.len();
    if filled > 0 {
        let r = cfg.dot_radius;
        let pad = cfg.padding as f32;
        let step = (wf - 2.0 * (inner_off + pad)) / cfg.num_characters as f32;
        let dot_y = yf + hf / 2.0;
        let start_x = xf + inner_off + pad + step / 2.0;

        let color = match entry.mode {
            InputMode::Secret => cfg.entry_foreground,
            InputMode::Visible => cfg.entry_foreground,
        };
        let mut paint = Paint::default();
        paint.set_color(color.to_tiny_skia());
        paint.anti_alias = true;

        for i in 0..filled.min(cfg.num_characters as usize) {
            let cx = start_x + i as f32 * step;
            let path = PathBuilder::from_circle(cx, dot_y, r).unwrap();
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
}

fn draw_rect(pixmap: &mut Pixmap, color: Color, x: f32, y: f32, w: f32, h: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut paint = Paint::default();
    paint.set_color(color.to_tiny_skia());
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

/// Width of the entry box in pixels.
pub fn entry_box_width(cfg: &RenderConfig) -> u32 {
    (cfg.num_characters as f32 * (cfg.dot_radius * 2.0 + cfg.padding as f32)
        + cfg.padding as f32 * 2.0
        + (cfg.border_width + cfg.outline_width) as f32 * 4.0) as u32
}

/// Height of the entry box in pixels.
pub fn entry_box_height(cfg: &RenderConfig) -> u32 {
    (cfg.dot_radius * 2.0
        + cfg.padding as f32 * 2.0
        + (cfg.border_width + cfg.outline_width) as f32 * 4.0) as u32
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::PasswordEntry;

    fn default_entry() -> PasswordEntry {
        PasswordEntry::new()
    }

    #[test]
    fn render_frame_returns_correct_size() {
        let cfg = RenderConfig {
            width: 100,
            height: 80,
            ..Default::default()
        };
        let pixels = render_frame(&cfg, &default_entry());
        assert_eq!(pixels.len(), 100 * 80);
    }

    #[test]
    fn background_color_fills_frame() {
        let cfg = RenderConfig {
            width: 4,
            height: 4,
            background_color: Color::new(1.0, 0.0, 0.0, 1.0),
            ..Default::default()
        };
        let pixels = render_frame(&cfg, &default_entry());
        // At least the top-left corner should be the background color.
        // (Entry box is in the lower third, so top pixels are pure background.)
        let expected_r = (pixels[0] >> 16) & 0xFF;
        assert_eq!(expected_r, 255, "top-left pixel should be fully red");
    }

    #[test]
    fn entry_box_dimensions_are_positive() {
        let cfg = RenderConfig::default();
        assert!(entry_box_width(&cfg) > 0);
        assert!(entry_box_height(&cfg) > 0);
    }

    #[test]
    fn render_with_entry_does_not_panic() {
        let cfg = RenderConfig {
            width: 320,
            height: 240,
            ..Default::default()
        };
        let mut entry = PasswordEntry::new();
        for ch in "password".chars() {
            entry.push(ch);
        }
        let pixels = render_frame(&cfg, &entry);
        assert_eq!(pixels.len(), 320 * 240);
    }

    #[test]
    fn render_full_entry_does_not_panic() {
        let cfg = RenderConfig {
            width: 640,
            height: 480,
            ..Default::default()
        };
        let mut entry = PasswordEntry::new();
        for _ in 0..crate::entry::MAX_PASSWORD_LENGTH {
            entry.push('x');
        }
        render_frame(&cfg, &entry); // must not panic
    }
}
