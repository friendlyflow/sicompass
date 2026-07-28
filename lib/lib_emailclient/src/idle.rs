//! IMAP IDLE background task.
//!
//! Runs an IMAP IDLE connection for a single folder on the shared email
//! runtime. When the server reports EXISTS or EXPUNGE, the shared `notify` flag
//! is set so the provider refreshes on the next render cycle.
//!
//! This replaced an OS thread driven by an `AtomicBool` plus a `SyncSender`
//! shutdown channel. That design could only notice a stop request when its 30 s
//! IDLE poll expired, so `stop()` carried a 45 s bounded join to cover the
//! worst case. A `CancellationToken` under `tokio::select!` cancels the wait
//! immediately instead, and the 30 s poll is gone: the keepalive interval is
//! now the RFC 2177 29 minutes it should always have been.

use crate::connection::{connect_imap, runtime, ImapSession};
use crate::EmailClientConfig;
use async_imap::extensions::idle::IdleResponse;
use async_imap::imap_proto::{MailboxDatum, Response};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const RECONNECT_DELAY: Duration = Duration::from_secs(10);
/// RFC 2177 advises re-issuing IDLE at least every 29 minutes so the server
/// does not drop an apparently inactive client.
const IDLE_KEEPALIVE: Duration = Duration::from_secs(29 * 60);

// ---------------------------------------------------------------------------
// IdleController
// ---------------------------------------------------------------------------

pub struct IdleController {
    /// Shared flag written by the IDLE task when new mail arrives.
    notify: Arc<AtomicBool>,
    /// Cancels the running task; `None` when nothing is running.
    cancel: Option<CancellationToken>,
    /// The OAuth access token the IDLE session should authenticate with.
    ///
    /// Shared rather than cloned into the task: the token refresh in `lib.rs`
    /// replaces the access token roughly hourly, and an IDLE session that
    /// captured it at start-up would keep reconnecting with a dead credential
    /// until the user re-entered the folder.
    token: Arc<Mutex<String>>,
}

impl IdleController {
    pub fn new(notify: Arc<AtomicBool>) -> Self {
        IdleController {
            notify,
            cancel: None,
            token: Arc::new(Mutex::new(String::new())),
        }
    }

    /// Start (or restart) IDLE monitoring on `folder`.
    ///
    /// Stops any existing session first, then spawns a new task on the shared
    /// email runtime.
    pub fn start(&mut self, config: EmailClientConfig, folder: String) {
        self.stop();

        *self.token.lock().expect("token mutex") = config.oauth_access_token.clone();

        let notify = Arc::clone(&self.notify);
        let token = Arc::clone(&self.token);
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());

        runtime().spawn(async move {
            idle_loop(config, folder, notify, cancel, token).await;
        });
    }

    /// Publish a freshly refreshed OAuth access token to the running session.
    ///
    /// Takes effect on the IDLE task's next reconnect; the current IDLE
    /// continues on the old token until the server drops it, which is the same
    /// behaviour as any other long-lived IMAP connection.
    pub fn update_token(&self, access_token: &str) {
        *self.token.lock().expect("token mutex") = access_token.to_owned();
    }

    /// Stop the background IDLE task.
    ///
    /// Returns immediately: cancelling the token interrupts the IDLE wait at
    /// once, and the task sends DONE and logs out on its way down. Nothing here
    /// blocks the render thread.
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
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

async fn idle_loop(
    config: EmailClientConfig,
    folder: String,
    notify: Arc<AtomicBool>,
    cancel: CancellationToken,
    token: Arc<Mutex<String>>,
) {
    while !cancel.is_cancelled() {
        if let Err(e) = run_idle_session(&config, &folder, &notify, &cancel, &token).await {
            eprintln!("emailclient_idle: session error: {e}");
        }

        if cancel.is_cancelled() {
            break;
        }

        // Back off before reconnecting, but wake immediately on cancellation.
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(RECONNECT_DELAY) => {}
        }
    }
}

