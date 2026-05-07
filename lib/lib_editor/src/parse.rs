use sicompass_sdk::ffon::FfonElement;

/// Parse file contents into a FFON element tree, dispatching on file extension.
///
/// - `.py` / `.pyw` / `.pyi` → indent-based Python blocks (`line:` opens a block)
/// - C-family (`.c`, `.h`, `.rs`, `.js`, `.ts`, …) → brace-based blocks (`line {` opens, `}` closes)
/// - Everything else → generic FFON rules (`:` suffix + `{ }` blocks)
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
// FFON parser (generic / .ffon files)
// ---------------------------------------------------------------------------

fn parse_ffon_block(lines: &[&str], i: &mut usize, inside_braces: bool) -> Vec<FfonElement> {
    let mut result = Vec::new();
    while *i < lines.len() {
        let line = lines[*i].trim();

        if line == "}" {
            if inside_braces { *i += 1; }
            return result;
        }
        if line.is_empty() { *i += 1; continue; }

        *i += 1;

        if line.ends_with(':') {
            let next_is_brace = *i < lines.len() && lines[*i].trim() == "{";
            let mut obj = FfonElement::new_obj(line);
            if next_is_brace {
                *i += 1;
                let children = parse_ffon_block(lines, i, true);
                for child in children {
                    obj.as_obj_mut().unwrap().push(child);
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
            let mut obj = FfonElement::new_obj(line);
            let children = parse_cbrace_block(lines, i, false);
            for child in children {
                obj.as_obj_mut().unwrap().push(child);
            }
            result.push(obj);
            // After the body, consume the closing/continuation brace(s) and
            // add them as siblings (one layer to the left of the body).
            // "} else {" / "} catch {" open a new Obj; pure "}" becomes a Str.
            while *i < lines.len() {
                let cl = lines[*i].trim();
                if !cl.starts_with('}') { break; }
                *i += 1;
                if cl.ends_with('{') {
                    // Continuation block: recurse, then loop for its closing }.
                    let mut cont = FfonElement::new_obj(cl);
                    let cont_children = parse_cbrace_block(lines, i, false);
                    for child in cont_children {
                        cont.as_obj_mut().unwrap().push(child);
                    }
                    result.push(cont);
                } else {
                    result.push(FfonElement::new_str(cl));
                    break;
                }
            }
        } else {
            result.push(FfonElement::new_str(line));
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

            let mut obj = FfonElement::new_obj(stripped);
            for child in children {
                obj.as_obj_mut().unwrap().push(child);
            }
            result.push(obj);
        } else {
            result.push(FfonElement::new_str(stripped));
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

    // --- generic FFON ---

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
        assert_eq!(elements[0].as_obj().unwrap().key, "section:");
    }

    #[test]
    fn parse_file_handles_nested_braces() {
        let src = "section:\n{\n  child1\n  child2\n}\nplain";
        let elements = parse_file(src);
        assert_eq!(elements.len(), 2);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(obj.key, "section:");
        assert_eq!(obj.children.len(), 2);
        assert_eq!(elements[1], FfonElement::new_str("plain"));
    }

    #[test]
    fn parse_file_handles_deep_nesting() {
        let src = "outer:\n{\n  inner:\n  {\n    deep\n  }\n}";
        let elements = parse_file(src);
        assert_eq!(elements.len(), 1);
        let outer = elements[0].as_obj().unwrap();
        let inner = outer.children[0].as_obj().unwrap();
        assert_eq!(inner.children[0], FfonElement::new_str("deep"));
    }

    #[test]
    fn parse_file_colon_in_value_is_leaf() {
        let elements = parse_file("name: Alice");
        assert_eq!(elements.len(), 1);
        assert!(elements[0].is_str());
    }

    // --- C-brace parser ---

    #[test]
    fn cbrace_function_becomes_obj() {
        let src = "void foo() {\n    return;\n}";
        let elements = parse_file_ext(src, "c");
        // Obj at index 0, closing "}" at index 1 (one layer to the left of the body)
        assert_eq!(elements.len(), 2);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(obj.key, "void foo() {");
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0], FfonElement::new_str("return;"));
        assert_eq!(elements[1], FfonElement::new_str("}"));
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
        assert_eq!(outer.key, "fn outer() {");
        // inner Obj + inner's closing "}" inside outer's body
        assert_eq!(outer.children.len(), 2);
        let inner = outer.children[0].as_obj().unwrap();
        assert_eq!(inner.key, "fn inner() {");
        assert_eq!(inner.children.len(), 1);
        assert_eq!(inner.children[0], FfonElement::new_str("x"));
        assert_eq!(outer.children[1], FfonElement::new_str("}"));
        assert_eq!(elements[1], FfonElement::new_str("}"));
    }

    #[test]
    fn cbrace_else_block_becomes_sibling() {
        let src = "if (x) {\n    a;\n} else {\n    b;\n}";
        let elements = parse_file_ext(src, "c");
        // if Obj, else Obj (continuation), then the closing "}"
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0].as_obj().unwrap().key, "if (x) {");
        assert_eq!(elements[1].as_obj().unwrap().key, "} else {");
        assert_eq!(elements[2], FfonElement::new_str("}"));
    }

    #[test]
    fn cbrace_ts_extension_works() {
        let src = "function foo() {\n  return 1;\n}";
        let elements = parse_file_ext(src, "ts");
        // Obj + closing "}"
        assert_eq!(elements.len(), 2);
        assert!(elements[0].is_obj());
        assert_eq!(elements[1], FfonElement::new_str("}"));
    }

    // --- Python indent parser ---

    #[test]
    fn python_function_becomes_obj() {
        let src = "def foo():\n    return 1\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 1);
        let obj = elements[0].as_obj().unwrap();
        assert_eq!(obj.key, "def foo():");
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0], FfonElement::new_str("return 1"));
    }

    #[test]
    fn python_class_with_methods() {
        let src = "class Foo:\n    def bar(self):\n        pass\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 1);
        let cls = elements[0].as_obj().unwrap();
        assert_eq!(cls.key, "class Foo:");
        assert_eq!(cls.children.len(), 1);
        let method = cls.children[0].as_obj().unwrap();
        assert_eq!(method.key, "def bar(self):");
        assert_eq!(method.children[0], FfonElement::new_str("pass"));
    }

    #[test]
    fn python_dedent_ends_block() {
        let src = "def foo():\n    x = 1\ntop = 2\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 2);
        assert!(elements[0].is_obj());
        assert_eq!(elements[1], FfonElement::new_str("top = 2"));
    }

    #[test]
    fn python_else_becomes_sibling() {
        let src = "if True:\n    a = 1\nelse:\n    b = 2\n";
        let elements = parse_file_ext(src, "py");
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].as_obj().unwrap().key, "if True:");
        assert_eq!(elements[1].as_obj().unwrap().key, "else:");
    }

    // --- navigate_path ---

    #[test]
    fn navigate_path_empty_returns_all() {
        let elems = parse_file("a\nb");
        assert_eq!(navigate_path(&elems, &[]).len(), 2);
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
        assert!(navigate_path(&elems, &["nonexistent:".to_string()]).is_empty());
    }
}
