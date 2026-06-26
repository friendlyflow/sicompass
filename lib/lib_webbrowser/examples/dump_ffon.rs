//! Quick manual test harness for the web→FFON conversion (landmark wrappers etc.).
//!
//!   # convert a local HTML file:
//!   cargo run -p sicompass-webbrowser --example dump_ffon -- path/to/page.html
//!
//!   # or fetch a live URL (launches headless Chrome):
//!   cargo run -p sicompass-webbrowser --example dump_ffon -- https://example.com
//!
//! Prints the FFON tree with indentation so you can eyeball the structure —
//! landmark groups (navigation / main content / footer / complementary) show up
//! as named Obj nodes wrapping their children.

use sicompass_sdk::ffon::{html_to_ffon_with_forms, FfonElement};

fn print_tree(elems: &[FfonElement], depth: usize) {
    for e in elems {
        let pad = "  ".repeat(depth);
        match e {
            FfonElement::Str(s) => println!("{pad}{s}"),
            FfonElement::Obj(o) => {
                println!("{pad}[{}]", o.key);
                print_tree(&o.children, depth + 1);
            }
        }
    }
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: dump_ffon <html-file | http(s)-url>");
        std::process::exit(2);
    });

    let elems = if arg.starts_with("http://") || arg.starts_with("https://") {
        sicompass_webbrowser::fetch_url_to_ffon(&arg)
    } else {
        let html = std::fs::read_to_string(&arg).expect("read HTML file");
        // Also surface the form map so we can see which fields are addressable.
        let (elems, form_map) = html_to_ffon_with_forms(&html, "https://example.com");
        let mut entries: Vec<_> = form_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        println!("--- FORM MAP ({} entries) ---", entries.len());
        for (path, node) in &entries {
            println!("  {path:?}  ->  {}  [{:?}]", node.css_selector, node.kind);
        }
        println!("--- FFON TREE ---");
        return print_tree(&elems, 0);
    };

    print_tree(&elems, 0);
}
