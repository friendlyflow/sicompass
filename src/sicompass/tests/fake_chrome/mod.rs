//! A fake Chrome, for the tests that drive the web browser plugin
//! (`tests/fixtures/plugins/webbrowser`) the way the app runs it: in the
//! sandbox, Chrome started through the host's `process` grant with a message
//! channel, its pages loaded from the plugin's browser task.
//!
//! No real Chrome is ever started by these tests. The program the plugin asks
//! for, `google-chrome`, resolves (through the host's test override) to a bash
//! script that forwards Chrome's pipe, file descriptors 3 and 4, to a server
//! in this process. The server speaks just enough of the DevTools protocol for
//! the browser's load path, and serves one stub page per URL:
//! `<p>Fake page for URL</p>`.

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The text a page for `url` reads, as the plugin renders it.
pub fn page_text(url: &str) -> String {
    format!("Fake page for {url}")
}

/// The fake Chrome's path: a script that connects Chrome's pipe to the
/// server. Started once per test binary.
pub fn program() -> PathBuf {
    static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
            let port = listener.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    std::thread::spawn(move || serve(stream));
                }
            });
            // Kept for the life of the test binary: every browser plugin in
            // it starts this script.
            let dir = tempfile::tempdir().unwrap().keep();
            let script = dir.join("google-chrome");
            std::fs::write(
                &script,
                format!(
                    "#!/usr/bin/env bash\n\
                     # A fake Chrome for sicompass's tests: its DevTools pipe\n\
                     # (fds 3 and 4) forwarded to the test process.\n\
                     exec 5<>/dev/tcp/127.0.0.1/{port}\n\
                     cat <&3 >&5 &\n\
                     cat <&5 >&4\n"
                ),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            script
        })
        .clone()
}

/// One fake Chrome: answer each command, and fire a load event after each
/// navigation.
fn serve(stream: TcpStream) {
    let mut reader = stream.try_clone().unwrap();
    let mut writer = stream;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    // The URL each session (tab) is on.
    let mut urls: std::collections::HashMap<String, String> = Default::default();
    let mut tabs = 0u32;
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Some(end) = buf.iter().position(|&b| b == 0) {
            let msg: Vec<u8> = buf.drain(..=end).collect();
            let Ok(cmd) = serde_json::from_slice::<Value>(&msg[..msg.len() - 1]) else {
                continue;
            };
            let id = cmd["id"].clone();
            let session = cmd["sessionId"].as_str().unwrap_or("").to_owned();
            let method = cmd["method"].as_str().unwrap_or("");
            let mut events = Vec::new();
            let result = match method {
                "Target.createTarget" => {
                    tabs += 1;
                    json!({"targetId": format!("target-{tabs}")})
                }
                "Target.attachToTarget" => {
                    let s = format!(
                        "session-{}",
                        cmd["params"]["targetId"].as_str().unwrap_or("")
                    );
                    urls.insert(s.clone(), "about:blank".to_owned());
                    json!({"sessionId": s})
                }
                "Page.navigate" => {
                    let url = cmd["params"]["url"].as_str().unwrap_or("").to_owned();
                    urls.insert(session.clone(), url);
                    events.push(json!({"method": "Page.loadEventFired",
                        "params": {"timestamp": 1}, "sessionId": session}));
                    json!({"frameId": "frame", "loaderId": "loader"})
                }
                "Runtime.evaluate" => {
                    let url = urls.get(&session).cloned().unwrap_or_default();
                    json!({"result": {"type": "string", "value": evaluate(
                        cmd["params"]["expression"].as_str().unwrap_or(""),
                        &url,
                    )}})
                }
                "Browser.close" => {
                    let _ = send(&mut writer, &json!({"id": id, "result": {}}));
                    return;
                }
                _ => json!({}),
            };
            if send(&mut writer, &json!({"id": id, "result": result})).is_err() {
                return;
            }
            for e in events {
                if send(&mut writer, &e).is_err() {
                    return;
                }
            }
        }
    }
}

/// What a script the browser runs answers, told apart by what it asks for.
fn evaluate(js: &str, url: &str) -> Value {
    let html = format!(
        "<!DOCTYPE html><html><head><title>Fake</title></head>\
         <body><p>{}</p></body></html>",
        page_text(url)
    );
    if js == "window.location.href" {
        json!(url)
    } else if js.contains("readyState") {
        json!("complete|100")
    } else if js.contains("XMLSerializer") || js.contains("sicStampLive") {
        // The whole page: plain, or as the prune serialises it.
        json!(html)
    } else if js.contains("sic-consent") || js.contains("gated") {
        // No cookie step on a fake page.
        json!({"gated": false, "labels": [], "marked": false, "pending": false})
    } else {
        json!(true)
    }
}

fn send(w: &mut TcpStream, v: &Value) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(v).unwrap();
    bytes.push(0);
    w.write_all(&bytes)
}
