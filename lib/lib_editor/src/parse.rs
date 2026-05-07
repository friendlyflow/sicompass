use sicompass_sdk::ffon::FfonElement;

/// Parse file contents into a FFON element tree.
///
/// Parsing rules (per PLAN.md: elements delimited by `\n`, `{`, `:`):
/// - Lines separated by `\n` are siblings.
/// - A line ending with `:` is a section header (`FfonElement::Obj`).  Its
///   children are the lines inside the immediately following `{ … }` block (if
///   any).
/// - A bare `{` / `}` pair groups children for the preceding section header.
/// - Blank lines are skipped.
/// - All other lines become `FfonElement::Str` leaves.
pub fn parse_file(contents: &str) -> Vec<FfonElement> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;
    parse_block(&lines, &mut i, false)
}

/// Navigate a FFON sub-path (sequence of Obj keys) within a pre-parsed tree.
///
/// Returns the children of the element reached by following `path`, or an
/// empty slice if the path cannot be resolved.
pub fn navigate_path<'a>(elements: &'a [FfonElement], path: &[String]) -> &'a [FfonElement] {
    if path.is_empty() {
        return elements;
    }
    for elem in elements {
        if let Some(obj) = elem.as_obj() {
            if obj.key == path[0] {
                return navigate_path(&obj.children, &path[1..]);
            }
        }
    }
    &[]
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

fn parse_block(lines: &[&str], i: &mut usize, inside_braces: bool) -> Vec<FfonElement> {
    let mut result = Vec::new();
    while *i < lines.len() {
        let raw = lines[*i];
        let line = raw.trim();

        if line == "}" {
            if inside_braces {
                *i += 1;
            }
            return result;
        }

        if line.is_empty() {
            *i += 1;
            continue;
        }

        *i += 1;

        // A section header ends with ':' and is followed (optionally) by '{'.
        if line.ends_with(':') {
            let next_is_brace = *i < lines.len() && lines[*i].trim() == "{";
            let mut obj = FfonElement::new_obj(line);
            if next_is_brace {
                *i += 1; // consume the '{'
                let children = parse_block(lines, i, true);
                if let Some(o) = obj.as_obj_mut() {
                    for child in children {
                        o.push(child);
                    }
                }
            }
            result.push(obj);
        } else {
            result.push(FfonElement::new_str(line));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_splits_on_newline() {
        let elements = parse_file("line1\nline2\nline3");
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0], FfonElement::new_str("line1"));
        assert_eq!(elements[1], FfonElement::new_str("line2"));
        assert_eq!(elements[2], FfonElement::new_str("line3"));
    }

    #[test]
    fn parse_file_skips_blank_lines() {
        let elements = parse_file("a\n\nb");
        assert_eq!(elements.len(), 2);
    }

    #[test]
    fn parse_file_colon_suffix_makes_obj() {
        let elements = parse_file("section:");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_obj());
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(obj.key, "section:");
        assert!(obj.children.is_empty());
    }

    #[test]
    fn parse_file_handles_nested_braces() {
        let src = "section:\n{\n  child1\n  child2\n}\nplain";
        let elements = parse_file(src);
        assert_eq!(elements.len(), 2);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(obj.key, "section:");
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0], FfonElement::new_str("child1"));
        assert_eq!(obj.children[1], FfonElement::new_str("child2"));
        assert_eq!(elements[1], FfonElement::new_str("plain"));
    }

    #[test]
    fn parse_file_handles_deep_nesting() {
        let src = "outer:\n{\n  inner:\n  {\n    deep\n  }\n}";
        let elements = parse_file(src);
        assert_eq!(elements.len(), 1);
        let outer = elements[0].as_obj().unwrap();
        assert_eq!(outer.key, "outer:");
        assert_eq!(outer.children.len(), 1);
        let inner = outer.children[0].as_obj().unwrap();
        assert_eq!(inner.key, "inner:");
        assert_eq!(inner.children.len(), 1);
        assert_eq!(inner.children[0], FfonElement::new_str("deep"));
    }

    #[test]
    fn parse_file_colon_in_value_is_leaf() {
        // "key: value" ends with " value", not ':', so it is a Str leaf.
        let elements = parse_file("name: Alice");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_str());
    }

    #[test]
    fn navigate_path_empty_returns_all() {
        let elems = parse_file("a\nb");
        let result = navigate_path(&elems, &[]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn navigate_path_finds_section_children() {
        let src = "section:\n{\n  child\n}";
        let elems = parse_file(src);
        let result = navigate_path(&elems, &["section:".to_string()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], FfonElement::new_str("child"));
    }

    #[test]
    fn navigate_path_unknown_returns_empty() {
        let elems = parse_file("line");
        let result = navigate_path(&elems, &["nonexistent:".to_string()]);
        assert!(result.is_empty());
    }
}
