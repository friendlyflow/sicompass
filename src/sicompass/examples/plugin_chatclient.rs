//! The chatclient plugin, built from its repo at the rev `Cargo.toml` pins, as a
//! program for `tests/integration.rs`.

sicompass_sdk::plugin::main!(chatclient_plugin::ChatClientProvider);
