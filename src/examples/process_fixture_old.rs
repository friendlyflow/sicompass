//! A plugin process from before protocol 1.1: it greets with 1.0, answers the
//! calls that open it, and exits on a request it was not built to read, as a
//! real 1.0 plugin does (its runtime takes the channel for broken). For
//! `tests/process_plugin.rs`.

use sicompass_sdk::plugin::{Descriptor, PollResult};
use sicompass_sdk::plugin_ipc::{Message, Request, Response, read_message, write_message};

fn main() {
    let mut out = std::io::stdout().lock();
    let mut input = std::io::stdin().lock();
    let hello = Message::Hello {
        protocol: "1.0".into(),
        name: "old".into(),
    };
    if write_message(&mut out, &hello).is_err() {
        return;
    }
    while let Ok(Some(Message::Call { id, request })) = read_message(&mut input) {
        let response = match request {
            Request::Describe => Response::Descriptor(Descriptor {
                name: "fixture".into(),
                display_name: "fixture".into(),
                ..Default::default()
            }),
            Request::Poll => Response::Poll(PollResult::default()),
            Request::CannotAddHere => return,
            _ => Response::Unit,
        };
        let reply = Message::Reply {
            id,
            response,
            moved_to: None,
        };
        if write_message(&mut out, &reply).is_err() {
            return;
        }
    }
}
