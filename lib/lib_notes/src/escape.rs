//! Neutralising angle brackets in note text before it becomes FFON.
//!
//! Every interactive element in this app is an HTML-like tag inside a string:
//! `<input>v</input>` is an editable field, `<button>fn</button>Label` is a
//! button whose Enter calls `on_button_press("fn")`, and the tag scanner reads
//! them **anywhere** in a `Str` payload or an `Obj` key.
//!
//! Note text is the most user-controlled string in the whole app — it is
//! whatever the user typed, saved verbatim, and rendered back. A note reading
//!
//! ```text
//! remember: <button>submit</button>Send
//! ```
//!
//! would come back as a live button. So note text is escaped on its way into a
//! label and unescaped on its way back out.
//!
//! The escape is `\<` / `\>`, which the SDK's tag scanner skips and its
//! renderer undoes for display, so the user still reads the characters they
//! typed. Same approach as `lib_gitclient`, for the same reason.

/// Escape `<` and `>` so the tag parser cannot see a tag.
pub fn escape(s: &str) -> String {
    if !s.contains('<') && !s.contains('>') {
        // The overwhelmingly common case: no second walk of the string.
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '<' => out.push_str("\\<"),
            '>' => out.push_str("\\>"),
            other => out.push(other),
        }
    }
    out
}

/// Undo [`escape`].
///
/// Safe on text that was never escaped, which matters because a row coming back
/// from the app is either one this provider rendered (escaped) or one the user
/// just typed (not). Text with no backslash passes through untouched.
pub fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('<') | Some('>') => {
                    out.push(chars.next().unwrap());
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sicompass_sdk::tags;

    /// The point of the module: what the user typed is what they read back, and
    /// no tag survives in between.
    fn round_trip(raw: &str) {
        let escaped = escape(raw);
        assert!(!tags::has_input(&escaped), "{raw:?} still parses as input");
        assert!(
            !tags::has_button(&escaped),
            "{raw:?} still parses as button"
        );
        assert!(!tags::has_link(&escaped), "{raw:?} still parses as a link");
        assert!(!tags::has_id(&escaped), "{raw:?} still parses as an id");
        assert_eq!(unescape(&escaped), raw);
    }

    #[test]
    fn a_note_that_looks_like_a_button_stays_text() {
        round_trip("<button>submit</button>Send");
    }

    #[test]
    fn a_note_that_looks_like_an_input_stays_text() {
        round_trip("<input>type here</input>");
    }

    #[test]
    fn a_note_that_looks_like_an_id_cannot_forge_one() {
        round_trip("<id>99</id>not my id");
    }

    #[test]
    fn ordinary_text_is_untouched() {
        assert_eq!(escape("milk and bread"), "milk and bread");
        assert_eq!(unescape("milk and bread"), "milk and bread");
    }

    #[test]
    fn a_lone_backslash_survives_a_round_trip() {
        round_trip(r"C:\notes");
        assert_eq!(unescape(r"C:\notes"), r"C:\notes");
    }

    #[test]
    fn comparison_operators_survive() {
        round_trip("a < b > c");
    }
}
