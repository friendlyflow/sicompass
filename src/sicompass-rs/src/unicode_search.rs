//! NFC-normalised, case-folded search. Equivalent to `unicode_search.c`.

use unicode_normalization::UnicodeNormalization;

pub fn contains_normalised(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() { return true; }
    let h: String = haystack.nfc().collect::<String>().to_lowercase();
    let n: String = needle.nfc().collect::<String>().to_lowercase();
    h.contains(&n)
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
}
