//! Font files, embedded in the binary.
//!
//! These used to be read from `fonts/` relative to the process working
//! directory, which meant the binary only started when launched from a
//! directory that happened to have a `fonts/` tree beside it. No release
//! archive, installer or package ever shipped one, so the published binary
//! could not start. Embedding them removes the failure mode.
//!
//! FreeType reads faces straight from memory via `FT_New_Memory_Face`. The
//! buffer has to outlive the face, which `'static` data satisfies for free,
//! so this is also stricter than the path-based loading it replaced.
//!
//! Everything here is redistributable, which is the other reason the set
//! changed: the previous primary face was Microsoft Consolas, which is
//! licensed only for use with the product it ships in and could not legally
//! go into a `.deb`, `.rpm`, `.dmg` or AppImage.

/// Primary face. Every codepoint is looked up here first.
///
/// DejaVu Sans Mono, under the Bitstream Vera / DejaVu license
/// (`fonts/LICENSE-DejaVu.txt`).
///
/// It replaced Consolas, which besides being non-redistributable was in
/// practice a 600-codepoint subset with no box-drawing glyphs at all, so the
/// `─`, `→` and `▸` the list and breadcrumb code draws were already coming
/// from this very face through the fallback chain.
pub const PRIMARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/DejaVuSansMono.ttf"
));

/// Faces tried, in order, for any codepoint [`PRIMARY`] lacks.
///
/// DejaVu Sans is proportional but covers Latin Extended, Greek, Cyrillic,
/// arrows and math symbols beyond what the mono face carries. (CJK is not
/// covered. Add e.g. Noto Sans Mono CJK here to enable it.)
pub const FALLBACKS: &[&[u8]] = &[include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/DejaVuSans.ttf"
))];

/// Color emoji face, rasterised into the separate RGBA atlas.
///
/// Noto Color Emoji, under the SIL Open Font License
/// (`fonts/LICENSE-NotoColorEmoji.txt`). Optional at runtime: if the face
/// fails to parse, emoji are simply disabled.
///
/// # This needs a FreeType built with PNG support
///
/// The strikes in this font are PNG bitmaps, so FreeType can only rasterise
/// them when it was compiled with `FT_CONFIG_OPTION_USE_PNG`. freetype-sys
/// 0.20.1 links the system FreeType when pkg-config finds
/// `freetype2 >= 24.3.18`, and otherwise falls back to a vendored `cc` build
/// that has PNG support switched off. So on Linux, `libfreetype-dev` at build
/// time and `libfreetype6` at run time are functional requirements, not
/// leftovers, and both `ci.yml` and `dist-workspace.toml` install them
/// deliberately.
///
/// `text::tests::color_atlas_rasterizes_emoji_to_rgba` is what catches a
/// build that lost it. The symptom otherwise is emoji silently disappearing.
pub const COLOR_EMOJI: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/NotoColorEmoji.ttf"
));

#[cfg(test)]
mod tests {
    use super::*;

    /// TrueType and OpenType magic. Catches an `include_bytes!` pointing at a
    /// path that exists but is not a font, and a truncated checkout.
    fn is_font(bytes: &[u8]) -> bool {
        matches!(
            bytes.get(..4),
            Some(b"\x00\x01\x00\x00") | Some(b"true") | Some(b"ttcf") | Some(b"OTTO")
        )
    }

    #[test]
    fn every_embedded_font_parses_as_a_font() {
        assert!(is_font(PRIMARY), "PRIMARY is not a TrueType/OpenType file");
        assert!(
            is_font(COLOR_EMOJI),
            "COLOR_EMOJI is not a TrueType/OpenType file"
        );
        for (i, f) in FALLBACKS.iter().enumerate() {
            assert!(is_font(f), "FALLBACKS[{i}] is not a TrueType/OpenType file");
        }
    }

    /// The whole point of embedding is that the binary carries these. A zero
    /// length blob would still compile and would only fail at startup.
    #[test]
    fn embedded_fonts_are_not_empty() {
        assert!(PRIMARY.len() > 10_000, "PRIMARY is {} bytes", PRIMARY.len());
        assert!(
            COLOR_EMOJI.len() > 10_000,
            "COLOR_EMOJI is {} bytes",
            COLOR_EMOJI.len()
        );
        for (i, f) in FALLBACKS.iter().enumerate() {
            assert!(f.len() > 10_000, "FALLBACKS[{i}] is {} bytes", f.len());
        }
    }
}
