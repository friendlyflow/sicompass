/// Tag parsing for FFON element strings.
///
/// Tags are HTML-like markers embedded in string content that control rendering
/// and interactivity. They support escaped angle brackets: `\<` → `<`.
///
/// Example: `"Edit this: <input>hello</input>"` is an editable field.

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Like `str::find` but skips matches where `<` is preceded by `\`.
fn find_unescaped(haystack: &str, needle: &str) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    if needle_bytes.is_empty() {
        return Some(0);
    }
    let mut start = 0;
    while start + needle_bytes.len() <= bytes.len() {
        if let Some(rel) = bytes[start..].windows(needle_bytes.len())
            .position(|w| w == needle_bytes)
        {
            let abs = start + rel;
            if abs > 0 && bytes[abs - 1] == b'\\' {
                start = abs + 1;
                continue;
            }
            return Some(abs);
        } else {
            break;
        }
    }
    None
}

/// Replace `\<` → `<` and `\>` → `>` in place.
fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('<') | Some('>') => {
                    result.push(chars.next().unwrap());
                }
                _ => result.push(c),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Extract the slice between `open_tag` and `close_tag` within `text`, skipping escaped tags.
/// Returns `None` if either tag is missing.
fn extract_between<'a>(text: &'a str, open: &str, open_len: usize, close: &str) -> Option<&'a str> {
    let start_pos = find_unescaped(text, open)?;
    let content_start = start_pos + open_len;
    let close_pos = text[content_start..].find(close)?;
    Some(&text[content_start..content_start + close_pos])
}

/// Extract the slice after `open_tag` (no close tag required), skipping escaped tags.
fn extract_after<'a>(text: &'a str, open: &str, open_len: usize, close: &str) -> Option<&'a str> {
    let start_pos = find_unescaped(text, open)?;
    let content_start = start_pos + open_len;
    let end = text[content_start..].find(close)
        .map(|p| content_start + p)
        .unwrap_or(text.len());
    Some(&text[content_start..end])
}

// ---------------------------------------------------------------------------
// Tag constants
// ---------------------------------------------------------------------------

const INPUT_OPEN: &str = "<input>";
const INPUT_CLOSE: &str = "</input>";
const RADIO_OPEN: &str = "<radio>";
const RADIO_CLOSE: &str = "</radio>";
const CHECKED_OPEN: &str = "<checked>";
const CHECKED_CLOSE: &str = "</checked>";
const CHECKBOX_OPEN: &str = "<checkbox>";
const CHECKBOX_CHECKED_OPEN: &str = "<checkbox checked>";
const CHECKBOX_CLOSE: &str = "</checkbox>";
const LINK_OPEN: &str = "<link>";
const LINK_CLOSE: &str = "</link>";
const IMAGE_OPEN: &str = "<image>";
const IMAGE_CLOSE: &str = "</image>";
const ID_OPEN: &str = "<id>";
const ID_CLOSE: &str = "</id>";
const BUTTON_OPEN: &str = "<button>";
const BUTTON_CLOSE: &str = "</button>";
const MANY_OPT_TAG: &str = "<many-opt></many-opt>";
const ONE_OPT_TAG: &str = "<one-opt></one-opt>";

// ---------------------------------------------------------------------------
// <input>...</input>
// ---------------------------------------------------------------------------

/// Returns true if `text` contains `<input>...</input>`.
pub fn has_input(text: &str) -> bool {
    find_unescaped(text, INPUT_OPEN).is_some()
        && find_unescaped(text, INPUT_CLOSE).is_some()
}

/// Extracts content between `<input>` and `</input>`.
pub fn extract_input(text: &str) -> Option<String> {
    extract_between(text, INPUT_OPEN, INPUT_OPEN.len(), INPUT_CLOSE).map(|s| s.to_owned())
}

/// Wraps `content` in `<input>...</input>`.
pub fn format_input(content: &str) -> String {
    format!("{INPUT_OPEN}{content}{INPUT_CLOSE}")
}

// ---------------------------------------------------------------------------
// <radio>...</radio>
// ---------------------------------------------------------------------------

pub fn has_radio(text: &str) -> bool {
    find_unescaped(text, RADIO_OPEN).is_some()
}

pub fn extract_radio(text: &str) -> Option<String> {
    extract_after(text, RADIO_OPEN, RADIO_OPEN.len(), RADIO_CLOSE).map(|s| s.to_owned())
}

// ---------------------------------------------------------------------------
// <checked>...</checked>
// ---------------------------------------------------------------------------

pub fn has_checked(text: &str) -> bool {
    find_unescaped(text, CHECKED_OPEN).is_some()
}

pub fn extract_checked(text: &str) -> Option<String> {
    extract_after(text, CHECKED_OPEN, CHECKED_OPEN.len(), CHECKED_CLOSE).map(|s| s.to_owned())
}

pub fn format_checked(content: &str) -> String {
    format!("{CHECKED_OPEN}{content}")
}

// ---------------------------------------------------------------------------
// <checkbox>...</checkbox> / <checkbox checked>...</checkbox>
// ---------------------------------------------------------------------------

/// Returns true if `text` has `<checkbox>` but NOT `<checkbox checked>`.
pub fn has_checkbox(text: &str) -> bool {
    find_unescaped(text, CHECKBOX_OPEN).is_some()
        && find_unescaped(text, CHECKBOX_CHECKED_OPEN).is_none()
}

pub fn has_checkbox_checked(text: &str) -> bool {
    find_unescaped(text, CHECKBOX_CHECKED_OPEN).is_some()
}

pub fn extract_checkbox(text: &str) -> Option<String> {
    extract_after(text, CHECKBOX_OPEN, CHECKBOX_OPEN.len(), CHECKBOX_CLOSE).map(|s| s.to_owned())
}

pub fn extract_checkbox_checked(text: &str) -> Option<String> {
    extract_after(text, CHECKBOX_CHECKED_OPEN, CHECKBOX_CHECKED_OPEN.len(), CHECKBOX_CLOSE)
        .map(|s| s.to_owned())
}

pub fn format_checkbox(content: &str) -> String {
    format!("{CHECKBOX_OPEN}{content}")
}

pub fn format_checkbox_checked(content: &str) -> String {
    format!("{CHECKBOX_CHECKED_OPEN}{content}")
}

// ---------------------------------------------------------------------------
// <link>...</link>
// ---------------------------------------------------------------------------

pub fn has_link(text: &str) -> bool {
    find_unescaped(text, LINK_OPEN).is_some()
        && find_unescaped(text, LINK_CLOSE).is_some()
}

pub fn extract_link(text: &str) -> Option<String> {
    extract_between(text, LINK_OPEN, LINK_OPEN.len(), LINK_CLOSE).map(|s| s.to_owned())
}

// ---------------------------------------------------------------------------
// <image>...</image>
// ---------------------------------------------------------------------------

pub fn has_image(text: &str) -> bool {
    find_unescaped(text, IMAGE_OPEN).is_some()
        && find_unescaped(text, IMAGE_CLOSE).is_some()
}

pub fn extract_image(text: &str) -> Option<String> {
    extract_between(text, IMAGE_OPEN, IMAGE_OPEN.len(), IMAGE_CLOSE).map(|s| s.to_owned())
}

// ---------------------------------------------------------------------------
// <id>...</id>
// ---------------------------------------------------------------------------

/// Returns true if `text` contains `<id>...</id>` (HTML element id metadata).
pub fn has_id(text: &str) -> bool {
    find_unescaped(text, ID_OPEN).is_some()
        && find_unescaped(text, ID_CLOSE).is_some()
}

/// Extracts the id value between `<id>` and `</id>`.
pub fn extract_id(text: &str) -> Option<String> {
    extract_between(text, ID_OPEN, ID_OPEN.len(), ID_CLOSE).map(|s| s.to_owned())
}

/// Wraps `id` in `<id>...</id>`.
pub fn format_id(id: &str) -> String {
    format!("{ID_OPEN}{id}{ID_CLOSE}")
}

// ---------------------------------------------------------------------------
// <many-opt></many-opt> / <one-opt></one-opt> prefixes
// ---------------------------------------------------------------------------

pub fn has_many_opt(text: &str) -> bool {
    text.starts_with(MANY_OPT_TAG)
}

pub fn has_one_opt(text: &str) -> bool {
    text.starts_with(ONE_OPT_TAG)
}

pub fn strip_many_opt(text: &str) -> &str {
    text.strip_prefix(MANY_OPT_TAG).unwrap_or(text)
}

pub fn strip_one_opt(text: &str) -> &str {
    text.strip_prefix(ONE_OPT_TAG).unwrap_or(text)
}

pub fn format_many_opt(key: &str) -> String {
    format!("{MANY_OPT_TAG}{key}")
}

pub fn format_one_opt(key: &str) -> String {
    format!("{ONE_OPT_TAG}{key}")
}

// ---------------------------------------------------------------------------
// <button>function_name</button>Display Text
// ---------------------------------------------------------------------------

pub fn has_button(text: &str) -> bool {
    find_unescaped(text, BUTTON_OPEN).is_some()
        && find_unescaped(text, BUTTON_CLOSE).is_some()
}

pub fn extract_button_function_name(text: &str) -> Option<String> {
    extract_between(text, BUTTON_OPEN, BUTTON_OPEN.len(), BUTTON_CLOSE).map(|s| s.to_owned())
}

pub fn extract_button_display_text(text: &str) -> Option<String> {
    let close_pos = text.find(BUTTON_CLOSE)?;
    Some(text[close_pos + BUTTON_CLOSE.len()..].to_owned())
}

// ---------------------------------------------------------------------------
// strip_display — remove all tags for plain text rendering, unescape \< \>
// ---------------------------------------------------------------------------

/// Strip all recognized tags from `text` and unescape `\<`/`\>`.
///
/// Mirrors `providerTagStripDisplay` in `tags.c`.
pub fn strip_display(text: &str) -> String {
    // 1. Strip opt-tag prefixes recursively
    if has_one_opt(text) {
        return strip_display(strip_one_opt(text));
    }
    if has_many_opt(text) {
        return strip_display(strip_many_opt(text));
    }

    // 2. Button: keep text before <button> + display text after </button>,
    //    then recurse to strip any remaining tags (e.g. <input>) in the display text.
    if let Some(btn_open) = find_unescaped(text, BUTTON_OPEN) {
        let before = &text[..btn_open];
        if let Some(btn_close) = text[btn_open..].find(BUTTON_CLOSE) {
            let display_start = btn_open + btn_close + BUTTON_CLOSE.len();
            let result = format!("{}{}", before, &text[display_start..]);
            return strip_display(&unescape(&result));
        }
        // Fallback: extract display text
        return extract_button_display_text(text)
            .map(|s| strip_display(&unescape(&s)))
            .unwrap_or_else(|| unescape(text));
    }

    // 3. <id>...</id>: metadata tag — strip the tag AND its content (id is not display text)
    if let Some(open_pos) = find_unescaped(text, ID_OPEN) {
        let before = &text[..open_pos];
        let after_open = &text[open_pos + ID_OPEN.len()..];
        let after = after_open.find(ID_CLOSE)
            .map(|p| &after_open[p + ID_CLOSE.len()..])
            .unwrap_or("");
        let result = format!("{before}{after}");
        return strip_display(&unescape(&result));
    }

    // 4. Find the first recognized tag pair and strip it
    let candidates: &[(&str, usize, &str)] = &[
        (INPUT_OPEN,             INPUT_OPEN.len(),             INPUT_CLOSE),
        (RADIO_OPEN,             RADIO_OPEN.len(),             RADIO_CLOSE),
        (CHECKED_OPEN,           CHECKED_OPEN.len(),           CHECKED_CLOSE),
        (CHECKBOX_CHECKED_OPEN,  CHECKBOX_CHECKED_OPEN.len(),  CHECKBOX_CLOSE),
        (CHECKBOX_OPEN,          CHECKBOX_OPEN.len(),          CHECKBOX_CLOSE),
        (LINK_OPEN,              LINK_OPEN.len(),              LINK_CLOSE),
        (IMAGE_OPEN,             IMAGE_OPEN.len(),             IMAGE_CLOSE),
    ];

    for &(open, open_len, close) in candidates {
        if let Some(open_pos) = find_unescaped(text, open) {
            let content_start = open_pos + open_len;
            let before = &text[..open_pos];
            let after_tags_content = &text[content_start..];
            let (content, after) = if let Some(close_pos) = after_tags_content.find(close) {
                (
                    &after_tags_content[..close_pos],
                    &after_tags_content[close_pos + close.len()..],
                )
            } else {
                (after_tags_content, "")
            };
            let result = format!("{before}{content}{after}");
            return unescape(&result);
        }
    }

    // No tags found — just unescape
    unescape(text)
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_provider/test_provider_tags.c (93 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- has_input ---

    #[test]
    fn test_has_input_true() {
        assert!(has_input("<input>hello</input>"));
    }

    #[test]
    fn test_has_input_false_no_tag() {
        assert!(!has_input("plain text"));
    }

    #[test]
    fn test_has_input_missing_close() {
        assert!(!has_input("<input>hello"));
    }

    #[test]
    fn test_has_input_escaped() {
        assert!(!has_input("\\<input>hello\\</input>"));
    }

    #[test]
    fn test_has_input_with_prefix_suffix() {
        assert!(has_input("prefix <input>val</input> suffix"));
    }

    // --- extract_input ---

    #[test]
    fn test_extract_input_basic() {
        assert_eq!(extract_input("<input>hello</input>"), Some("hello".to_owned()));
    }

    #[test]
    fn test_extract_input_empty() {
        assert_eq!(extract_input("<input></input>"), Some("".to_owned()));
    }

    #[test]
    fn test_extract_input_with_prefix_suffix() {
        assert_eq!(
            extract_input("Label: <input>value</input> (hint)"),
            Some("value".to_owned())
        );
    }

    #[test]
    fn test_extract_input_none_no_tag() {
        assert_eq!(extract_input("no tag"), None);
    }

    // --- format_input ---

    #[test]
    fn test_format_input() {
        assert_eq!(format_input("hello"), "<input>hello</input>");
    }

    #[test]
    fn test_format_input_empty() {
        assert_eq!(format_input(""), "<input></input>");
    }

    // --- radio ---

    #[test]
    fn test_has_radio_true() {
        assert!(has_radio("<radio>pick a color"));
    }

    #[test]
    fn test_has_radio_false() {
        assert!(!has_radio("plain"));
    }

    #[test]
    fn test_extract_radio_with_close() {
        assert_eq!(extract_radio("<radio>color</radio>"), Some("color".to_owned()));
    }

    #[test]
    fn test_extract_radio_no_close() {
        assert_eq!(extract_radio("<radio>color"), Some("color".to_owned()));
    }

    // --- checked ---

    #[test]
    fn test_has_checked_true() {
        assert!(has_checked("<checked>blue"));
    }

    #[test]
    fn test_has_checked_false() {
        assert!(!has_checked("blue"));
    }

    #[test]
    fn test_extract_checked() {
        assert_eq!(extract_checked("<checked>blue"), Some("blue".to_owned()));
    }

    #[test]
    fn test_format_checked() {
        assert_eq!(format_checked("blue"), "<checked>blue");
    }

    // --- checkbox ---

    #[test]
    fn test_has_checkbox_true() {
        assert!(has_checkbox("<checkbox>label"));
    }

    #[test]
    fn test_has_checkbox_false_for_checked() {
        assert!(!has_checkbox("<checkbox checked>label"));
    }

    #[test]
    fn test_has_checkbox_checked_true() {
        assert!(has_checkbox_checked("<checkbox checked>label"));
    }

    #[test]
    fn test_has_checkbox_checked_false() {
        assert!(!has_checkbox_checked("<checkbox>label"));
    }

    #[test]
    fn test_extract_checkbox_with_close() {
        assert_eq!(
            extract_checkbox("<checkbox>my label</checkbox>"),
            Some("my label".to_owned())
        );
    }

    #[test]
    fn test_extract_checkbox_no_close() {
        assert_eq!(extract_checkbox("<checkbox>my label"), Some("my label".to_owned()));
    }

    #[test]
    fn test_extract_checkbox_checked() {
        assert_eq!(
            extract_checkbox_checked("<checkbox checked>my label"),
            Some("my label".to_owned())
        );
    }

    #[test]
    fn test_format_checkbox() {
        assert_eq!(format_checkbox("label"), "<checkbox>label");
    }

    #[test]
    fn test_format_checkbox_checked() {
        assert_eq!(format_checkbox_checked("label"), "<checkbox checked>label");
    }

    // --- link ---

    #[test]
    fn test_has_link_true() {
        assert!(has_link("<link>/path/to/file.json</link>"));
    }

    #[test]
    fn test_has_link_false() {
        assert!(!has_link("no link"));
    }

    #[test]
    fn test_extract_link() {
        assert_eq!(
            extract_link("<link>/path/to/file.json</link>"),
            Some("/path/to/file.json".to_owned())
        );
    }

    #[test]
    fn test_extract_link_with_prefix() {
        assert_eq!(
            extract_link("See: <link>https://example.com</link>"),
            Some("https://example.com".to_owned())
        );
    }

    // --- image ---

    #[test]
    fn test_has_image_true() {
        assert!(has_image("<image>/path/to/img.jpg</image>"));
    }

    #[test]
    fn test_has_image_false() {
        assert!(!has_image("no image"));
    }

    #[test]
    fn test_extract_image() {
        assert_eq!(
            extract_image("<image>/img.png</image>"),
            Some("/img.png".to_owned())
        );
    }

    // --- many-opt / one-opt ---

    #[test]
    fn test_has_many_opt_true() {
        assert!(has_many_opt("<many-opt></many-opt>key"));
    }

    #[test]
    fn test_has_many_opt_false() {
        assert!(!has_many_opt("key"));
    }

    #[test]
    fn test_has_one_opt_true() {
        assert!(has_one_opt("<one-opt></one-opt>key"));
    }

    #[test]
    fn test_strip_many_opt() {
        assert_eq!(strip_many_opt("<many-opt></many-opt>rest"), "rest");
    }

    #[test]
    fn test_strip_one_opt() {
        assert_eq!(strip_one_opt("<one-opt></one-opt>rest"), "rest");
    }

    #[test]
    fn test_strip_opt_no_tag() {
        assert_eq!(strip_one_opt("no tag"), "no tag");
    }

    #[test]
    fn test_format_many_opt() {
        assert_eq!(format_many_opt("key"), "<many-opt></many-opt>key");
    }

    #[test]
    fn test_format_one_opt() {
        assert_eq!(format_one_opt("key"), "<one-opt></one-opt>key");
    }

    // --- button ---

    #[test]
    fn test_has_button_true() {
        assert!(has_button("<button>fn_name</button>Display"));
    }

    #[test]
    fn test_has_button_false() {
        assert!(!has_button("no button"));
    }

    #[test]
    fn test_extract_button_function_name() {
        assert_eq!(
            extract_button_function_name("<button>myFunction</button>Click me"),
            Some("myFunction".to_owned())
        );
    }

    #[test]
    fn test_extract_button_display_text() {
        assert_eq!(
            extract_button_display_text("<button>fn</button>Click me"),
            Some("Click me".to_owned())
        );
    }

    #[test]
    fn test_extract_button_display_empty() {
        assert_eq!(
            extract_button_display_text("<button>fn</button>"),
            Some("".to_owned())
        );
    }

    // --- strip_display ---

    #[test]
    fn test_strip_display_plain() {
        assert_eq!(strip_display("plain text"), "plain text");
    }

    #[test]
    fn test_strip_display_input() {
        assert_eq!(strip_display("<input>hello</input>"), "hello");
    }

    #[test]
    fn test_strip_display_input_with_prefix_suffix() {
        assert_eq!(strip_display("Label: <input>val</input> (hint)"), "Label: val (hint)");
    }

    #[test]
    fn test_strip_display_radio() {
        assert_eq!(strip_display("<radio>pick a color</radio>"), "pick a color");
    }

    #[test]
    fn test_strip_display_checked() {
        assert_eq!(strip_display("<checked>blue</checked>"), "blue");
    }

    #[test]
    fn test_strip_display_checkbox() {
        assert_eq!(strip_display("<checkbox>label</checkbox>"), "label");
    }

    #[test]
    fn test_strip_display_checkbox_checked() {
        assert_eq!(strip_display("<checkbox checked>label</checkbox>"), "label");
    }

    #[test]
    fn test_strip_display_link() {
        assert_eq!(strip_display("<link>https://example.com</link>"), "https://example.com");
    }

    #[test]
    fn test_strip_display_image() {
        assert_eq!(strip_display("<image>/img.jpg</image>"), "/img.jpg");
    }

    #[test]
    fn test_strip_display_button() {
        assert_eq!(strip_display("<button>fn</button>Click me"), "Click me");
    }

    #[test]
    fn test_strip_display_button_with_prefix() {
        assert_eq!(strip_display("Add: <button>fn</button>Click me"), "Add: Click me");
    }

    #[test]
    fn test_strip_display_button_with_input_in_display() {
        // Button display text itself contains <input> tags — should be stripped recursively.
        // Mirrors the sales demo case: one-opt key like "max pressure: <input>100</input> Pa"
        assert_eq!(
            strip_display("<button>one-opt:max pressure: <input>100</input> Pa</button>max pressure: <input>100</input> Pa"),
            "max pressure: 100 Pa"
        );
    }

    #[test]
    fn test_strip_display_button_with_radio_in_display() {
        // Button display text contains <radio> tag — should be stripped recursively.
        assert_eq!(
            strip_display("<button>one-opt:<radio>paint</button><radio>paint"),
            "paint"
        );
    }

    #[test]
    fn test_strip_display_one_opt() {
        assert_eq!(strip_display("<one-opt></one-opt>value"), "value");
    }

    #[test]
    fn test_strip_display_many_opt() {
        assert_eq!(strip_display("<many-opt></many-opt>value"), "value");
    }

    #[test]
    fn test_strip_display_unescape() {
        assert_eq!(strip_display("text with \\<brackets\\>"), "text with <brackets>");
    }

    #[test]
    fn test_strip_display_escaped_tag_not_stripped() {
        // \<input> is escaped — should not be treated as a tag
        let result = strip_display("\\<input>text\\</input>");
        assert_eq!(result, "<input>text</input>");
    }

    // --- escape handling in has_* ---

    #[test]
    fn test_escaped_input_not_recognized() {
        assert!(!has_input("\\<input>hello\\</input>"));
    }

    #[test]
    fn test_escaped_link_not_recognized() {
        assert!(!has_link("\\<link>url\\</link>"));
    }

    // --- empty / null-equivalent edge cases ---

    #[test]
    fn test_has_input_empty() {
        assert!(!has_input(""));
    }

    #[test]
    fn test_strip_display_empty() {
        assert_eq!(strip_display(""), "");
    }

    #[test]
    fn test_has_radio_empty() {
        assert!(!has_radio(""));
    }

    #[test]
    fn test_extract_radio_empty() {
        assert_eq!(extract_radio(""), None);
    }

    #[test]
    fn test_has_checked_empty() {
        assert!(!has_checked(""));
    }

    #[test]
    fn test_extract_checked_with_close() {
        assert_eq!(extract_checked("<checked>dark</checked>"), Some("dark".to_owned()));
    }

    #[test]
    fn test_extract_checked_empty() {
        assert_eq!(extract_checked(""), None);
    }

    #[test]
    fn test_has_checkbox_empty() {
        assert!(!has_checkbox(""));
    }

    #[test]
    fn test_has_checkbox_checked_empty() {
        assert!(!has_checkbox_checked(""));
    }

    #[test]
    fn test_has_link_empty() {
        assert!(!has_link(""));
    }

    #[test]
    fn test_extract_link_no_tags() {
        assert_eq!(extract_link("no tags"), None);
    }

    #[test]
    fn test_has_image_empty() {
        assert!(!has_image(""));
    }

    #[test]
    fn test_extract_image_no_tags() {
        assert_eq!(extract_image("no image tags"), None);
    }

    #[test]
    fn test_has_many_opt_empty() {
        assert!(!has_many_opt(""));
    }

    #[test]
    fn test_has_one_opt_false() {
        assert!(!has_one_opt("no tag"));
    }

    #[test]
    fn test_has_one_opt_empty() {
        assert!(!has_one_opt(""));
    }

    #[test]
    fn test_strip_one_opt_empty() {
        assert_eq!(strip_one_opt(""), "");
    }

    #[test]
    fn test_strip_many_opt_no_tag() {
        assert_eq!(strip_many_opt("no tag"), "no tag");
    }

    #[test]
    fn test_strip_many_opt_empty() {
        assert_eq!(strip_many_opt(""), "");
    }

    #[test]
    fn test_extract_button_function_name_no_tag() {
        assert_eq!(extract_button_function_name("no button"), None);
    }

    #[test]
    fn test_extract_button_display_text_no_tag() {
        assert_eq!(extract_button_display_text("no button"), None);
    }

    // --- opt + nested tag combinations ---

    #[test]
    fn test_strip_display_one_opt_and_input() {
        assert_eq!(
            strip_display("<one-opt></one-opt><input>value</input>"),
            "value"
        );
    }

    #[test]
    fn test_strip_display_many_opt_and_checkbox() {
        assert_eq!(
            strip_display("<many-opt></many-opt><checkbox>task</checkbox>"),
            "task"
        );
    }

    // --- escape handling for remaining tag types ---

    #[test]
    fn test_escaped_radio_not_recognized() {
        assert!(!has_radio("\\<radio>group"));
    }

    #[test]
    fn test_escaped_checked_not_recognized() {
        assert!(!has_checked("\\<checked>option"));
    }

    #[test]
    fn test_escaped_checkbox_not_recognized() {
        assert!(!has_checkbox("\\<checkbox>label"));
    }

    #[test]
    fn test_escaped_checkbox_checked_not_recognized() {
        assert!(!has_checkbox_checked("\\<checkbox checked>label"));
    }

    #[test]
    fn test_escaped_image_not_recognized() {
        assert!(!has_image("\\<image>pic.jpg\\</image>"));
    }

    #[test]
    fn test_escaped_button_not_recognized() {
        assert!(!has_button("\\<button>fn\\</button>text"));
    }

    #[test]
    fn test_strip_display_unescapes_checkbox() {
        assert_eq!(strip_display("\\<checkbox>label"), "<checkbox>label");
    }

    #[test]
    fn test_strip_display_unescapes_mixed() {
        // Real input tag + escaped angle brackets in surrounding text
        assert_eq!(
            strip_display("prefix \\<b\\> <input>editable</input> suffix"),
            "prefix <b> editable suffix"
        );
    }

    // --- <id> tag ---

    #[test]
    fn test_has_id_true() {
        assert!(has_id("<id>section-1</id>Heading"));
    }

    #[test]
    fn test_has_id_false_no_tag() {
        assert!(!has_id("plain text"));
    }

    #[test]
    fn test_has_id_false_missing_close() {
        assert!(!has_id("<id>unclosed"));
    }

    #[test]
    fn test_extract_id() {
        assert_eq!(extract_id("<id>page-main-content</id>Main"), Some("page-main-content".to_owned()));
    }

    #[test]
    fn test_extract_id_no_tags() {
        assert_eq!(extract_id("no id here"), None);
    }

    #[test]
    fn test_format_id() {
        assert_eq!(format_id("section"), "<id>section</id>");
    }

    #[test]
    fn test_strip_display_id_strips_tag_and_content() {
        // <id> content is metadata, not display text — must be stripped entirely
        assert_eq!(strip_display("<id>page-main-content</id>Main heading"), "Main heading");
    }

    #[test]
    fn test_strip_display_id_with_link() {
        // id tag stripped, then link tag processed normally
        assert_eq!(
            strip_display("<id>foo</id>text <link>#foo</link>"),
            "text #foo"
        );
    }

    #[test]
    fn test_strip_display_id_empty_content() {
        assert_eq!(strip_display("<id></id>visible"), "visible");
    }

    #[test]
    fn test_escaped_id_not_recognized() {
        assert!(!has_id("\\<id>section\\</id>"));
    }
}
