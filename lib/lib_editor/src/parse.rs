use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::tags;

/// Parse file contents into a FFON element tree, dispatching on file extension.
///
/// - `.py` / `.pyw` / `.pyi` → indent-based Python blocks (`line:` opens a block)
/// - C-family (`.c`, `.h`, `.rs`, `.js`, `.ts`, …) → brace-based blocks (`line {` opens, `}` closes)
/// - Everything else → generic FFON rules (`:` suffix + `{ }` blocks)
///
/// Every produced element is annotated with `<src=N>` where N is the 0-based
/// source-line index in the original file. The annotation is invisible in the UI
/// (stripped by `strip_display`) but lets `commit_edit` map each FFON element
/// back to the exact line that must be modified on disk.
pub fn parse_file_ext(contents: &str, ext: &str) -> Vec<FfonElement> {
    match ext.to_ascii_lowercase().as_str() {
        "py" | "pyw" | "pyi" => parse_python(contents),
        "c" | "h" | "cpp" | "cxx" | "cc" | "hpp" | "hxx"
        | "js" | "jsx" | "mjs" | "cjs"
        | "ts" | "tsx"
        | "java" | "go" | "cs" | "swift" | "kt" | "kts"
        | "rs" | "php" | "css" | "scss" | "less" => parse_cbrace(contents),
        _ => parse_file(contents),
    }
}

/// Parse using the original FFON rules (`:` suffix + `{ }` blocks).
pub fn parse_file(contents: &str) -> Vec<FfonElement> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;
    parse_ffon_block(&lines, &mut i, false)
}

/// Navigate a FFON sub-path (sequence of Obj keys) within a pre-parsed tree.
///
/// Obj keys carry a `<src=N>` prefix; the path segments are plain (stripped) key
/// names, so we strip the annotation before comparing.
pub fn navigate_path<'a>(elements: &'a [FfonElement], path: &[String]) -> &'a [FfonElement] {
    if path.is_empty() {
        return elements;
    }
    for elem in elements {
        if let Some(obj) = elem.as_obj() {
            // Strip all display tags (<input>, <src=N>, etc.) before comparing
            // with the stored path segment (which is already stripped by push_path).
            let key_plain = tags::strip_display(&obj.key);
            if key_plain == path[0] {
                return navigate_path(&obj.children, &path[1..]);
            }
        }
    }
    &[]
}

// ---------------------------------------------------------------------------
// FFON parser (generic / .ffon files)
// ---------------------------------------------------------------------------