/// Connect, authenticate, select the folder, then run the IDLE inner loop.
async fn run_idle_session(
    config: &EmailClientConfig,
    folder: &str,
    notify: &Arc<AtomicBool>,
    cancel: &CancellationToken,
    token: &Arc<Mutex<String>>,
) -> Result<(), String> {
    // Re-read the shared token on every reconnect so a refresh that happened
    // while we were idling is picked up here.
    let mut config = config.clone();
    {
        let current = token.lock().expect("token mutex");
        if !current.is_empty() {
            config.oauth_access_token = current.clone();
        }
    }

    let mut session: ImapSession = connect_imap(&config).await?;
    session.select(folder).await.map_err(|e| e.to_string())?;

    while !cancel.is_cancelled() {
        let mut handle = session.idle();
        handle.init().await.map_err(|e| e.to_string())?;

        // Scoped so the mutable borrow of `handle` ends before `done()`
        // consumes it.
        let outcome = {
            let (wait, _stop) = handle.wait_with_timeout(IDLE_KEEPALIVE);
            tokio::select! {
                result = wait => Some(result.map_err(|e| e.to_string())?),
                _ = cancel.cancelled() => None,
            }
        };

        // Always hand the session back, so a cancelled wait still leaves the
        // connection in a state we can log out of cleanly.
        session = handle.done().await.map_err(|e| e.to_string())?;

        match outcome {
            // Cancelled.
            None => break,
            Some(IdleResponse::ManualInterrupt) => break,
            // Keepalive expired with nothing to report; re-issue IDLE.
            Some(IdleResponse::Timeout) => {}
            Some(IdleResponse::NewData(data)) => {
                if is_mailbox_change(data.parsed()) {
                    notify.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    let _ = session.logout().await;
    Ok(())
}

/// Whether an unsolicited response means the mailbox contents changed.
///
/// Anything else (notably the `* OK Still here` keepalive some servers send)
/// must not trigger a refresh.
fn is_mailbox_change(response: &Response<'_>) -> bool {
    matches!(
        response,
        Response::Expunge(_)
            | Response::Vanished { .. }
            | Response::MailboxData(MailboxDatum::Exists(_))
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idle_controller_start_stop_noop_without_config() {
        // With an empty IMAP URL the task should fail fast without panicking.
        let notify = Arc::new(AtomicBool::new(false));
        let mut ctrl = IdleController::new(Arc::clone(&notify));
        ctrl.start(EmailClientConfig::default(), "INBOX".to_owned());
        std::thread::sleep(Duration::from_millis(100));
        ctrl.stop();
        // No panic is the success criterion.
    }

    #[test]
    fn test_needs_refresh_propagates_via_flag() {
        let notify = Arc::new(AtomicBool::new(false));
        let _ctrl = IdleController::new(Arc::clone(&notify));
        // Simulate what the IDLE task does on new mail.
        notify.store(true, Ordering::Relaxed);
        assert!(notify.load(Ordering::Relaxed));
        // Simulate provider calling clear_needs_refresh.
        notify.store(false, Ordering::Relaxed);
        assert!(!notify.load(Ordering::Relaxed));
    }

    #[test]
    fn test_update_token_is_visible_to_the_session() {
        let notify = Arc::new(AtomicBool::new(false));
        let ctrl = IdleController::new(notify);
        ctrl.update_token("refreshed-token");
        assert_eq!(
            *ctrl.token.lock().expect("token mutex"),
            "refreshed-token",
            "a refreshed access token must reach the running IDLE session"
        );
    }

    #[test]
    fn test_only_mailbox_changes_raise_the_flag() {
        assert!(is_mailbox_change(&Response::Expunge(3)));
        assert!(is_mailbox_change(&Response::MailboxData(
            MailboxDatum::Exists(7)
        )));
        // A keepalive must not look like new mail.
        assert!(!is_mailbox_change(&Response::Data {
            status: async_imap::imap_proto::Status::Ok,
            code: None,
            information: Some("Still here".into()),
        }));
    }
}
