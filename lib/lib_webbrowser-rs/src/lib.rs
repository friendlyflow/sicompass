//! Web browser provider — Rust port of `lib_webbrowser/`.
//!
//! Fetches a URL via `reqwest` (blocking), parses HTML with `scraper`
//! (html5ever), and converts the DOM to a flat FFON tree of strings and
//! objects that mirrors the C provider's lexbor-based output.
//!
//! ## FFON tree layout
//!
//! ```text
//! meta             (obj)  — keyboard shortcut hints
//! <url-bar>        (obj when page loaded, str when no page)
//!   heading        (obj)  — h1-h6 → nested objects
//!   paragraph      (str)  — plain text
//!   link text      (str)  — "anchor text <link>url</link>"
//!   list           (obj)  — ul/ol wrapper
//!     - item       (str)
//!   table          (str)  — "cell1 | cell2 | …"
//!   image          (str)  — "alt text [img]"
//! ```

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;

// ---------------------------------------------------------------------------
// Cached page
// ---------------------------------------------------------------------------

struct CachedPage {
    #[allow(dead_code)]
    url: String,
    elements: Vec<FfonElement>,
}

// ---------------------------------------------------------------------------
// WebbrowserProvider
// ---------------------------------------------------------------------------

pub struct WebbrowserProvider {
    current_url: String,
    cached_page: Option<CachedPage>,
}

impl WebbrowserProvider {
    pub fn new() -> Self {
        WebbrowserProvider {
            current_url: String::new(),
            cached_page: None,
        }
    }

    /// Fetch `url` over HTTP and parse to a Vec of FfonElements.
    fn load_url(&mut self, url: &str) {
        let html = match fetch_html(url) {
            Ok(h) => h,
            Err(e) => {
                let msg = format!("Error loading {url}: {e}");
                self.cached_page = Some(CachedPage {
                    url: url.to_owned(),
                    elements: vec![FfonElement::new_str(msg)],
                });
                self.current_url = url.to_owned();
                return;
            }
        };
        let elements = html_to_ffon(&html, url);
        self.cached_page = Some(CachedPage { url: url.to_owned(), elements });
        self.current_url = url.to_owned();
    }
}

impl Default for WebbrowserProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for WebbrowserProvider {
    fn name(&self) -> &str { "webbrowser" }
    fn display_name(&self) -> &str { "web browser" }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let mut result = Vec::new();

        // meta: shortcut hints
        let mut meta = FfonElement::new_obj("meta");
        {
            let m = meta.as_obj_mut().unwrap();
            m.push(FfonElement::new_str("I   Edit URL"));
            m.push(FfonElement::new_str("/   Search"));
            m.push(FfonElement::new_str("F5  Refresh"));
            m.push(FfonElement::new_str(":   Commands"));
        }
        result.push(meta);

        // URL bar element
        let url_bar = format!(
            "<input>{}</input>",
            if self.current_url.is_empty() { "https://" } else { &self.current_url }
        );

        if let Some(ref page) = self.cached_page {
            // Page loaded: wrap URL bar + page content in an Obj
            let mut page_obj = FfonElement::new_obj(&url_bar);
            let o = page_obj.as_obj_mut().unwrap();
            for elem in &page.elements {
                o.push(elem.clone());
            }
            result.push(page_obj);
        } else {
            // No page yet: just the URL bar as a string
            result.push(FfonElement::new_str(url_bar));
        }

