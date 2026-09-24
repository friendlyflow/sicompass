//! Following a `<link>` row to a page served over HTTP, end to end.
//!
//! Any provider can put `text <link>https://…</link>` on an `Obj`. The list
//! shows it as a link row, and Right fetches the URL through the HTTP client
//! the app registers (`boot::register_http_client`) and shows the page in
//! place: a JSON FFON document as it is, an HTML page through the web
//! pipeline. Remote services and the cloud backup rows used it once, and it did
//! not depend on them, which is what this pins.
//!
//! Its own test binary because the HTTP client is registered once per process.

use sdl3::keyboard::{Keycode, Mod};
use sicompass::app_state::AppRenderer;
use sicompass::events::dispatch_key;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::GenericProvider;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn press(r: &mut AppRenderer, key: Keycode) {
    dispatch_key(r, Some(key), Mod::NOMOD);
}

fn keys(elements: &[FfonElement]) -> Vec<String> {
    elements
        .iter()
        .map(|e| match e {
            FfonElement::Str(s) => s.clone(),
            FfonElement::Obj(o) => o.key.clone(),
        })
        .collect()
}

#[test]
fn a_link_row_opens_the_json_page_it_points_at() {
    sicompass::boot::register_http_client();

    // wiremock needs a runtime only to start and program the server; the app's
    // blocking client then talks to it from this thread.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    rt.block_on(
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                "alpha",
                { "beta": ["gamma"] }
            ])))
            .mount(&server),
    );

    let link = format!("Server page <link>{}/page</link>", server.uri());
    let mut r = AppRenderer::new();
    r.hooks = Box::new(sicompass::boot::ProgramsHooks::default());
    let row = link.clone();
    sicompass::programs::register_provider(
        &mut r,
        Box::new(GenericProvider::new("demo", "demo", move |_| {
            vec![FfonElement::new_obj(row.clone())]
        })),
    );
    sicompass::list::create_list_current_layer(&mut r);

    // Into the provider: the cursor is on the link row, shown as a link.
    press(&mut r, Keycode::Right);
    let label = r.current_list_item().map(|i| i.label.clone()).unwrap();
    assert!(label.starts_with("+l"), "a link row: {label}");

    // Right follows it.
    press(&mut r, Keycode::Right);
    assert_eq!(r.current_id.depth(), 3, "one level into the page");
    let root = r.ffon[0].as_obj().unwrap();
    let FfonElement::Obj(row) = &root.children[0] else {
        panic!("the link row is still an Obj")
    };
    assert_eq!(row.key, link);
    assert_eq!(
        keys(&row.children),
        vec!["alpha".to_owned(), "beta".to_owned()]
    );
    let received = rt.block_on(server.received_requests()).unwrap();
    assert_eq!(received.len(), 1, "fetched once");
}
