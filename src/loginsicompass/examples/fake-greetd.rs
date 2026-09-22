//! A greetd that always asks for a password and accepts one you choose.
//!
//! This is how the greeter gets driven without PAM: nested inside a desktop
//! session, where there is no greetd at all, and without ten real
//! authentication failures every time the wrong-password path is exercised.
//!
//! ```text
//! cargo run -p loginsicompass --example fake-greetd -- /tmp/greetd.sock hunter2
//! GREETD_SOCK=/tmp/greetd.sock cargo run -p desicompass -- --backend auto \
//!     --startup-cmd "$PWD/target/debug/loginsicompass --state-dir /tmp/state"
//! ```
//!
//! It speaks the real wire format, so it exercises `greetd.rs` rather than
//! standing in for it. An example rather than a `[[bin]]` on purpose: examples
//! are not installed, so this cannot end up beside the real greeter.
//!
//! The modules are included by path because `loginsicompass` is a binary crate
//! with no library target for an example to link against.

#[path = "../src/greetd.rs"]
mod greetd;

#[path = "../src/fakegreetd.rs"]
mod fakegreetd;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: fake-greetd <socket-path> [password]");
        std::process::exit(2);
    };
    let password = args.next().unwrap_or_else(|| "password".to_owned());

    // A stale socket from a previous run would make `bind` fail with EADDRINUSE.
    let _ = std::fs::remove_file(&path);

    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot bind {path}: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!("fake greetd listening on {path}; the password is {password:?}");
    fakegreetd::serve_forever(&listener, &password);
}
