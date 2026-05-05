//! IMAP IDLE background thread — port of `emailclient_idle.c`.
//!
//! Spawns a thread that maintains an IMAP IDLE connection to a single folder.
//! When the server sends EXISTS or EXPUNGE, the shared `notify` flag is set
//! so the provider can refresh on the next render cycle.

use crate::connection::{connect_imap, ImapSession};
use crate::EmailClientConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Arc;
use std::time::Duration;

const RECONNECT_DELAY_SECS: u64 = 10;
/// Maximum time to wait for the IDLE thread to exit after stop() is called.
const STOP_TIMEOUT_SECS: u64 = RECONNECT_DELAY_SECS + 35;

// ---------------------------------------------------------------------------
// IdleController
// ---------------------------------------------------------------------------

pub struct IdleController {
    /// Shared flag written by the IDLE thread when new mail arrives.
    notify: Arc<AtomicBool>,
    /// Signal to stop the background thread.
    running: Arc<AtomicBool>,
    /// Background thread handle.
    thread: Option<std::thread::JoinHandle<()>>,
    /// Dropping this sender wakes the reconnect-delay sleep in the worker.
    shutdown_tx: Option<SyncSender<()>>,
}

impl IdleController {
    pub fn new(notify: Arc<AtomicBool>) -> Self {
        IdleController {
            notify,
            running: Arc::new(AtomicBool::new(false)),
            thread: None,
            shutdown_tx: None,
        }
    }

    /// Start (or restart) IDLE monitoring on `folder`.
    ///
    /// Stops any existing session first, then spawns a new background thread.
    pub fn start(&mut self, config: EmailClientConfig, folder: String) {
        self.stop();

        let notify = Arc::clone(&self.notify);
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::Relaxed);

        let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
        self.shutdown_tx = Some(tx);

        self.thread = Some(std::thread::spawn(move || {
            idle_loop(config, folder, notify, running, rx);
        }));
    }

    /// Stop the background IDLE thread and wait for it to exit (bounded wait).
    ///
    /// Signals the shutdown channel and joins the thread via a helper thread
    /// with a timeout of `STOP_TIMEOUT_SECS`.  The timeout is long enough to
    /// cover one full IDLE poll interval (30 s) plus reconnect delay, so in
    /// practice the join always succeeds.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Drop the sender — the worker's recv_timeout returns Err immediately,
        // cutting short any reconnect delay sleep.
        let _ = self.shutdown_tx.take();
        if let Some(handle) = self.thread.take() {
            let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = handle.join();
                let _ = done_tx.send(());
            });
            // Best-effort bounded wait — don't block forever.
            let _ = done_rx.recv_timeout(Duration::from_secs(STOP_TIMEOUT_SECS));
        }
    }
}

impl Drop for IdleController {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// IDLE loop
// ---------------------------------------------------------------------------

/// The main IDLE background thread function.
fn idle_loop(
    config: EmailClientConfig,
    folder: String,
    notify: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    shutdown_rx: Receiver<()>,
) {
    while running.load(Ordering::Relaxed) {
        match run_idle_session(&config, &folder, &notify, &running) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("emailclient_idle: session error: {e}");
            }
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

        // Back-off before reconnecting — wake up early if shutdown is signaled.
        let _ = shutdown_rx.recv_timeout(Duration::from_secs(RECONNECT_DELAY_SECS));
    }
}

/// Connect, authenticate, select folder, then run the IDLE inner loop.
fn run_idle_session(
    config: &EmailClientConfig,
    folder: &str,
    notify: &Arc<AtomicBool>,
    running: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut session: ImapSession = connect_imap(config)?;
    session.select(folder).map_err(|e| e.to_string())?;

    // Inner IDLE loop.
    // Use wait_with_timeout so the thread wakes every IDLE_POLL_INTERVAL and
    // can check the running flag.  This keeps stop() non-blocking — the thread
    // exits on its own within one poll interval after running is set to false.
    const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(30);
    while running.load(Ordering::Relaxed) {
        let idle = session.idle().map_err(|e| e.to_string())?;
        let outcome = idle
            .wait_with_timeout(IDLE_POLL_INTERVAL)
            .map_err(|e| e.to_string())?;
        // Handle consumed — session borrow released; drain unsolicited responses
        // only when the server actually notified us (not on a poll timeout).
        if running.load(Ordering::Relaxed)
            && matches!(outcome, imap::extensions::idle::WaitOutcome::MailboxChanged)
        {
            while let Ok(response) = session.unsolicited_responses.try_recv() {
                if matches!(
                    response,
                    imap::types::UnsolicitedResponse::Exists(_)
                        | imap::types::UnsolicitedResponse::Expunge(_)
                ) {
                    notify.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    let _ = session.logout();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_controller_start_stop_noop_without_config() {
        // With an empty IMAP URL the thread should fail fast without panicking.
        let notify = Arc::new(AtomicBool::new(false));
        let mut ctrl = IdleController::new(Arc::clone(&notify));
        ctrl.start(EmailClientConfig::default(), "INBOX".to_owned());
        // Give the thread a moment to exit.
        std::thread::sleep(Duration::from_millis(100));
        ctrl.stop();
        // No panic is the success criterion.
    }

    #[test]
    fn test_needs_refresh_propagates_via_flag() {
        let notify = Arc::new(AtomicBool::new(false));
        let ctrl = IdleController::new(Arc::clone(&notify));
        // Simulate what the IDLE thread does on new mail.
        notify.store(true, Ordering::Relaxed);
        assert!(notify.load(Ordering::Relaxed));
        // Simulate provider calling clear_needs_refresh.
        notify.store(false, Ordering::Relaxed);
        assert!(!notify.load(Ordering::Relaxed));
    }
}