fn parse_ffon_block(lines: &[&str], i: &mut usize, inside_braces: bool) -> Vec<FfonElement> {
    let mut result = Vec::new();
    while *i < lines.len() {
        let src_line = *i;
        let line = lines[*i].trim();

        if line == "}" {
            if inside_braces { *i += 1; }
            return result;
        }
        if line.is_empty() { *i += 1; continue; }

        *i += 1;

        if line.ends_with(':') {
            let next_is_brace = *i < lines.len() && lines[*i].trim() == "{";
            let key = format!("{}{}", tags::format_src(src_line), line);
            let children = if next_is_brace {
                *i += 1;
                parse_ffon_block(lines, i, true)
            } else {
                Vec::new()
            };
            if children.is_empty() {
                result.push(FfonElement::new_str(key));
            } else {
                let mut obj = FfonElement::new_obj(&key);
                for child in children {
                    obj.as_obj_mut().unwrap().push(child);
                }
                result.push(obj);
            }
        } else {
            let content = format!("{}{}", tags::format_src(src_line), line);
            result.push(FfonElement::new_str(content));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// C-family brace parser
// ---------------------------------------------------------------------------

fn parse_cbrace(contents: &str) -> Vec<FfonElement> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;
    parse_cbrace_block(&lines, &mut i, true)
}

/// Parse a sequence of C-brace lines.
///
/// `top_level` — when true, stray `}` not matched to an opening `{` are
/// skipped; when false (inside a block body), an unmatched `}` terminates
/// the call without consuming the line so the caller can place it as a
/// sibling of the block's Obj.
fn parse_cbrace_block(lines: &[&str], i: &mut usize, top_level: bool) -> Vec<FfonElement> {
    let mut result = Vec::new();
    while *i < lines.len() {
        let src_line = *i;
        let line = lines[*i].trim();
        if line.is_empty() { *i += 1; continue; }

        if line.starts_with('}') {
            if top_level {
                *i += 1; // skip stray top-level braces
                continue;
            }
            // End of block — return without consuming so the parent can place
            // this closing brace as a sibling of the Obj.
            return result;
        }

        *i += 1;

        if line.ends_with('{') {
            let key = format!("{}{}", tags::format_src(src_line), line);
            let children = parse_cbrace_block(lines, i, false);
            if children.is_empty() {
                result.push(FfonElement::new_str(key));
            } else {
                let mut obj = FfonElement::new_obj(&key);
                for child in children {
                    obj.as_obj_mut().unwrap().push(child);
                }
                result.push(obj);
            }
            // After the body, consume the closing/continuation brace(s) and
            // add them as siblings (one layer to the left of the body).
            // "} else {" / "} catch {" open a new Obj (or Str if its body is
            // empty); pure "}" becomes a Str.
            while *i < lines.len() {
                let cl_src = *i;
                let cl = lines[*i].trim();
                if !cl.starts_with('}') { break; }
                *i += 1;
                if cl.ends_with('{') {
                    let cont_key = format!("{}{}", tags::format_src(cl_src), cl);
                    let cont_children = parse_cbrace_block(lines, i, false);
                    if cont_children.is_empty() {
                        result.push(FfonElement::new_str(cont_key));
                    } else {
                        let mut cont = FfonElement::new_obj(&cont_key);
                        for child in cont_children {
                            cont.as_obj_mut().unwrap().push(child);
                        }
                        result.push(cont);
                    }
                } else {
                    result.push(FfonElement::new_str(format!("{}{}", tags::format_src(cl_src), cl)));
                    break;
                }
            }
        } else {
            result.push(FfonElement::new_str(format!("{}{}", tags::format_src(src_line), line)));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Python indent parser
// ---------------------------------------------------------------------------

fn parse_python(contents: &str) -> Vec<FfonElement> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut i = 0;
    parse_python_block(&lines, &mut i, 0)
}

fn measure_indent(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

fn parse_python_block(lines: &[&str], i: &mut usize, base_indent: usize) -> Vec<FfonElement> {
    let mut result = Vec::new();
    while *i < lines.len() {
        let src_line = *i;
        let raw = lines[*i];
        let stripped = raw.trim();
        if stripped.is_empty() { *i += 1; continue; }

        let indent = measure_indent(raw);
        if indent < base_indent {
            // Dedented — return to parent without consuming.
            return result;
        }

        *i += 1;

        if stripped.ends_with(':') {
            // Look ahead (skipping blanks) to find the child indent level.
            let mut j = *i;
            while j < lines.len() && lines[j].trim().is_empty() { j += 1; }
            let child_indent = if j < lines.len() { measure_indent(lines[j]) } else { 0 };

            let children = if child_indent > indent {
                parse_python_block(lines, i, child_indent)
            } else {
                Vec::new()
            };

            let key = format!("{}{}", tags::format_src(src_line), stripped);
            if children.is_empty() {
                result.push(FfonElement::new_str(key));
            } else {
                let mut obj = FfonElement::new_obj(&key);
                for child in children {
                    obj.as_obj_mut().unwrap().push(child);
                }
                result.push(obj);
            }
        } else {
            result.push(FfonElement::new_str(format!("{}{}", tags::format_src(src_line), stripped)));
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

    fn strip_src(s: &str) -> &str {
        tags::extract_src(s).map(|(_, rest)| rest).unwrap_or(s)
    }

    // --- generic FFON ---

    #[test]
    fn parse_file_splits_on_newline() {
        let elements = parse_file("line1\nline2\nline3");
        assert_eq!(elements.len(), 3);
        assert_eq!(strip_src(elements[0].as_str().unwrap()), "line1");
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "line2");
        assert_eq!(strip_src(elements[2].as_str().unwrap()), "line3");
    }

    #[test]
    fn parse_file_skips_blank_lines() {
        let elements = parse_file("a\n\nb");
        assert_eq!(elements.len(), 2);
    }

    #[test]
    fn parse_file_colon_suffix_without_body_is_str() {
        // A bare "section:" with no `{ }` body has nothing to navigate into,
        // so it renders as a Str (`-i`) rather than an empty Obj (`+i`).
        let elements = parse_file("section:");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_str());
        assert_eq!(strip_src(elements[0].as_str().unwrap()), "section:");
    }

    #[test]
    fn parse_file_colon_suffix_with_body_is_obj() {
        let elements = parse_file("section:\n{\n  child\n}");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_obj());
        assert_eq!(strip_src(elements[0].as_obj().unwrap().key.as_str()), "section:");
    }

    #[test]
    fn parse_file_colon_suffix_with_empty_braces_is_str() {
        // `{` followed immediately by `}` is not a real body — the header
        // should still collapse to Str.
        let elements = parse_file("section:\n{\n}");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_str());
    }

    #[test]
    fn cbrace_empty_block_collapses_to_str() {
        // `struct Foo {` with an immediate `}` has no body, so the opener is a
        // Str sibling of the closing brace rather than an empty Obj.
        let elements = parse_file_ext("struct Foo {\n}", "rs");
        assert_eq!(elements.len(), 2);
        assert!(elements[0].is_str(), "empty-body opener must be Str");
        assert_eq!(strip_src(elements[0].as_str().unwrap()), "struct Foo {");
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "}");
    }

    #[test]
    fn python_empty_def_collapses_to_str() {
        // `def foo():` with no indented body is a Str.
        let elements = parse_file_ext("def foo():\nx = 1\n", "py");
        assert_eq!(elements.len(), 2);
        assert!(elements[0].is_str(), "empty-body def must be Str");
        assert_eq!(strip_src(elements[0].as_str().unwrap()), "def foo():");
    }

    #[test]
    fn parse_file_handles_nested_braces() {
        let src = "section:\n{\n  child1\n  child2\n}\nplain";
        let elements = parse_file(src);
        assert_eq!(elements.len(), 2);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(strip_src(&obj.key), "section:");
        assert_eq!(obj.children.len(), 2);
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "plain");
    }

    #[test]
    fn parse_file_handles_deep_nesting() {
        let src = "outer:\n{\n  inner:\n  {\n    deep\n  }\n}";
        let elements = parse_file(src);
        assert_eq!(elements.len(), 1);
        let outer = elements[0].as_obj().unwrap();
        let inner = outer.children[0].as_obj().unwrap();
        assert_eq!(strip_src(&inner.key), "inner:");
        assert_eq!(strip_src(inner.children[0].as_str().unwrap()), "deep");
    }

    #[test]
    fn parse_file_colon_in_value_is_leaf() {
        let elements = parse_file("name: Alice");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_str());
    }

    #[test]
    fn parse_file_src_annotations_correct() {
        let elements = parse_file("alpha\n\nbeta\ngamma");
        // alpha → line 0, blank skipped, beta → line 2, gamma → line 3
        assert_eq!(elements[0].as_str().map(|s| tags::extract_src(s).map(|(n,_)| n)), Some(Some(0)));
        assert_eq!(elements[1].as_str().map(|s| tags::extract_src(s).map(|(n,_)| n)), Some(Some(2)));
        assert_eq!(elements[2].as_str().map(|s| tags::extract_src(s).map(|(n,_)| n)), Some(Some(3)));
    }

    // --- C-brace parser ---

    #[test]
    fn cbrace_function_becomes_obj() {
        let src = "void foo() {\n    return;\n}";
        let elements = parse_file_ext(src, "c");
        // Obj at index 0, closing "}" at index 1 (one layer to the left of the body)
        assert_eq!(elements.len(), 2);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(strip_src(&obj.key), "void foo() {");
        assert_eq!(obj.children.len(), 1);
        assert_eq!(strip_src(obj.children[0].as_str().unwrap()), "return;");
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "}");
    }

    #[test]
    fn cbrace_plain_line_is_leaf() {
        let src = "int x = 1;";
        let elements = parse_file_ext(src, "c");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_str());
    }

    #[test]
    fn cbrace_nested_blocks() {
        let src = "fn outer() {\n    fn inner() {\n        x\n    }\n}";
        let elements = parse_file_ext(src, "rs");
        // outer Obj + outer's closing "}" at top level
        assert_eq!(elements.len(), 2);
        let outer = elements[0].as_obj().unwrap();
        assert_eq!(strip_src(&outer.key), "fn outer() {");
        // inner Obj + inner's closing "}" inside outer's body
        assert_eq!(outer.children.len(), 2);
        let inner = outer.children[0].as_obj().unwrap();
        assert_eq!(strip_src(&inner.key), "fn inner() {");
        assert_eq!(outer.children.len(), 2);
        assert_eq!(strip_src(inner.children[0].as_str().unwrap()), "x");
        assert_eq!(strip_src(outer.children[1].as_str().unwrap()), "}");
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "}");
    }

    #[test]
    fn cbrace_else_block_becomes_sibling() {
        let src = "if (x) {\n    a;\n} else {\n    b;\n}";
        let elements = parse_file_ext(src, "c");
        // if Obj, else Obj (continuation), then the closing "}"
        assert_eq!(elements.len(), 3);
        assert_eq!(strip_src(elements[0].as_obj().unwrap().key.as_str()), "if (x) {");
        assert_eq!(strip_src(elements[1].as_obj().unwrap().key.as_str()), "} else {");
        assert_eq!(strip_src(elements[2].as_str().unwrap()), "}");
    }

    #[test]
    fn cbrace_ts_extension_works() {
        let src = "function foo() {\n  return 1;\n}";
        let elements = parse_file_ext(src, "ts");
        // Obj + closing "}"
        assert_eq!(elements.len(), 2);
        assert!(elements[0].is_obj());
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "}");
    }

    // --- Python indent parser ---

    #[test]
    fn python_function_becomes_obj() {
        let src = "def foo():\n    return 1\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 1);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(strip_src(&obj.key), "def foo():");
        assert_eq!(obj.children.len(), 1);
        assert_eq!(strip_src(obj.children[0].as_str().unwrap()), "return 1");
    }

    #[test]
    fn python_class_with_methods() {
        let src = "class Foo:\n    def bar(self):\n        pass\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 1);
        let cls = elements[0].as_obj().unwrap();
        assert_eq!(strip_src(&cls.key), "class Foo:");
        assert_eq!(cls.children.len(), 1);
        let method = cls.children[0].as_obj().unwrap();
        assert_eq!(strip_src(&method.key), "def bar(self):");
        assert_eq!(strip_src(method.children[0].as_str().unwrap()), "pass");
    }

    #[test]
    fn python_dedent_ends_block() {
        let src = "def foo():\n    x = 1\ntop = 2\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 2);
        assert!(elements[0].is_obj());
        assert_eq!(strip_src(elements[1].as_str().unwrap()), "top = 2");
    }

    #[test]
    fn python_else_becomes_sibling() {
        let src = "if True:\n    a = 1\nelse:\n    b = 2\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 2);
        assert_eq!(strip_src(elements[0].as_obj().unwrap().key.as_str()), "if True:");
        assert_eq!(strip_src(elements[1].as_obj().unwrap().key.as_str()), "else:");
    }

    // --- navigate_path with src annotations ---

    #[test]
    fn navigate_path_empty_returns_all() {
        let elems = parse_file("a\nb");
        assert_eq!(navigate_path(&elems, &[]).len(), 2);
    }

    #[test]
    fn navigate_path_finds_section_children() {
        let src = "section:\n{\n  child\n}";
        let elems = parse_file(src);
        // Path uses plain key (no <src=N> prefix) — navigate_path strips it internally.
        let result = navigate_path(&elems, &["section:".to_string()]);
        assert_eq!(result.len(), 1);
        assert_eq!(strip_src(result[0].as_str().unwrap()), "child");
    }

    #[test]
    fn navigate_path_unknown_returns_empty() {
        let elems = parse_file("line");
        assert!(navigate_path(&elems, &["nonexistent:".to_string()]).is_empty());
    }
}
