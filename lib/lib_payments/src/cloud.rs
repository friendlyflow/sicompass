//! The cloud-backup companion a provider embeds.
//!
//! Notes and project management both want the same five things: a settings
//! switch, a row at the top of their first layer, a background uploader that
//! does not stall typing, the handlers that make the grafted tier tree work,
//! and a restore. All of that is here so both providers get one implementation
//! and one set of words, and so their own files stay about notes and boards.
//!
//! A provider wires it up in six places:
//!
//! 1. a [`sicompass_sdk::SettingDecl::checkbox`] in its manifest,
//! 2. `on_setting_change` -> [`CloudBackup::on_setting_change`],
//! 3. the top of its root level -> [`CloudBackup::row`],
//! 4. the end of its save path -> [`CloudBackup::mark_dirty`],
//! 5. `on_button_press` / `on_radio_change` / the input commit -> the
//!    delegating methods below,
//! 6. `take_error`, `take_announcement`, `needs_refresh`.
//!
//! Nothing leaves the machine unless the switch is on *and* the subscription
//! is good. With the switch off this is inert, and the provider behaves exactly
//! as it did before cloud backup existed.

use crate::cert::LicenseStatus;
use crate::{backup, config, entitlement, row, tier_input::TierSession};
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long the store has to stay quiet before an upload starts.
///
/// Typing a note fires a save per keystroke, and every one of those would
/// otherwise be a request. Five seconds is short enough that a user who edits
/// and closes the lid is covered, and long enough that a sentence is one
/// upload rather than forty.
const DEBOUNCE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Test hook
//
// `status()` reads the certificate out of the real user config directory, so
// without an override a developer who actually holds a subscription would see
// this crate's tests, and the providers' tests, take the other branch.
//
// Thread-local and never global: the test binary runs tests in parallel on many
// threads, and a global would let one test's override decide another's result.
// ---------------------------------------------------------------------------

thread_local! {
    static TEST_STATUS: std::cell::RefCell<Option<LicenseStatus>> =
        const { std::cell::RefCell::new(None) };
}

/// Force the subscription status seen by this thread. Tests only.
#[doc(hidden)]
pub fn _set_test_status(status: Option<LicenseStatus>) {
    TEST_STATUS.with(|s| *s.borrow_mut() = status);
}

fn status() -> LicenseStatus {
    if let Some(forced) = TEST_STATUS.with(|s| s.borrow().clone()) {
        return forced;
    }
    entitlement::cloud_status()
}

fn is_active() -> bool {
    entitlement::backs_up(&status())
}

/// One upload request.
struct Job {
    root: PathBuf,
    store_url: String,
    token: String,
}

/// Shared with the worker thread.
struct Shared {
    plugin: &'static str,
    needs_refresh: AtomicBool,
    error: Mutex<Option<String>>,
    /// Hash of the last snapshot the server acknowledged, so an unchanged
    /// store costs nothing at all, not even a request.
    last_hash: Mutex<Option<String>>,
}

impl Shared {
    fn fail(&self, reason: String) {
        let mut args = localize::Args::new();
        args.set("reason", reason);
        *self.error.lock().unwrap() = Some(localize::t_args("payments-backup-failed", &args));
        self.needs_refresh.store(true, Ordering::Release);
    }
}

/// Cloud backup for one provider store.
pub struct CloudBackup {
    plugin: &'static str,
    /// The provider's `settings.json` key for its own "enable cloud backup"
    /// checkbox, e.g. `notesCloudBackup`.
    enable_key: &'static str,
    enabled: bool,
    token: String,
    tier: TierSession,
    shared: Arc<Shared>,
    /// `None` until the first upload is queued, so a user who never turns this
    /// on never gets a thread.
    sender: Option<Sender<Job>>,
    announcement: Option<String>,
}