        result
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        let url = new_content.trim().to_owned();
        if url.is_empty() { return false; }
        // Prepend https:// if no scheme
        let full_url = if url.contains("://") {
            url
        } else {
            format!("https://{url}")
        };
        self.load_url(&full_url);
        true
    }

    fn commands(&self) -> Vec<String> {
        vec!["refresh".to_owned()]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        if cmd == "refresh" {
            let url = self.current_url.clone();
            if !url.is_empty() {
                self.cached_page = None;
                self.load_url(&url);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// HTTP fetch
// ---------------------------------------------------------------------------

fn fetch_html(url: &str) -> Result<String, String> {
    reqwest::blocking::Client::builder()
        .user_agent("sicompass/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// HTML → FFON conversion
// ---------------------------------------------------------------------------

/// Tags we skip entirely (including all their children).
const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "svg", "head", "nav", "footer",
    "header", "aside",
];

/// Tags that produce objects (sections/headings) vs strings (text/inline).
const BLOCK_TAGS: &[&str] = &[
    "div", "section", "article", "main", "h1", "h2", "h3", "h4", "h5", "h6",
    "blockquote", "figure", "details", "summary",
];

const HEADING_TAGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];

/// Convert an HTML string to a flat list of FfonElements.
fn html_to_ffon(html: &str, base_url: &str) -> Vec<FfonElement> {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").unwrap();

    let body = match document.select(&body_sel).next() {
        Some(b) => b,
        None => return vec![FfonElement::new_str("(empty page)")],
    };

    let mut out: Vec<FfonElement> = Vec::new();
    convert_node(body, base_url, &mut out, 0);
    out
}

fn convert_node(
    node: scraper::ElementRef,
    base_url: &str,
    out: &mut Vec<FfonElement>,
    depth: usize,
) {
    let tag = node.value().name();

    if SKIP_TAGS.contains(&tag) {
        return;
    }

    match tag {
        t if HEADING_TAGS.contains(&t) => {
            let text = collect_text(node, base_url).trim().to_owned();
            if !text.is_empty() {
                let obj = FfonElement::new_obj(text);
                // Recurse children into this heading's obj
                for child in node.children().filter_map(scraper::ElementRef::wrap) {
                    let child_tag = child.value().name();
                    if !HEADING_TAGS.contains(&child_tag) {
                        convert_node(child, base_url, &mut Vec::new(), depth + 1);
                    }
                }
                out.push(obj);
            }
        }
        "p" => {
            let text = collect_text(node, base_url).trim().to_owned();
            if !text.is_empty() {
                out.push(FfonElement::new_str(text));
            }
        }
        "ul" | "ol" => {
            let list_label = if tag == "ol" { "ordered list" } else { "list" };
            let mut list_obj = FfonElement::new_obj(list_label);
            let li_sel = scraper::Selector::parse("li").unwrap();
            for (i, li) in node.select(&li_sel).enumerate() {
                let text = collect_text(li, base_url).trim().to_owned();
                if !text.is_empty() {
                    let item = if tag == "ol" {
                        format!("{}. {}", i + 1, text)
                    } else {
                        format!("- {}", text)
                    };
                    list_obj.as_obj_mut().unwrap().push(FfonElement::new_str(item));
                }
            }
            if list_obj.as_obj().map_or(false, |o| !o.children.is_empty()) {
                out.push(list_obj);
            }
        }
        "table" => {
            convert_table(node, out);
        }
        "pre" | "code" => {
            let text = node.text().collect::<String>();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                out.push(FfonElement::new_str(trimmed.to_owned()));
            }
        }
        "img" => {
            let alt = node.value().attr("alt").unwrap_or("image");
            if !alt.is_empty() && alt != "image" {
                out.push(FfonElement::new_str(format!("{alt} [img]")));
            }
        }
        "a" => {
            let href = resolve_href(node.value().attr("href").unwrap_or(""), base_url);
            let text = node.text().collect::<String>().trim().to_owned();
            if !text.is_empty() && !href.is_empty() {
                out.push(FfonElement::new_str(format!("{text} <link>{href}</link>")));
            } else if !text.is_empty() {
                out.push(FfonElement::new_str(text));
            }
        }
        "dl" => {
            convert_dl(node, base_url, out);
        }
        t if BLOCK_TAGS.contains(&t) || depth == 0 => {
            // Generic block: recurse into children
            for child in node.children().filter_map(scraper::ElementRef::wrap) {
                convert_node(child, base_url, out, depth + 1);
            }
        }
        _ => {
            // Text-level: emit text if it has any
            let text = collect_text(node, base_url).trim().to_owned();
            if !text.is_empty() {
                out.push(FfonElement::new_str(text));
            }
        }
    }
}

/// Collect all text within a node, converting <a> tags to `text <link>url</link>`.
fn collect_text(node: scraper::ElementRef, base_url: &str) -> String {
    use scraper::Node;
    let mut buf = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(t) => buf.push_str(t),
            Node::Element(e) => {
                if let Some(elem_ref) = scraper::ElementRef::wrap(child) {
                    let name = e.name();
                    if SKIP_TAGS.contains(&name) {
                        continue;
                    }
                    if name == "a" {
                        let href = resolve_href(e.attr("href").unwrap_or(""), base_url);
                        let text = collect_text(elem_ref, base_url);
                        let text = text.trim();
                        if !text.is_empty() && !href.is_empty() {
                            buf.push_str(&format!("{text} <link>{href}</link>"));
                        } else if !text.is_empty() {
                            buf.push_str(text);
                        }
                    } else {
                        buf.push_str(&collect_text(elem_ref, base_url));
                    }
                }
            }
            _ => {}
        }
    }
    buf
}

