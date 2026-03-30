//! NFC-normalised, case-folded search. Equivalent to `unicode_search.c`.

use unicode_normalization::UnicodeNormalization;

pub fn contains_normalised(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() { return true; }
    let h: String = haystack.nfc().collect::<String>().to_lowercase();
    let n: String = needle.nfc().collect::<String>().to_lowercase();
    h.contains(&n)
}

/// Returns the byte position in `haystack` where `needle` first matches
/// (NFC-normalised, case-folded), or `None` if there is no match.
///
/// The byte position corresponds to the offset in the NFC-normalised lowercase
/// version of `haystack`.  For pre-composed (already-NFC) input where
/// upper/lower-case variants have the same UTF-8 byte length (the common case
/// for Latin, Cyrillic, Greek, etc.) this equals the byte offset in the
/// original string.
pub fn find_normalised_byte_pos(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() { return Some(0); }
    let h_norm: String = haystack.nfc().collect::<String>().to_lowercase();
    let n_norm: String = needle.nfc().collect::<String>().to_lowercase();
    h_norm.find(&n_norm)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn empty_needle() { assert!(contains_normalised("anything", "")); }
    #[test] fn ascii_match() { assert!(contains_normalised("hello world", "world")); }
    #[test] fn case_insensitive() { assert!(contains_normalised("Hello", "hello")); }
    #[test] fn no_match() { assert!(!contains_normalised("hello", "xyz")); }
    #[test] fn nfc_roundtrip() {
        assert!(contains_normalised("\u{00e9}", "e\u{0301}"));
        assert!(contains_normalised("e\u{0301}", "\u{00e9}"));
    }

    #[test] fn uppercase_needle() { assert!(contains_normalised("hello world", "HELLO")); }
    #[test] fn exact_match() { assert!(contains_normalised("hello", "hello")); }
    #[test] fn unicode_case_fold() {
        // "CAFÉ" should match "café"
        assert!(contains_normalised("café", "CAFÉ"));
    }
    #[test] fn unicode_accent_resume() {
        assert!(contains_normalised("Résumé", "résumé"));
    }
    #[test] fn partial_no_match() {
        // needle longer than haystack
        assert!(!contains_normalised("hel", "hello"));
    }

    // --- find_normalised_byte_pos ---

    #[test]
    fn pos_empty_needle_returns_zero() {
        assert_eq!(find_normalised_byte_pos("hello", ""), Some(0));
    }

    #[test]
    fn pos_at_start() {
        assert_eq!(find_normalised_byte_pos("Hello World", "hello"), Some(0));
    }

    #[test]
    fn pos_in_middle() {
        // "Hello " = 6 bytes → "world" starts at byte 6
        assert_eq!(find_normalised_byte_pos("Hello World", "world"), Some(6));
    }

    #[test]
    fn pos_no_match() {
        assert_eq!(find_normalised_byte_pos("hello", "xyz"), None);
    }

    #[test]
    fn pos_unicode_at_start() {
        // "café" starts at byte 0
        assert_eq!(find_normalised_byte_pos("Café Latte", "café"), Some(0));
    }

    #[test]
    fn pos_unicode_in_middle() {
        // "My " = 3 bytes, "Café" starts at byte 3
        assert_eq!(find_normalised_byte_pos("My Café", "café"), Some(3));
    }

    #[test]
    fn pos_case_fold_unicode() {
        assert_eq!(find_normalised_byte_pos("RÉSUMÉ", "résumé"), Some(0));
    }
}