impl CloudBackup {
    /// `plugin` is the server-side store name (`"notes"`, `"kanban"`).
    pub fn new(plugin: &'static str, enable_key: &'static str) -> Self {
        crate::register_translations();
        CloudBackup {
            plugin,
            enable_key,
            enabled: false,
            token: config::redeem_token(),
            tier: TierSession::new(),
            shared: Arc::new(Shared {
                plugin,
                needs_refresh: AtomicBool::new(false),
                error: Mutex::new(None),
                last_hash: Mutex::new(None),
            }),
            sender: None,
            announcement: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Whether anything should actually be sent: the user asked for it, and
    /// the subscription that pays for the storage is good.
    pub fn is_backing_up(&self) -> bool {
        self.enabled && is_active()
    }

    // ---- settings ---------------------------------------------------------

    /// Route a broadcast setting. Returns whether it was one of ours.
    ///
    /// The app hands every setting to every provider, so a caller can chain
    /// this ahead of its own keys.
    pub fn on_setting_change(&mut self, key: &str, value: &str) -> bool {
        if key == self.enable_key {
            self.set_enabled(matches!(value, "true" | "1" | "on"));
            return true;
        }
        if key == "licenseRedeemToken" {
            self.token = value.to_owned();
        }
        self.tier.on_setting_change(key, value)
    }

    /// Switch backup on or off.
    ///
    /// Switching it on without a subscription is allowed on purpose: it tells
    /// the user what is missing and leaves the row in place to fix it, which is
    /// friendlier than a checkbox that silently refuses to stay ticked.
    pub fn set_enabled(&mut self, enabled: bool) {
        let was = self.enabled;
        self.enabled = enabled;
        if enabled && !was && !is_active() {
            self.announcement = Some(localize::t("payments-cloud-needs-subscription"));
            *self.shared.error.lock().unwrap() =
                Some(localize::t("payments-cloud-needs-subscription"));
        }
        self.shared.needs_refresh.store(true, Ordering::Release);
    }

    // ---- the row ----------------------------------------------------------

    /// The row for the top of the provider's root level, or `None` when the
    /// switch is off. Always a link to the cloud tier; only the wording moves.
    pub fn row(&self) -> Option<FfonElement> {
        if !self.enabled {
            return None;
        }
        Some(row::cloud_row(self.tier.store_url(), &status()))
    }

    /// Whether `raw` is the row above, for a provider's `reconcile` to skip.
    pub fn is_row(raw: &str) -> bool {
        row::is_cloud_row(raw)
    }

    // ---- the tier tree ----------------------------------------------------

    pub fn on_button_press(&mut self, function_name: &str) -> bool {
        match self.tier.on_button_press(function_name) {
            Some(Err(e)) => {
                *self.shared.error.lock().unwrap() = Some(e);
                true
            }
            Some(Ok(())) => true,
            None => false,
        }
    }

    pub fn on_radio_change(&mut self, group: &str, value: &str) {
        self.tier.on_radio_change(group, value);
    }

    /// An `<input>` under the grafted tier tree was committed. Returns whether
    /// it was one of ours.
    pub fn commit_input(&mut self, label: &str, value: &str) -> bool {
        let Some(result) = self.tier.commit_input(label, value) else {
            return false;
        };
        if label == "License redeem token" {
            // Redeeming here is the point of showing the tier tree inside the
            // provider: the row's wording has to change without a trip to
            // Settings.
            //
            // The token is kept even when the redeem itself failed. A server
            // that was unreachable for ten seconds is no reason to throw away
            // what the user pasted, and the certificate can be fetched again
            // later with the credential still in hand.
            let token = value.trim().to_owned();
            if !token.is_empty() {
                config::save_redeem_token(&token);
            }
            self.token = token;
            self.shared.needs_refresh.store(true, Ordering::Release);
        }
        if let Err(e) = result {
            *self.shared.error.lock().unwrap() = Some(e);
        }
        true
    }

    // ---- uploading --------------------------------------------------------

    /// The store under `root` changed. Cheap and safe to call from a save path
    /// that runs on every keystroke: it queues, it never does I/O.
    pub fn mark_dirty(&mut self, root: &Path) {
        if !self.is_backing_up() {
            return;
        }
        if self.token.is_empty() {
            return;
        }
        let job = Job {
            root: root.to_path_buf(),
            store_url: self.tier.store_url().to_owned(),
            token: self.token.clone(),
        };
        if self.sender.is_none() {
            self.sender = Some(spawn_worker(Arc::clone(&self.shared)));
        }
        // A dead worker (the thread panicked) must not take the provider down
        // with it: the user's notes are already safe on disk.
        if let Some(tx) = &self.sender
            && tx.send(job).is_err()
        {
            self.sender = None;
        }
    }

    // ---- restore ----------------------------------------------------------

    /// Pull the server's copy over an empty store. See [`backup::restore`] for
    /// why it refuses to run over live data.
    ///
    /// `Ok(true)` means files were written and the caller must reload its store
    /// from disk. `Ok(false)` means there was nothing to restore. Both carry a
    /// line to tell the user; the error case carries the reason.
    pub fn restore(&mut self, root: &Path) -> Result<(bool, String), String> {
        crate::register_translations();
        if !self.enabled {
            return Err(localize::t("payments-cloud-needs-subscription"));
        }
        match backup::restore(self.tier.store_url(), &self.token, root, self.plugin) {
            Ok(true) => {
                // The store on disk is now the server's, so the next upload
                // must not be skipped as "unchanged".
                *self.shared.last_hash.lock().unwrap() = None;
                self.shared.needs_refresh.store(true, Ordering::Release);
                Ok((true, localize::t("payments-restore-done")))
            }
            Ok(false) => Ok((false, localize::t("payments-restore-empty"))),
            Err(e) => Err(e),
        }
    }

    // ---- provider plumbing ------------------------------------------------

    pub fn take_error(&mut self) -> Option<String> {
        self.shared.error.lock().unwrap().take()
    }

    pub fn take_announcement(&mut self) -> Option<String> {
        self.announcement.take()
    }

    pub fn needs_refresh(&self) -> bool {
        self.shared.needs_refresh.load(Ordering::Acquire)
    }

    pub fn clear_needs_refresh(&mut self) {
        self.shared.needs_refresh.store(false, Ordering::Release);
    }
}

/// Whether `children` is a page grafted in from the license server rather than
/// a list the provider rendered.
///
/// Following the cloud row makes the app fetch the server's tier tree and graft
/// it onto that row, **inside the provider's own FFON tree**. The app does not
/// push a path segment for a `<link>`, so the provider still believes the
/// cursor is on the level it last rendered, and an edit made on the payment
/// page arrives at `sync_ffon_body_children` looking exactly like an edit to
/// the user's own list. Reconciling it would rewrite the store from the
/// contents of a checkout form.
///
/// The test is for a `<button>`, `<radio>` or `<checked>` tag **outside** any
/// `<input>` wrapper. Every tier tree carries a button to pay with and a radio
/// to choose a plan, and neither sits in an input. The exclusion is what makes
/// this safe against the user's own writing: a note or a card titled
/// `<button>x</button>` is handed back as `<input><button>x</button></input>`,
/// and dropping that edit because of what someone typed would be its own kind
/// of data loss.
pub fn is_grafted_page(children: &[FfonElement]) -> bool {
    children.iter().any(|e| {
        let raw = match e {
            FfonElement::Str(s) => s.as_str(),
            FfonElement::Obj(o) => o.key.as_str(),
        };
        let outside = outside_inputs(raw);
        sicompass_sdk::tags::has_button(&outside)
            || sicompass_sdk::tags::has_radio(&outside)
            || sicompass_sdk::tags::has_checked(&outside)
    })
}

/// `raw` with the contents of every `<input>...</input>` removed, so a tag the
/// user typed into a field cannot be mistaken for one the server sent.
fn outside_inputs(raw: &str) -> String {
    const OPEN: &str = "<input>";
    const CLOSE: &str = "</input>";
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start + OPEN.len()]);
        rest = &rest[start + OPEN.len()..];
        match rest.find(CLOSE) {
            Some(end) => {
                // The value itself is dropped; the wrapper is kept so the
                // string still reads as an input row.
                out.push_str(CLOSE);
                rest = &rest[end + CLOSE.len()..];
            }
            // Unterminated: nothing after this can be trusted as "outside".
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The uploader. One OS thread rather than a runtime: it spends its life
/// asleep on a channel, and the request it makes is blocking anyway.
fn spawn_worker(shared: Arc<Shared>) -> Sender<Job> {
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    let name = format!("sicompass-backup-{}", shared.plugin);
    let worker_shared = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
        .name(name)
        .spawn(move || worker_loop(&worker_shared, &rx));
    if spawned.is_err() {
        // Out of threads. Backup is off for this session; the notes are still
        // on disk, which is the part that matters.
        shared.fail("could not start the backup worker".to_owned());
    }
    tx
}

fn worker_loop(shared: &Arc<Shared>, rx: &Receiver<Job>) {
    while let Ok(mut job) = rx.recv() {
        // Absorb the rest of the burst. Typing a sentence fires a save per
        // keystroke, and this turns that into one upload.
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(newer) => job = newer,
                Err(RecvTimeoutError::Timeout) => break,
                // The provider is gone. Its last edit was already saved to
                // disk, and uploading it now would race a shutdown.
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        upload(shared, &job);
    }
}

fn upload(shared: &Arc<Shared>, job: &Job) {
    let snapshot = match backup::read_store(&job.root, shared.plugin) {
        Ok(s) => s,
        // Could not read the store. Uploading a partial read as if it were the
        // whole store is how a backup destroys itself, so nothing is sent.
        Err(e) => return shared.fail(e),
    };

    if shared.last_hash.lock().unwrap().as_deref() == Some(snapshot.hash.as_str()) {
        return;
    }

    match backup::put_snapshot(&job.store_url, &job.token, &snapshot) {
        Ok(_) => {
            *shared.last_hash.lock().unwrap() = Some(snapshot.hash.clone());
        }
        Err(e) => shared.fail(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active() -> LicenseStatus {
        LicenseStatus::Active {
            licensee: "Acme Corp".to_owned(),
            renews_in_days: 342,
        }
    }

    /// Every test runs with an explicit status, so none of them depends on
    /// whether the developer holds a real subscription.
    fn backup_with(status: Option<LicenseStatus>) -> CloudBackup {
        _set_test_status(status);
        CloudBackup::new("notes", "notesCloudBackup")
    }

    #[test]
    fn the_switch_is_off_until_it_is_switched_on() {
        let b = backup_with(Some(LicenseStatus::None));
        assert!(!b.is_enabled());
        assert!(b.row().is_none(), "nothing about payment before opting in");
        assert!(!b.is_backing_up());
    }

    #[test]
    fn switching_on_without_a_subscription_says_so_out_loud() {
        let mut b = backup_with(Some(LicenseStatus::None));
        b.set_enabled(true);
        let spoken = b
            .take_announcement()
            .expect("the screen reader must say why");
        assert!(spoken.contains("cloud and store"), "{spoken}");
        assert!(b.take_error().is_some(), "and the header must show it too");
        // Said once, not on every fetch.
        assert!(b.take_announcement().is_none());
    }

    #[test]
    fn switching_on_with_a_subscription_is_quiet() {
        let mut b = backup_with(Some(active()));
        b.set_enabled(true);
        assert!(b.take_announcement().is_none());
        assert!(b.take_error().is_none());
        assert!(b.is_backing_up());
    }

    #[test]
    fn the_row_appears_only_once_the_switch_is_on() {
        let mut b = backup_with(Some(LicenseStatus::None));
        assert!(b.row().is_none());
        b.set_enabled(true);
        assert!(b.row().is_some());
        b.set_enabled(false);
        assert!(b.row().is_none());
    }

    #[test]
    fn the_rows_wording_follows_the_subscription() {
        let mut unpaid = backup_with(Some(LicenseStatus::None));
        unpaid.set_enabled(true);
        let FfonElement::Obj(o) = unpaid.row().unwrap() else {
            unreachable!()
        };
        let unpaid_key = o.key.clone();

        let mut paid = backup_with(Some(active()));
        paid.set_enabled(true);
        let FfonElement::Obj(o) = paid.row().unwrap() else {
            unreachable!()
        };
        assert_ne!(unpaid_key, o.key);
        // Both are links to the same place, so the row never moves.
        assert!(unpaid_key.contains("/cloud</link>"));
        assert!(o.key.contains("/cloud</link>"));
    }

    #[test]
    fn the_settings_checkbox_drives_the_switch() {
        let mut b = backup_with(Some(active()));
        assert!(b.on_setting_change("notesCloudBackup", "true"));
        assert!(b.is_enabled());
        assert!(b.on_setting_change("notesCloudBackup", "false"));
        assert!(!b.is_enabled());
    }

    /// The other provider's switch must not move this one.
    #[test]
    fn another_providers_switch_is_not_ours() {
        let mut b = backup_with(Some(active()));
        assert!(!b.on_setting_change("kanbanCloudBackup", "true"));
        assert!(!b.is_enabled());
        assert!(!b.on_setting_change("colorScheme", "light"));
    }

    #[test]
    fn the_store_settings_are_ours_too() {
        let mut b = backup_with(Some(active()));
        assert!(b.on_setting_change("storeUrl", "https://srv.example"));
        assert!(b.on_setting_change("licenseRedeemToken", "tok-42"));
        assert_eq!(b.token, "tok-42");
        b.set_enabled(true);
        let FfonElement::Obj(o) = b.row().unwrap() else {
            unreachable!()
        };
        assert!(o.key.contains("https://srv.example/cloud"), "{}", o.key);
    }

    /// The load-bearing one: with the switch off, a save must not reach for the
    /// network or start a thread.
    #[test]
    fn a_save_uploads_nothing_while_the_switch_is_off() {
        let mut b = backup_with(Some(active()));
        b.on_setting_change("licenseRedeemToken", "tok-42");
        b.mark_dirty(Path::new("/nonexistent"));
        assert!(b.sender.is_none(), "no worker without the switch");
    }

    #[test]
    fn a_save_uploads_nothing_without_a_subscription() {
        let mut b = backup_with(Some(LicenseStatus::None));
        b.set_enabled(true);
        b.on_setting_change("licenseRedeemToken", "tok-42");
        b.mark_dirty(Path::new("/nonexistent"));
        assert!(b.sender.is_none(), "no worker without a subscription");
    }

    #[test]
    fn a_save_uploads_nothing_without_a_token() {
        let mut b = backup_with(Some(active()));
        b.set_enabled(true);
        b.on_setting_change("licenseRedeemToken", "");
        b.mark_dirty(Path::new("/nonexistent"));
        assert!(b.sender.is_none(), "no worker without a credential");
    }

    #[test]
    fn the_tier_tree_controls_are_handled() {
        let mut b = backup_with(Some(LicenseStatus::None));
        // Port 1 refuses immediately, so neither the checkout nor the redeem
        // below reaches the real store server.
        b.on_setting_change("storeUrl", "http://127.0.0.1:1");

        assert!(b.on_button_press("checkout:cloud"));
        assert!(b.commit_input("License redeem token", "tok-42"));
        // Not ours, so the provider keeps its own controls.
        assert!(!b.on_button_press("archive"));
        assert!(!b.commit_input("buy milk", "x"));
    }

    /// A server that was unreachable for a moment must not cost the user the
    /// token they just pasted: without it the backup has no credential at all.
    #[test]
    fn a_pasted_token_is_kept_even_when_redeeming_fails() {
        let mut b = backup_with(Some(LicenseStatus::None));
        b.on_setting_change("storeUrl", "http://127.0.0.1:1");
        assert!(b.commit_input("License redeem token", "  tok-42  "));
        assert_eq!(b.token, "tok-42", "the token is trimmed and kept");
        assert!(
            b.take_error().is_some(),
            "and the failure is still reported"
        );
    }

    #[test]
    fn the_cloud_row_is_recognised_for_reconcile() {
        let mut b = backup_with(Some(LicenseStatus::None));
        b.set_enabled(true);
        let FfonElement::Obj(o) = b.row().unwrap() else {
            unreachable!()
        };
        assert!(CloudBackup::is_row(&o.key));
        assert!(!CloudBackup::is_row("<id>7</id><input>buy milk</input>"));
    }

    #[test]
    fn restore_is_refused_while_the_switch_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = backup_with(Some(active()));
        let message = b.restore(dir.path()).unwrap_err();
        assert!(message.contains("cloud and store"), "{message}");
    }

    /// Restoring over live notes is refused by `backup::restore`, and the
    /// refusal has to reach the user rather than being swallowed here.
    #[test]
    fn restore_over_live_data_is_reported_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("0001"), "mine").unwrap();
        let mut b = backup_with(Some(active()));
        b.set_enabled(true);
        b.on_setting_change("storeUrl", "http://127.0.0.1:1");
        b.on_setting_change("licenseRedeemToken", "tok-42");
        assert!(b.restore(dir.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("0001")).unwrap(),
            "mine"
        );
    }

    // ---- grafted-page detection -------------------------------------------

    /// The server's cloud tier tree, as the app grafts it into a provider.
    fn tier_page() -> Vec<FfonElement> {
        vec![
            FfonElement::new_str("Enable 'cloud and store' per month"),
            FfonElement::new_str("Lemonsqueezy setup: <input>acct-1</input>"),
            FfonElement::new_obj("<radio>monthly or yearly"),
            FfonElement::new_str("<button>checkout:cloud</button>for payment"),
            FfonElement::new_str("License redeem token: <input>tok-42</input>"),
        ]
    }

    #[test]
    fn a_tier_page_is_recognised() {
        assert!(is_grafted_page(&tier_page()));
    }

    #[test]
    fn a_radio_options_sublist_is_recognised() {
        let options = vec![
            FfonElement::new_str("<checked>per year"),
            FfonElement::new_str("per month"),
        ];
        assert!(is_grafted_page(&options));
    }

    #[test]
    fn an_ordinary_list_of_notes_is_not_a_grafted_page() {
        let rows = vec![
            FfonElement::new_obj("list meta:"),
            FfonElement::new_str("<id>1</id><input>milk</input>"),
            FfonElement::new_obj("<id>2</id><input>Groceries</input>"),
        ];
        assert!(!is_grafted_page(&rows));
    }

    /// The one that makes this safe to apply to the user's own writing. A note
    /// titled with a tag is escaped when rendered, but the app hands back what
    /// was typed, so the raw row really does contain `<button>`.
    #[test]
    fn a_note_the_user_titled_with_a_tag_is_not_a_grafted_page() {
        for typed in [
            "<button>x</button>",
            "<radio>pick one",
            "<checked>yes",
            "<button>a</button> and <radio>b",
        ] {
            let rows = vec![
                FfonElement::new_str(format!("<input>{typed}</input>")),
                FfonElement::new_str("<id>1</id><input>milk</input>"),
            ];
            assert!(
                !is_grafted_page(&rows),
                "a note the user typed must never be mistaken for the payment page: {typed}"
            );
        }
    }

    #[test]
    fn a_cloud_row_alone_is_not_a_grafted_page() {
        let mut b = backup_with(Some(LicenseStatus::None));
        b.set_enabled(true);
        let rows = vec![
            b.row().unwrap(),
            FfonElement::new_str("<id>1</id><input>milk</input>"),
        ];
        assert!(!is_grafted_page(&rows));
    }

    #[test]
    fn an_unterminated_input_does_not_panic() {
        let rows = vec![FfonElement::new_str("<input>never closed")];
        assert!(!is_grafted_page(&rows));
    }

    #[test]
    fn inputs_are_blanked_but_the_wrapper_survives() {
        assert_eq!(
            outside_inputs("<id>1</id><input>milk</input>"),
            "<id>1</id><input></input>"
        );
        assert_eq!(
            outside_inputs("a <input><button>x</button></input> b"),
            "a <input></input> b"
        );
        assert_eq!(
            outside_inputs("<button>checkout:cloud</button>for payment"),
            "<button>checkout:cloud</button>for payment"
        );
    }
}
