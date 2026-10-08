//! A plugin process for `tests/process_plugin.rs`: one of each thing a plugin
//! does, driven by commands, so the host can be tested against a real process.

use sicompass_sdk::FfonElement;
use sicompass_sdk::plugin::{
    self, DashboardKind, Descriptor, Frame, Key, Plugin, PollResult, ProviderOp, host, license,
};

struct Fixture {
    path: String,
    greeting: Option<String>,
    note: String,
    child: Option<std::process::Child>,
    ops: Vec<ProviderOp>,
    rendering: Vec<String>,
    keys: u32,
}

impl Plugin for Fixture {
    fn new() -> Self {
        Fixture {
            path: "/".into(),
            greeting: None,
            note: String::new(),
            child: None,
            ops: Vec::new(),
            rendering: Vec::new(),
            keys: 0,
        }
    }

    fn init(&mut self) {
        self.greeting = host::get_setting("greeting");
    }

    fn describe(&self) -> Descriptor {
        Descriptor {
            name: "fixture".into(),
            display_name: host::translate("fixture-name"),
            dashboard_kind: DashboardKind::Interactive,
            supports_config_files: true,
            ..Default::default()
        }
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        vec![
            FfonElement::new_str(host::translate("fixture-hello")),
            FfonElement::new_str(format!(
                "greeting: {}",
                self.greeting.as_deref().unwrap_or("-")
            )),
            FfonElement::new_str(format!("note: {}", self.note)),
            FfonElement::new_str(format!(
                "storage: {}",
                plugin::storage_dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default()
            )),
        ]
    }

    fn poll(&mut self) -> PollResult {
        // Rendering finishes in the next poll, the way a real one finishes on
        // a thread of its own.
        for url in self.rendering.drain(..) {
            host::page_rendered(&url, &[FfonElement::new_str(format!("rendered {url}"))]);
        }
        PollResult {
            at_root: self.path == "/",
            child_pid: self.child.as_ref().map(|c| c.id()),
            ..Default::default()
        }
    }

    fn current_path(&self) -> &str {
        &self.path
    }

    fn set_current_path(&mut self, p: &str) {
        self.path = p.to_owned();
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        if key == "greeting" {
            self.greeting = Some(value.to_owned());
        }
    }

    fn execute_command(&mut self, cmd: &str, selection: &str) -> bool {
        match cmd {
            "jump" => self.path = "/elsewhere".into(),
            "boom" => panic!("the fixture was told to panic"),
            "hang" => std::thread::sleep(std::time::Duration::from_secs(60)),
            "print" => println!("this must not reach the channel"),
            "token" => self.note = license::token(selection).unwrap_or_else(|| "none".into()),
            "spawn-child" => {
                self.child = plugin::command("sleep").arg("30").spawn().ok();
            }
            "write-storage" => {
                let dir = plugin::storage_dir().expect("storage was granted");
                std::fs::write(dir.join("written.txt"), selection).unwrap();
            }
            "edit" => self.ops.push(ProviderOp {
                command: "edit".into(),
                payload: plugin::encode_one(&FfonElement::new_str(selection)),
                label: format!("edit {selection}"),
            }),
            _ => return false,
        }
        true
    }

    fn render_url(&mut self, url: &str) -> bool {
        if url.contains("decline") {
            return false;
        }
        self.rendering.push(url.to_owned());
        true
    }

    fn cannot_add_here(&mut self) -> Option<String> {
        (self.path == "/full").then(|| "nothing fits in /full".to_owned())
    }
    fn take_timeline_entries(&mut self) -> Vec<ProviderOp> {
        std::mem::take(&mut self.ops)
    }

    fn undo(&mut self, entry: &ProviderOp) -> Result<(), String> {
        self.note = format!("undid {}", entry.label);
        Ok(())
    }

    fn load_config(&mut self, contents: &[u8]) -> bool {
        self.note = String::from_utf8_lossy(contents).into_owned();
        true
    }

    fn save_config(&self) -> Option<Vec<u8>> {
        Some(format!("saved {}", self.note).into_bytes())
    }

    fn dashboard_render(&mut self, cols: u16, rows: u16) -> Frame {
        let mut f = plugin::blank_frame(cols, rows);
        plugin::write_str(&mut f, 0, 0, &format!("keys {}", self.keys), 0xFFFF_FFFF);
        f.cursor = Some((0, 0));
        f
    }

    fn dashboard_key(&mut self, _key: Key) -> bool {
        self.keys += 1;
        true
    }

    fn cleanup(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

sicompass_sdk::plugin::main!(Fixture);
