//! A plugin process from a future sicompass: it greets with a protocol major
//! this app does not speak. For `tests/process_plugin.rs`.

use sicompass_sdk::plugin_ipc::{Message, write_message};

fn main() {
    let mut out = std::io::stdout().lock();
    let _ = write_message(
        &mut out,
        &Message::Hello {
            protocol: "99.0".into(),
            name: "future".into(),
        },
    );
    // Wait for the app to let go.
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
}