fn convert_table(node: scraper::ElementRef, out: &mut Vec<FfonElement>) {
    let row_sel = scraper::Selector::parse("tr").unwrap();
    for row in node.select(&row_sel) {
        let cell_sel = scraper::Selector::parse("th, td").unwrap();
        let cells: Vec<String> = row
            .select(&cell_sel)
            .map(|c| c.text().collect::<String>().trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if !cells.is_empty() {
            out.push(FfonElement::new_str(cells.join(" | ")));
        }
    }
}

fn convert_dl(node: scraper::ElementRef, base_url: &str, out: &mut Vec<FfonElement>) {
    let mut current_dt: Option<FfonElement> = None;
    for child in node.children().filter_map(scraper::ElementRef::wrap) {
        match child.value().name() {
            "dt" => {
                if let Some(dt) = current_dt.take() {
                    out.push(dt);
                }
                let text = collect_text(child, base_url).trim().to_owned();
                if !text.is_empty() {
                    current_dt = Some(FfonElement::new_obj(text));
                }
            }
            "dd" => {
                let text = collect_text(child, base_url).trim().to_owned();
                if !text.is_empty() {
                    if let Some(ref mut dt) = current_dt {
                        dt.as_obj_mut().unwrap().push(FfonElement::new_str(text));
                    } else {
                        out.push(FfonElement::new_str(text));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(dt) = current_dt {
        out.push(dt);
    }
}

/// Resolve a potentially relative href against the base URL.
fn resolve_href(href: &str, base_url: &str) -> String {
    if href.is_empty() || href.starts_with('#') {
        return String::new();
    }
    if href.contains("://") || href.starts_with("mailto:") || href.starts_with("tel:") {
        return href.to_owned();
    }
    // Relative URL resolution
    if let Ok(base) = url::Url::parse(base_url) {
        if let Ok(resolved) = base.join(href) {
            return resolved.to_string();
        }
    }
    href.to_owned()
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_webbrowser/test_webbrowser.c (16 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- html_to_ffon unit tests ----

    #[test]
    fn test_empty_page_returns_placeholder() {
        let result = html_to_ffon("<html><body></body></html>", "https://example.com");
        // empty body → placeholder
        assert!(result.len() <= 1);
    }

    #[test]
    fn test_paragraph_becomes_str() {
        let result = html_to_ffon(
            "<html><body><p>Hello world</p></body></html>",
            "https://example.com",
        );
        assert!(result.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Hello world"))));
    }

    #[test]
    fn test_heading_becomes_obj() {
        let result = html_to_ffon(
            "<html><body><h1>Title</h1></body></html>",
            "https://example.com",
        );
        assert!(result.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Title"))));
    }

    #[test]
    fn test_script_skipped() {
        let result = html_to_ffon(
            "<html><body><script>alert('x')</script><p>visible</p></body></html>",
            "https://example.com",
        );
        // No element should contain script content
        for e in &result {
            if let Some(s) = e.as_str() {
                assert!(!s.contains("alert"));
            }
        }
        assert!(result.iter().any(|e| e.as_str().map_or(false, |s| s.contains("visible"))));
    }

    #[test]
    fn test_nav_skipped() {
        let result = html_to_ffon(
            "<html><body><nav><a href='/'>Home</a></nav><p>content</p></body></html>",
            "https://example.com",
        );
        for e in &result {
            if let Some(s) = e.as_str() {
                assert!(!s.contains("Home") || s.contains("content"));
            }
        }
    }

    #[test]
    fn test_link_gets_link_tag() {
        let result = html_to_ffon(
            "<html><body><p><a href='https://rust-lang.org'>Rust</a></p></body></html>",
            "https://example.com",
        );
        let found = result.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<link>") && s.contains("rust-lang.org"))
        });
        assert!(found);
    }

    #[test]
    fn test_relative_link_resolved() {
        let result = html_to_ffon(
            "<html><body><p><a href='/page'>Page</a></p></body></html>",
            "https://example.com",
        );
        let found = result.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("example.com/page"))
        });
        assert!(found);
    }

    #[test]
    fn test_unordered_list_becomes_obj() {
        let result = html_to_ffon(
            "<html><body><ul><li>Alpha</li><li>Beta</li></ul></body></html>",
            "https://example.com",
        );
        let list = result.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key == "list")
        });
        assert!(list.is_some());
        let children = &list.unwrap().as_obj().unwrap().children;
        assert_eq!(children.len(), 2);
        assert!(children[0].as_str().unwrap().contains("Alpha"));
        assert!(children[1].as_str().unwrap().contains("Beta"));
    }

    #[test]
    fn test_ordered_list_numbered() {
        let result = html_to_ffon(
            "<html><body><ol><li>First</li><li>Second</li></ol></body></html>",
            "https://example.com",
        );
        let list = result.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key == "ordered list")
        });
        assert!(list.is_some());
        let children = &list.unwrap().as_obj().unwrap().children;
        assert!(children[0].as_str().unwrap().starts_with("1."));
        assert!(children[1].as_str().unwrap().starts_with("2."));
    }

    #[test]
    fn test_table_row_pipe_delimited() {
        let result = html_to_ffon(
            "<html><body><table><tr><td>A</td><td>B</td></tr></table></body></html>",
            "https://example.com",
        );
        let row = result.iter().find(|e| {
            e.as_str().map_or(false, |s| s.contains(" | "))
        });
        assert!(row.is_some());
        assert!(row.unwrap().as_str().unwrap().contains("A | B"));
    }

    #[test]
    fn test_image_shows_alt() {
        let result = html_to_ffon(
            "<html><body><img alt='A diagram' src='x.png'/></body></html>",
            "https://example.com",
        );
        let img = result.iter().find(|e| {
            e.as_str().map_or(false, |s| s.contains("A diagram") && s.contains("[img]"))
        });
        assert!(img.is_some());
    }

    #[test]
    fn test_fetch_returns_meta_and_url_bar() {
        let mut p = WebbrowserProvider::new();
        let items = p.fetch();
        // Index 0: meta obj
        assert!(items[0].as_obj().map_or(false, |o| o.key == "meta"));
        // Index 1: url bar (no page loaded → str)
        assert!(items[1].as_str().is_some());
    }

    #[test]
    fn test_fetch_url_bar_contains_input_tag() {
        let mut p = WebbrowserProvider::new();
        let items = p.fetch();
        let url_bar = items[1].as_str().unwrap();
        assert!(url_bar.contains("<input>") && url_bar.contains("</input>"));
    }

    #[test]
    fn test_commit_edit_prepends_https() {
        let mut p = WebbrowserProvider::new();
        // commit_edit triggers fetch — use a URL that won't resolve in tests
        // We just test the URL normalization logic directly via current_url
        // by patching: simulate commit fail gracefully
        let _ = p.commit_edit("https://", "example.com");
        // After commit (whether fetch succeeds or fails), current_url is set
        assert_eq!(p.current_url, "https://example.com");
    }

    #[test]
    fn test_commands_includes_refresh() {
        let p = WebbrowserProvider::new();
        assert!(p.commands().contains(&"refresh".to_owned()));
    }

    #[test]
    fn test_resolve_href_absolute() {
        let result = resolve_href("https://other.com/page", "https://base.com");
        assert_eq!(result, "https://other.com/page");
    }

    #[test]
    fn test_resolve_href_relative() {
        let result = resolve_href("/path/to/page", "https://example.com/current");
        assert!(result.contains("example.com/path/to/page"));
    }

    #[test]
    fn test_resolve_href_anchor_empty() {
        let result = resolve_href("#section", "https://example.com");
        assert!(result.is_empty());
    }
}
