//! Neutralising angle brackets in git output before it becomes FFON.
//!
//! Every interactive element in this app is an HTML-like tag inside a string:
//! `<input>v</input>` is an editable field, `<button>fn</button>Label` is a
//! button whose Enter calls `on_button_press("fn")`, and `strip_display`
//! interprets them **anywhere** in a `Str` payload or an `Obj` key.
//!
//! A git client renders other people's source code: diff hunks of HTML, JSX and
//! Rust generics, plus commit subjects and branch names that can contain
//! anything. Unescaped, a diff line
//!
//! ```text
//! +   <button>submit</button>Send
//! ```
//!
//! renders as a live button, and pressing Enter on it calls into the provider
//! with the function name `submit`. `+ <input type="text">` becomes a field the
//! user can type into, and (because `path_is_filesystem()` is true, so
//! `element_nav_name` prefers `<input>` content over `strip_display`) an `Obj`
//! key containing one would push the *input's* text as the path segment.
//!
//! So every git-derived string is routed through [`escape_markup`] on its way
//! into the tree. The escape is `\<` / `\>`, which the SDK's tag scanner skips
//! and its renderer unescapes again for display.

/// Escape `<` and `>` so the SDK's tag parser cannot see a tag.
///
/// The backslash form is what `tags::find_unescaped` skips and `tags::unescape`
/// undoes at render time, so the user still reads the original characters.
pub fn escape_markup(s: &str) -> String {
    if !s.contains('<') && !s.contains('>') {
        // The overwhelmingly common case: hand back an unchanged copy without
        // walking the string a second time.
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

#[cfg(test)]
mod tests {
    use super::*;
    use sicompass_sdk::tags;

    /// The whole point: what goes in is what the user reads back out, with no
    /// tag surviving in between.
    fn round_trip(raw: &str) {
        let escaped = escape_markup(raw);
        assert!(
            !tags::has_input(&escaped),
            "{raw:?} still parses as an input"
        );
        assert!(
            !tags::has_button(&escaped),
            "{raw:?} still parses as a button"
        );
        assert!(
            !tags::has_checkbox(&escaped),
            "{raw:?} still parses as a checkbox"
        );
        assert_eq!(tags::strip_display(&escaped), raw, "display text changed");
    }

    #[test]
    fn a_plain_line_is_untouched() {
        assert_eq!(escape_markup("+ let x = 1;"), "+ let x = 1;");
    }

    #[test]
    fn an_input_tag_in_a_diff_line_is_neutralised() {
        round_trip("+   <input type=\"text\">");
    }

    #[test]
    fn a_button_tag_in_a_diff_line_is_neutralised() {
        round_trip("+   <button>submit</button>Send");
    }

    #[test]
    fn a_checkbox_tag_in_a_diff_line_is_neutralised() {
        round_trip("-   <checkbox checked>agree</checkbox>");
    }

    #[test]
    fn rust_generics_survive_intact() {
        round_trip("pub fn f(v: Vec<Vec<u8>>) -> Option<&str> { None }");
    }

    #[test]
    fn comparison_operators_survive_intact() {
        round_trip("if a < b && c > d {");
    }

    #[test]
    fn a_commit_subject_with_an_author_address_survives() {
        round_trip("fix: reported by <nico@example.com>");
    }

    #[test]
    fn a_filename_with_angle_brackets_survives() {
        round_trip("src/<generated>/mod.rs");
    }

    #[test]
    fn an_already_escaped_backslash_is_not_special() {
        // A literal backslash in source code is not markup and must come back
        // unchanged, or diffs of escape sequences would read wrong.
        round_trip("let s = \"a\\nb\";");
    }
}
