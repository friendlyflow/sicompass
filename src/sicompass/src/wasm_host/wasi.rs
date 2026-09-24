//! The WASI p2 a plugin gets: a fixed, inert baseline.
//!
//! Guests target `wasm32-wasip2`, whose `std` imports a handful of WASI
//! interfaces even for a plugin that never touches a file (stdio, environment,
//! exit, clocks, io). Linking those is what lets a guest use plain `std`, and none
//! of them grants authority over anything outside the plugin:
//!
//! - **stdout and stderr** go to the host log, line by line, prefixed with the
//!   plugin's name, exactly like the `log` import. Never a terminal.
//! - **stdin** is empty, the **environment** has no variables and no arguments.
//! - **clocks and randomness** reveal nothing a guest could not learn from
//!   `now-millis` already.
//! - **the filesystem** has no preopened directory, so every path fails. A plugin
//!   granted `storage` or `filesystem` gets preopens (docs/plugin-platform.md §4).
//! - **sockets** are denied in the context below, and the import audit rejects a
//!   component that imports them without a `sockets` grant, so they never reach an
//!   instance in the first place.
//!
//! The rule that makes the import list a capability set is kept by the audit in
//! [`super::audit_component_imports`], which accepts exactly
//! [`BASELINE_INTERFACES`] from `wasi:*`.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use wasmtime::component::{Linker, ResourceTable};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamError};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use super::HostState;

/// The `wasi:*` interfaces every plugin may import, without their `@version`:
/// the SDK's single definition, which the audit also uses.
pub use sicompass_sdk::plugin_abi::WASI_BASELINE as BASELINE_INTERFACES;

/// Whether a `wasi:*` interface (version already stripped) is in the baseline.
pub fn is_baseline(interface: &str) -> bool {
    sicompass_sdk::plugin_abi::is_wasi_baseline(interface)
}

/// The per-plugin WASI context: the inert baseline, plus a preopen for each
/// granted `(guest path, host path)`.
pub fn ctx(plugin_name: &str, preopens: &[(std::path::PathBuf, std::path::PathBuf)]) -> Result<WasiCtx, String> {
    let mut b = WasiCtxBuilder::new();
    for (guest, host) in preopens {
        std::fs::create_dir_all(host).map_err(|e| format!("{}: {e}", host.display()))?;
        b.preopened_dir(host, guest.to_string_lossy(), wasmtime_wasi::FsPerms::ReadWrite)
            .map_err(|e| format!("cannot open {}: {e}", host.display()))?;
    }
    b.stdout(LogStream::new(plugin_name, "stdout"))
        .stderr(LogStream::new(plugin_name, "stderr"))
        // Belt and braces: the audit already refuses a component that imports
        // sockets without a grant, so these only matter if that ever regresses.
        .allow_tcp(false)
        .allow_udp(false)
        .allow_ip_name_lookup(false);
    Ok(b.build())
}

/// Link every WASI p2 interface wasmtime implements.
///
/// Linking an interface does not hand it to a guest: an instance receives only
/// what its component imports, and the audit refuses a component whose imports
/// go beyond [`BASELINE_INTERFACES`] and its grants before it is instantiated. The
/// context from [`baseline_ctx`] then makes the baseline inert.
pub fn add_to_linker(linker: &mut Linker<HostState>) -> Result<(), String> {
    wasmtime_wasi::p2::add_to_linker_sync(linker).map_err(|e| format!("link wasi p2: {e}"))
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// A fresh resource table for a plugin instance.
pub fn resource_table() -> ResourceTable {
    ResourceTable::new()
}

// ---------------------------------------------------------------------------
// stdout / stderr into the host log
// ---------------------------------------------------------------------------

/// Longest line kept before it is logged as it stands. A guest printing without
/// newlines must not grow host memory without bound.
const MAX_LINE: usize = 8 * 1024;

/// A plugin's stdout or stderr, turned into log lines.
///
/// Synchronous on purpose: guest calls run on the UI thread, which has no tokio
/// runtime, and `wasmtime-wasi`'s async stdout needs one.
#[derive(Clone)]
pub struct LogStream {
    plugin: Arc<str>,
    stream: &'static str,
    pending: Arc<Mutex<Vec<u8>>>,
}

impl LogStream {
    pub fn new(plugin: &str, stream: &'static str) -> Self {
        LogStream {
            plugin: Arc::from(plugin),
            stream,
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Append bytes, emitting every complete line (and any line past
    /// [`MAX_LINE`]). Returns the lines emitted, for tests.
    fn push(&self, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(mut pending) = self.pending.lock() else {
            return out;
        };
        pending.extend_from_slice(bytes);
        loop {
            let cut = match pending.iter().position(|&b| b == b'\n') {
                Some(i) => Some((i, i + 1)),
                None if pending.len() >= MAX_LINE => Some((MAX_LINE, MAX_LINE)),
                None => None,
            };
            let Some((end, skip)) = cut else { break };
            let line = String::from_utf8_lossy(&pending[..end]).into_owned();
            pending.drain(..skip);
            tracing::info!(
                target: "wasm_plugin",
                plugin = %self.plugin,
                stream = self.stream,
                "{line}"
            );
            out.push(line);
        }
        out
    }
}

impl IsTerminal for LogStream {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for LogStream {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }

    fn async_stream(&self) -> Box<dyn tokio::io::AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }
}

impl OutputStream for LogStream {
    fn write(&mut self, bytes: Bytes) -> Result<(), StreamError> {
        self.push(&bytes);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        Ok(())
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        // Always ready: lines leave the buffer as they complete, and a line that
        // never ends is cut at MAX_LINE.
        Ok(MAX_LINE)
    }
}

#[async_trait::async_trait]
impl Pollable for LogStream {
    async fn ready(&mut self) {}
}

impl tokio::io::AsyncWrite for LogStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.push(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_lines_are_logged_and_partial_ones_wait() {
        let s = LogStream::new("demo", "stdout");
        assert!(s.push(b"hel").is_empty());
        assert_eq!(s.push(b"lo\nwor"), vec!["hello".to_owned()]);
        assert_eq!(s.push(b"ld\n\n"), vec!["world".to_owned(), String::new()]);
    }

    #[test]
    fn a_line_without_newline_is_cut_rather_than_grown_forever() {
        let s = LogStream::new("demo", "stdout");
        let lines = s.push(&vec![b'x'; MAX_LINE * 2 + 5]);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.len() == MAX_LINE));
        assert_eq!(s.pending.lock().unwrap().len(), 5);
    }

    #[test]
    fn the_baseline_names_no_authority() {
        for i in BASELINE_INTERFACES {
            assert!(i.starts_with("wasi:"), "{i}");
            assert!(!i.contains("sockets"), "{i} must be gated, not baseline");
            assert!(!i.contains("http"), "{i}: plugins use sicompass:plugin/net");
        }
    }
}
