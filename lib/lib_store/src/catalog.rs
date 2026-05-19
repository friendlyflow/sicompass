//! Store catalog fetch.
//!
//! The catalog is hosted by the license/store server. This mirrors the proven
//! approach in `lib_remote` (`RemoteProvider::fetch_from_server`): GET the
//! server's `/root`, expect a JSON array of FFON, and wrap each top-level
//! object key with a `<link>` tag so the app's existing link resolver handles
//! sub-navigation into server-hosted store pages.
//!
//! The `wrap_with_link` / `url_encode` helpers are copied (not imported) from
//! `lib_remote`: the SDK boundary keeps `lib_*` crates from depending on each
//! other, and these are ~40 lines.

use sicompass_sdk::ffon::{parse_json_value, FfonElement};
use std::time::Duration;

/// Fetch the store catalog from `{store_url}/root`.
///
/// On any error returns a single explanatory string element, so the store
/// provider still renders (with its license status and checkout link) even
/// when the server is unreachable or not yet deployed.
pub fn fetch_catalog(store_url: &str) -> Vec<FfonElement> {
    if store_url.is_empty() {
        return vec![FfonElement::new_str("Store catalog: no store URL configured")];
    }

    let base = store_url.trim_end_matches('/');
    let root_url = format!("{base}/root");

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return vec![FfonElement::new_str(format!("Store catalog unavailable: {e}"))],
    };

    let response = match client.get(&root_url).header("Accept", "application/json").send() {
        Ok(r) => r,
        Err(e) => {
            return vec![FfonElement::new_str(format!(
                "Store catalog unavailable: {e}"
            ))]
        }
    };

    if !response.status().is_success() {
        return vec![FfonElement::new_str(format!(
            "Store catalog unavailable: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("")
        ))];
    }

    let body = match response.text() {
        Ok(t) => t,
        Err(e) => return vec![FfonElement::new_str(format!("Store catalog unavailable: {e}"))],
    };

    let arr = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(serde_json::Value::Array(a)) => a,
        Ok(_) => return vec![FfonElement::new_str("Store catalog: invalid response (not a list)")],
        Err(e) => return vec![FfonElement::new_str(format!("Store catalog: invalid JSON ({e})"))],
    };

    arr.iter().map(|v| wrap_with_link(v, base)).collect()
}

/// Wrap a top-level JSON value with a `<link>` tag on its object key so
/// sub-navigation is resolved by the app's link handler.
fn wrap_with_link(v: &serde_json::Value, base_url: &str) -> FfonElement {
    if let serde_json::Value::Object(map) = v {
        if let Some((key, _)) = map.iter().next() {
            if key.contains("<link>") {
                return parse_json_value(v);
            }
            let link_key = format!("<link>{base_url}/{}</link>{key}", url_encode(key));
            return FfonElement::new_obj(link_key);
        }
    }
    parse_json_value(v)
}

/// Minimal percent-encoding for path segments (RFC 3986 unreserved chars pass
/// through; everything else is %-encoded). Mirrors `encodeURIComponent`.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*'
            | b'\'' | b'(' | b')' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_url_returns_message() {
        let items = fetch_catalog("");
        assert_eq!(items.len(), 1);
        assert!(items[0].as_str().unwrap().contains("no store URL"));
    }

    #[test]
    fn url_encode_spaces_and_slashes() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a/b"), "a%2Fb");
        assert_eq!(url_encode("plain"), "plain");
    }

    #[test]
    fn wrap_with_link_tags_object_keys() {
        let v = serde_json::json!({ "Themes": [] });
        let wrapped = wrap_with_link(&v, "https://store.example");
        let key = wrapped.as_obj().expect("should be an Obj").key.clone();
        assert!(key.contains("<link>https://store.example/Themes</link>Themes"));
    }

    #[test]
    fn wrap_with_link_passes_strings_through() {
        let v = serde_json::json!("just a string");
        assert_eq!(wrap_with_link(&v, "https://store.example"), FfonElement::Str("just a string".to_owned()));
    }
}
