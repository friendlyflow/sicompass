//! The login screen, as a provider.
//!
//! Everything the greeter shows is an ordinary sicompass page: two `<radio>`
//! groups, a `<password>` field and three `<button>` rows. That is the point of
//! building the greeter on `sicompass-ui` rather than drawing it by hand — the
//! masking, the screen-reader labels, the insert-mode editing and the
//! announcement live region all already work, and none of it is reimplemented
//! here.
//!
//! # There is no login button
//!
//! Pressing Enter in Insert mode on the password field submits, which is how
//! every other `<input>` in the app commits. A separate button would be a
//! second way to do one thing, and would put a row between the field and the
//! error message it produces.
//!
//! # Where the password lives
//!
//! In a [`Zeroizing<String>`], taken (not copied) when it is handed to the
//! worker. `fetch()` always emits an *empty* `<password></password>`: the live
//! typed value belongs to the host's insert buffer, which is what blanks the
//! field after every attempt without a special case here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::tags;
use zeroize::Zeroizing;

use crate::auth::{GreetdCmd, GreetdEvent, GreetdWorker};
use crate::lastlogin;
use crate::power;
use crate::sessions::SessionEntry;
use crate::users::UserEntry;

/// Group labels. These are what `on_radio_change` is handed back, so they are
/// matched on rather than re-derived.
const GROUP_USER: &str = "User";
const GROUP_SESSION: &str = "Session";

const PASSWORD_LABEL: &str = "Password";

/// Where the conversation with greetd has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No attempt in flight and nothing asked yet.
    Idle,
    /// A request is out; greetd has not answered.
    Waiting,
    /// greetd asked something and is waiting for us.
    Prompting { secret: bool },
    /// The user is through; the session is being started.
    Starting,
    /// greetd accepted `start_session`. The greeter should exit so greetd can
    /// hand over the display.
    Done,
}

pub struct LoginProvider {
    /// `[]` at the page, `["User"]` or `["Session"]` inside a group.
    path: Vec<String>,

    users: Vec<UserEntry>,
    sessions: Vec<SessionEntry>,
    selected_user: usize,
    selected_session: usize,

    password: Zeroizing<String>,

    phase: Phase,
    /// greetd's own prompt text, shown verbatim when it sends one.
    prompt: Option<String>,
    /// The last notice or failure. Sticky: it stays until the next attempt, so
    /// the user can arrow onto it and have it read again.
    message: Option<String>,

    announcement: Option<String>,
    fatal: Option<String>,

    greetd: Option<GreetdWorker>,
    last: lastlogin::Store,
    power: power::Commands,

    clock: String,
    clock_minute: u64,
    /// Set when only the clock changed: drives `needs_refresh`, never `tick`.
    cosmetic: bool,
    /// Set when a greetd event changed something: drives `tick`.
    dirty: bool,

    /// Flipped once greetd has accepted `start_session`. The main loop reads it
    /// through `HostHooks::should_quit`, because greetd only launches the
    /// session after this process exits.
    done: Arc<AtomicBool>,
}

impl LoginProvider {
    pub fn new(
        users: Vec<UserEntry>,
        sessions: Vec<SessionEntry>,
        last: lastlogin::Store,
        power: power::Commands,
        greetd: Option<GreetdWorker>,
    ) -> Self {
        let user_names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
        let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
        let selected_user = lastlogin::index_of(&user_names, last.user());
        let selected_session = lastlogin::index_of(&session_ids, last.session());

        let mut me = Self {
            path: Vec::new(),
            users,
            sessions,
            selected_user,
            selected_session,
            password: Zeroizing::new(String::new()),
            phase: Phase::Idle,
            prompt: None,
            message: None,
            announcement: None,
            fatal: None,
            greetd,
            last,
            power,
            clock: String::new(),
            clock_minute: 0,
            cosmetic: false,
            dirty: false,
            done: Arc::new(AtomicBool::new(false)),
        };
        me.refresh_clock();
        me.begin_for_selected_user();
        me
    }

    /// True once greetd has started the session and the greeter should quit.
    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Shared with the render loop, which polls it once per frame.
    pub fn done_flag(&self) -> &Arc<AtomicBool> {
        &self.done
    }

    fn current_user(&self) -> Option<&UserEntry> {
        self.users.get(self.selected_user)
    }

    fn current_session(&self) -> Option<&SessionEntry> {
        self.sessions.get(self.selected_session)
    }

    /// Start (or restart) authentication for whoever is selected.
    fn begin_for_selected_user(&mut self) {
        let Some(user) = self.current_user().map(|u| u.name.clone()) else {
            self.message = Some("No accounts to sign in to".to_owned());
            return;
        };
        let Some(greetd) = self.greetd.as_ref() else {
            // No socket: the UI still works, which is what makes it possible to
            // run the greeter nested for development.
            self.message = Some("Not connected to greetd".to_owned());
            return;
        };
        greetd.send(GreetdCmd::Create { username: user });
        self.phase = Phase::Waiting;
        self.password = Zeroizing::new(String::new());
    }

    /// Hand the typed password to greetd.
    fn submit(&mut self) {
        // Take the secret *first*, before anything that can fail. Whatever
        // happens next, this provider's copy is gone and `Zeroizing` wipes the
        // buffer it came from: an early return must never leave a typed
        // password sitting in the field.
        let secret = std::mem::replace(&mut self.password, Zeroizing::new(String::new()));

        if !matches!(self.phase, Phase::Prompting { .. }) {
            self.fail_to_submit("Not ready for a password yet");
            return;
        }
        let Some(greetd) = self.greetd.as_ref() else {
            self.fail_to_submit("Not connected to greetd");
            return;
        };
        greetd.send(GreetdCmd::Answer {
            response: Some(secret.to_string()),
        });
        self.phase = Phase::Waiting;
        self.message = None;
        self.announcement = Some("Checking your password".to_owned());
    }

    /// Report a submit that never left the building.
    ///
    /// Spoken as well as shown: a screen-reader user who pressed Enter and
    /// heard nothing has no way to tell that from a slow PAM.
    fn fail_to_submit(&mut self, why: &str) {
        self.message = Some(why.to_owned());
        self.announcement = Some(why.to_owned());
    }

    /// greetd accepted the credentials; ask it to launch the chosen session.
    fn start_session(&mut self) {
        let Some(session) = self.current_session().cloned() else {
            self.message = Some("No session to start".to_owned());
            self.phase = Phase::Idle;
            return;
        };
        let Some(greetd) = self.greetd.as_ref() else {
            return;
        };
        greetd.send(GreetdCmd::Start {
            cmd: session.exec.clone(),
            env: session.env(),
        });
        self.phase = Phase::Starting;
        self.announcement = Some(format!("Starting {}", session.name));
    }

    /// Drain everything the worker has produced. Returns true if the page changed.
    fn drain_greetd(&mut self) -> bool {
        let mut changed = false;
        loop {
            let Some(evt) = self.greetd.as_ref().and_then(|g| g.try_recv()) else {
                break;
            };
            changed = true;
            match evt {
                GreetdEvent::Prompt { secret, text } => {
                    self.phase = Phase::Prompting { secret };
                    self.announcement = Some(text.clone());
                    self.prompt = Some(text);
                }
                GreetdEvent::Notice { text, is_error } => {
                    if is_error {
                        self.announcement = Some(text.clone());
                    }
                    self.message = Some(text);
                }
                GreetdEvent::Authenticated => self.start_session(),
                GreetdEvent::Started => {
                    // Only now is the choice worth remembering.
                    if let Some(u) = self.current_user() {
                        let name = u.name.clone();
                        self.last.set_user(&name);
                    }
                    if let Some(s) = self.current_session() {
                        let id = s.id.clone();
                        self.last.set_session(&id);
                    }
                    self.last.commit();
                    self.phase = Phase::Done;
                    self.done.store(true, Ordering::Relaxed);
                }
                GreetdEvent::Failed { auth, text } => {
                    if auth {
                        self.message = Some("Wrong password. Try again.".to_owned());
                        self.announcement = Some("Wrong password. Try again.".to_owned());
                        // greetd keeps the conversation open after an auth
                        // error, but the simplest correct thing is to start the
                        // attempt over so we are never guessing which prompt is
                        // outstanding.
                        self.begin_for_selected_user();
                    } else {
                        self.message = Some(text.clone());
                        self.announcement = Some(text);
                        self.phase = Phase::Idle;
                    }
                }
                GreetdEvent::Io { text } => {
                    self.fatal = Some(text.clone());
                    self.announcement = Some(text);
                    self.phase = Phase::Idle;
                }
            }
        }
        changed
    }

    fn refresh_clock(&mut self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let minute = now / 60;
        if minute == self.clock_minute && !self.clock.is_empty() {
            return false;
        }
        self.clock_minute = minute;
        self.clock = format_clock(now);
        true
    }

    // ---- Page construction -------------------------------------------------

    fn radio_group(&self, label: &str, options: &[String], selected: usize) -> FfonElement {
        let mut group = FfonElement::new_obj(format!("<radio>{label}"));
        let obj = group.as_obj_mut().expect("new_obj is an Obj");
        for (i, opt) in options.iter().enumerate() {
            obj.push(FfonElement::Str(if i == selected {
                tags::format_checked(opt)
            } else {
                opt.clone()
            }));
        }
        group
    }

    fn user_labels(&self) -> Vec<String> {
        self.users.iter().map(|u| u.name.clone()).collect()
    }

    fn session_labels(&self) -> Vec<String> {
        self.sessions.iter().map(|s| s.name.clone()).collect()
    }

    /// The whole page, at path `/`.
    fn page(&self) -> Vec<FfonElement> {
        let mut out = Vec::new();

        if self.users.is_empty() {
            // Degradation path: nothing in /etc/passwd we can offer. Let the
            // user type a name rather than showing an empty group.
            out.push(FfonElement::Str(format!(
                "User: {}",
                tags::format_input("")
            )));
        } else {
            out.push(self.radio_group(GROUP_USER, &self.user_labels(), self.selected_user));
        }

        if !self.sessions.is_empty() {
            out.push(self.radio_group(
                GROUP_SESSION,
                &self.session_labels(),
                self.selected_session,
            ));
        }

        // Always empty: the live value lives in the host's insert buffer.
        out.push(FfonElement::Str(format!(
            "{PASSWORD_LABEL}: {}",
            tags::format_password("")
        )));

        if let Some(p) = &self.prompt {
            out.push(FfonElement::Str(p.clone()));
        }
        if let Some(m) = &self.message {
            out.push(FfonElement::Str(m.clone()));
        }

        out.push(FfonElement::Str(format!(
            "<button>{}</button>Suspend",
            power::SUSPEND
        )));
        out.push(FfonElement::Str(format!(
            "<button>{}</button>Restart",
            power::REBOOT
        )));
        out.push(FfonElement::Str(format!(
            "<button>{}</button>Shut down",
            power::POWEROFF
        )));

        out.push(FfonElement::Str(self.clock.clone()));
        out
    }

    /// The index of the password row within [`page`], for landing the cursor.
    pub fn password_row(&self) -> usize {
        let mut i = 0;
        i += 1; // user group or typed-name row
        if !self.sessions.is_empty() {
            i += 1;
        }
        i
    }
}

impl Provider for LoginProvider {
    fn name(&self) -> &str {
        "login"
    }

    fn display_name(&self) -> String {
        "Sign in".to_owned()
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        // Path-scoped: the whole page at the root, and just the options inside
        // a group. Returning the whole tree from inside a group would graft a
        // copy of the page under one of its own descendants — which is why
        // `refresh_current_directory` keeps a list of providers that do that,
        // and why this one is deliberately not on it.
        match self.path.first().map(String::as_str) {
            Some(GROUP_USER) => {
                let labels = self.user_labels();
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        FfonElement::Str(if i == self.selected_user {
                            tags::format_checked(l)
                        } else {
                            l.clone()
                        })
                    })
                    .collect()
            }
            Some(GROUP_SESSION) => {
                let labels = self.session_labels();
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        FfonElement::Str(if i == self.selected_session {
                            tags::format_checked(l)
                        } else {
                            l.clone()
                        })
                    })
                    .collect()
            }
            _ => self.page(),
        }
    }

    fn push_path(&mut self, segment: &str) {
        self.path.push(tags::strip_display(segment));
    }

    fn pop_path(&mut self) {
        self.path.pop();
    }

    fn current_path(&self) -> &str {
        if self.path.is_empty() { "/" } else { "/group" }
    }

    fn on_radio_change(&mut self, group: &str, value: &str) {
        let group = tags::strip_display(group);
        match group.as_str() {
            GROUP_USER => {
                let Some(idx) = self.users.iter().position(|u| u.name == value) else {
                    return;
                };
                if idx == self.selected_user {
                    return;
                }
                self.selected_user = idx;
                self.prompt = None;
                self.message = None;
                self.announcement = Some(format!("User {value}"));
                // greetd is configuring a session for the *previous* user.
                // Cancel it before asking for another, or the new attempt is
                // refused.
                if let Some(g) = self.greetd.as_ref() {
                    g.send(GreetdCmd::Cancel);
                }
                self.begin_for_selected_user();
            }
            GROUP_SESSION => {
                let Some(idx) = self.sessions.iter().position(|s| s.name == value) else {
                    return;
                };
                self.selected_session = idx;
                self.announcement = Some(format!("Session {value}"));
                // No greetd traffic: the session only matters at start_session.
            }
            _ => {}
        }
    }

    fn commit_edit(&mut self, _old: &str, new: &str) -> bool {
        // The host hands over the real typed text, not the mask.
        let value = tags::extract_password(new)
            .or_else(|| tags::extract_input(new))
            .unwrap_or_else(|| tags::strip_display(new));

        if let Some(u) = tags::extract_input(new)
            && self.users.is_empty()
        {
            // The typed-name degradation path.
            let name = tags::strip_display(&u);
            if !name.is_empty() {
                self.users = vec![UserEntry {
                    name,
                    uid: 0,
                    full_name: None,
                    shell: String::new(),
                }];
                self.selected_user = 0;
                self.begin_for_selected_user();
            }
            return true;
        }

        self.password = Zeroizing::new(value);
        self.submit();
        true
    }

    fn on_button_press(&mut self, function_name: &str) {
        let Some(line) = power::Commands::announcement(function_name) else {
            return;
        };
        // Say it before spawning, so a screen reader gets the words out while
        // the screen is still up.
        self.announcement = Some(line.to_owned());
        if power::Commands::ends_the_session(function_name)
            && let Some(g) = self.greetd.as_ref()
        {
            g.send(GreetdCmd::Cancel);
        }
        if let Err(e) = self.power.run(function_name) {
            let text = format!("Could not {function_name}: {e}");
            self.announcement = Some(text.clone());
            self.message = Some(text);
        }
    }

    fn tick(&mut self) -> bool {
        // The clock deliberately does NOT report through `tick`: a true tick
        // rebuilds the tree even in Insert mode, which would throw away a
        // half-typed password once a minute. It goes through `needs_refresh`,
        // which the host gates on not being in Insert mode.
        if self.refresh_clock() {
            self.cosmetic = true;
        }
        let changed = self.drain_greetd();
        self.dirty |= changed;
        std::mem::take(&mut self.dirty)
    }

    fn needs_refresh(&self) -> bool {
        self.cosmetic
    }

    fn clear_needs_refresh(&mut self) {
        self.cosmetic = false;
    }

    fn take_announcement(&mut self) -> Option<String> {
        self.announcement.take()
    }

    fn take_error(&mut self) -> Option<String> {
        self.fatal.take()
    }
}

/// `Monday 22 September, 21:04`, in local time.
///
/// Hand-rolled rather than pulling `chrono` in: a greeter needs one format, and
/// the date arithmetic below is the whole of it.
fn format_clock(unix_secs: u64) -> String {
    const DAYS: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    let local = unix_secs as i64 + local_utc_offset_secs();
    let days = local.div_euclid(86_400);
    let secs_today = local.rem_euclid(86_400);
    let (h, m) = (secs_today / 3600, (secs_today % 3600) / 60);

    // 1970-01-01 was a Thursday, which is why DAYS starts there.
    let weekday = DAYS[days.rem_euclid(7) as usize];
    // The year is deliberately not shown: it is the one part of the date
    // nobody reads off a login screen, and it makes the line longer to speak.
    let (_year, month, day) = civil_from_days(days);
    format!(
        "{weekday} {day} {}, {h:02}:{m:02}",
        MONTHS[(month - 1) as usize]
    )
}

/// Seconds east of UTC, from the `TZ`-aware `localtime_r`.
fn local_utc_offset_secs() -> i64 {
    // SAFETY: `localtime_r` writes into a `tm` we own and reads a `time_t` we
    // own; neither pointer escapes. `tm_gmtoff` is a GNU/BSD extension that
    // libc exposes on every platform this binary is built for (Linux only).
    unsafe {
        let t: libc::time_t = 0;
        let mut out: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut out).is_null() {
            return 0;
        }
        out.tm_gmtoff as i64
    }
}

/// Days since the Unix epoch to `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, which is the standard branch-free way to
/// do this and is why there is no leap-year table here.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakegreetd::{Step, bind, serve_script};
    use crate::greetd::{AuthMessageType, ErrorType, GreetdClient, Response};
    use crate::sessions::SessionType;
    use std::time::{Duration, Instant};

    fn user(name: &str) -> UserEntry {
        UserEntry {
            name: name.to_owned(),
            uid: 1000,
            full_name: None,
            shell: "/bin/sh".to_owned(),
        }
    }

    fn session(id: &str, name: &str) -> SessionEntry {
        SessionEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            exec: vec![format!("/bin/{id}")],
            desktop_names: name.to_owned(),
            session_type: SessionType::Wayland,
        }
    }

    /// A provider with no greetd behind it — enough for every page-shape test.
    fn offline(users: Vec<UserEntry>, sessions: Vec<SessionEntry>) -> LoginProvider {
        let dir = tempfile::tempdir().unwrap();
        LoginProvider::new(
            users,
            sessions,
            lastlogin::Store::load(dir.path()),
            power::Commands::default(),
            None,
        )
    }

    fn two_users() -> LoginProvider {
        offline(
            vec![user("nico"), user("guest")],
            vec![session("desicompass", "Desicompass"), session("cosmic", "COSMIC")],
        )
    }

    fn labels(v: &[FfonElement]) -> Vec<String> {
        v.iter()
            .map(|e| match e {
                FfonElement::Str(s) => s.clone(),
                FfonElement::Obj(o) => o.key.clone(),
            })
            .collect()
    }

    // ---- Page shape ----

    #[test]
    fn the_page_has_two_radio_groups_a_password_and_three_power_buttons() {
        let mut p = two_users();
        let page = p.fetch();
        let l = labels(&page);

        assert_eq!(l[0], "<radio>User");
        assert_eq!(l[1], "<radio>Session");
        assert!(l[2].starts_with("Password: <password>"), "{}", l[2]);

        // Exactly one password field on the page.
        assert_eq!(
            page.iter()
                .filter(|e| matches!(e, FfonElement::Str(s) if tags::has_password(s)))
                .count(),
            1
        );
        // No login button: Enter in the field is the only way to submit.
        assert!(
            !l.iter().any(|s| s.contains("<button>login")),
            "there must be no login button: {l:?}"
        );
        // Three separate power buttons, not a group.
        for f in [power::SUSPEND, power::REBOOT, power::POWEROFF] {
            assert_eq!(
                l.iter().filter(|s| s.contains(&format!("<button>{f}</button>"))).count(),
                1,
                "expected exactly one {f} button in {l:?}"
            );
        }
    }

    #[test]
    fn the_password_row_index_points_at_the_password() {
        let mut p = two_users();
        let page = p.fetch();
        let row = p.password_row();
        match &page[row] {
            FfonElement::Str(s) => assert!(tags::has_password(s), "row {row} was {s}"),
            other => panic!("row {row} is not a leaf: {other:?}"),
        }
    }

    /// With no sessions the page is one row shorter, and the index must follow.
    #[test]
    fn the_password_row_index_follows_a_missing_session_group() {
        let mut p = offline(vec![user("nico")], vec![]);
        let page = p.fetch();
        let row = p.password_row();
        match &page[row] {
            FfonElement::Str(s) => assert!(tags::has_password(s), "row {row} was {s}"),
            other => panic!("row {row} is not a leaf: {other:?}"),
        }
    }

    #[test]
    fn the_selected_option_is_the_checked_one() {
        let mut p = two_users();
        let page = p.fetch();
        let group = page[0].as_obj().unwrap();
        assert_eq!(group.children.len(), 2);
        assert!(tags::has_checked(group.children[0].as_str().unwrap()));
        assert!(!tags::has_checked(group.children[1].as_str().unwrap()));
    }

    /// The contract that keeps `refresh_current_directory`'s depth-≥-2 branch
    /// from grafting a copy of the whole page under one of its own children.
    #[test]
    fn fetch_inside_a_group_returns_only_that_groups_options() {
        let mut p = two_users();
        p.push_path("<radio>User");
        let inside = p.fetch();
        assert_eq!(labels(&inside), vec!["<checked>nico", "guest"]);

        p.pop_path();
        p.push_path("<radio>Session");
        let inside = p.fetch();
        assert_eq!(labels(&inside), vec!["<checked>Desicompass", "COSMIC"]);

        p.pop_path();
        assert_eq!(p.fetch().len(), 8, "back to the whole page");
    }

    #[test]
    fn with_no_accounts_the_user_row_becomes_a_typed_input() {
        let mut p = offline(vec![], vec![session("s", "S")]);
        let l = labels(&p.fetch());
        assert!(l[0].starts_with("User: <input>"), "{}", l[0]);
    }

    // ---- Selection ----

    #[test]
    fn changing_the_session_announces_but_sends_nothing() {
        let mut p = two_users();
        p.on_radio_change(GROUP_SESSION, "COSMIC");
        assert_eq!(p.selected_session, 1);
        assert_eq!(p.take_announcement().as_deref(), Some("Session COSMIC"));
    }

    #[test]
    fn an_unknown_option_is_ignored() {
        let mut p = two_users();
        p.on_radio_change(GROUP_SESSION, "Plan 9");
        assert_eq!(p.selected_session, 0);
        p.on_radio_change("Nonsense", "nico");
        assert_eq!(p.selected_user, 0);
    }

    // ---- Announcements never leak the password ----

    /// The provider-side counterpart to the host's own guarantee (see the
    /// tests around `input_is_password` in `accesskit_sdl.rs`).
    #[test]
    fn no_announcement_ever_contains_the_password() {
        const SECRET: &str = "correct-horse-battery-staple";
        let mut p = two_users();
        let mut spoken: Vec<String> = Vec::new();

        p.phase = Phase::Prompting { secret: true };
        p.commit_edit("", &tags::format_password(SECRET));
        while let Some(a) = p.take_announcement() {
            spoken.push(a);
        }
        p.tick();
        while let Some(a) = p.take_announcement() {
            spoken.push(a);
        }

        for line in &spoken {
            assert!(
                !line.contains(SECRET),
                "announcement leaked the password: {line}"
            );
            // Not even a fragment of it.
            for w in SECRET.split('-') {
                assert!(!line.contains(w), "announcement leaked {w:?}: {line}");
            }
        }
        assert!(!spoken.is_empty(), "submitting must say something");
    }

    #[test]
    fn the_password_buffer_is_cleared_after_submitting() {
        let mut p = two_users();
        p.phase = Phase::Prompting { secret: true };
        p.commit_edit("", &tags::format_password("hunter2"));
        assert!(
            p.password.is_empty(),
            "the provider must not keep the password after handing it over"
        );
    }

    #[test]
    fn fetch_always_emits_an_empty_password_field() {
        let mut p = two_users();
        p.phase = Phase::Prompting { secret: true };
        p.commit_edit("", &tags::format_password("hunter2"));
        let page = p.fetch();
        let row = page[p.password_row()].as_str().unwrap().to_owned();
        assert_eq!(
            tags::extract_password(&row).as_deref(),
            Some(""),
            "the typed value belongs to the host's insert buffer, not here"
        );
    }

    #[test]
    fn submitting_with_no_prompt_outstanding_sends_nothing_and_says_so() {
        let mut p = two_users();
        assert_eq!(p.phase, Phase::Idle);
        p.commit_edit("", &tags::format_password("hunter2"));
        assert_eq!(p.phase, Phase::Idle, "phase must not advance");
        assert!(p.message.is_some(), "the user must be told why nothing happened");
    }

    // ---- tick vs needs_refresh ----

    /// Stated as a test so nobody moves the clock onto `tick`. A `true` tick
    /// rebuilds the tree even in Insert mode, and the symptom would be "my
    /// password disappears while I type it, once a minute".
    #[test]
    fn the_clock_refreshes_through_needs_refresh_never_through_tick() {
        let mut p = two_users();
        p.clock_minute = 0; // force the minute to look stale
        p.clock = "stale".to_owned();

        assert!(!p.tick(), "a clock change must not report through tick()");
        assert!(p.needs_refresh(), "it must report through needs_refresh()");
        p.clear_needs_refresh();
        assert!(!p.needs_refresh());
    }

    #[test]
    fn the_clock_reads_as_a_date_and_time() {
        // 2026-09-22 was a Tuesday. Rendered in local time, so assert shape
        // rather than an exact string.
        let s = format_clock(1_758_534_000);
        assert!(s.contains(','), "expected 'Day N Month, HH:MM', got {s}");
        let (date, time) = s.split_once(", ").unwrap();
        assert_eq!(date.split(' ').count(), 3, "weekday day month: {date}");
        assert_eq!(time.len(), 5, "HH:MM: {time}");
        assert!(!s.contains("2026"), "the year is deliberately omitted: {s}");
    }

    // ---- Against a real socket ----

    fn drive(p: &mut LoginProvider, want_phase: Phase) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while p.phase != want_phase && Instant::now() < deadline {
            p.tick();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn with_greetd(script: Vec<Step>) -> (LoginProvider, std::thread::JoinHandle<Vec<String>>) {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let (listener, path) = bind(dir.path());
        let server = std::thread::spawn(move || serve_script(&listener, script));
        let client = GreetdClient::connect_to(path.to_str().unwrap()).unwrap();
        let worker = GreetdWorker::spawn(client);
        let state = tempfile::tempdir().unwrap();
        let p = LoginProvider::new(
            vec![user("nico"), user("guest")],
            vec![session("desicompass", "Desicompass")],
            lastlogin::Store::load(state.path()),
            power::Commands::default(),
            Some(worker),
        );
        std::mem::forget(state);
        (p, server)
    }

    #[test]
    fn a_full_login_starts_the_session_and_sets_done() {
        let (mut p, server) = with_greetd(vec![
            Step::new("create_session", Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password:".into(),
            }),
            Step::new("hunter2", Response::Success),
            Step::new("start_session", Response::Success),
        ]);

        drive(&mut p, Phase::Prompting { secret: true });
        p.commit_edit("", &tags::format_password("hunter2"));
        drive(&mut p, Phase::Done);

        assert!(p.is_done());
        assert!(p.done_flag().load(Ordering::Relaxed), "the loop must be told to stop");

        let seen = server.join().unwrap();
        assert!(seen[2].contains(r#""cmd":["/bin/desicompass"]"#), "{}", seen[2]);
        assert!(seen[2].contains("XDG_SESSION_TYPE=wayland"), "{}", seen[2]);
    }

    #[test]
    fn a_wrong_password_reports_and_starts_a_fresh_attempt() {
        let (mut p, server) = with_greetd(vec![
            Step::new("create_session", Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password:".into(),
            }),
            Step::new("wrong", Response::Error {
                error_type: ErrorType::AuthError,
                description: "authentication error: PERM_DENIED".into(),
            }),
            // The provider starts over, so a second create_session follows.
            Step::new("create_session", Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password:".into(),
            }),
        ]);

        drive(&mut p, Phase::Prompting { secret: true });
        p.commit_edit("", &tags::format_password("wrong"));
        drive(&mut p, Phase::Waiting);
        // Let the retry land.
        let deadline = Instant::now() + Duration::from_secs(5);
        while p.message.is_none() && Instant::now() < deadline {
            p.tick();
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(p.message.as_deref(), Some("Wrong password. Try again."));
        assert!(!p.is_done());
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 3, "a failed attempt must be restarted, not left half-open");
        assert!(seen[2].contains("create_session"));
    }

    #[test]
    fn switching_user_cancels_the_session_that_was_being_configured() {
        let (mut p, server) = with_greetd(vec![
            Step::new("create_session", Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password:".into(),
            }),
            Step::new("cancel_session", Response::Success),
            Step::new("create_session", Response::AuthMessage {
                auth_message_type: AuthMessageType::Secret,
                auth_message: "Password:".into(),
            }),
        ]);

        drive(&mut p, Phase::Prompting { secret: true });
        p.on_radio_change(GROUP_USER, "guest");
        drive(&mut p, Phase::Prompting { secret: true });

        assert_eq!(p.selected_user, 1);
        let seen = server.join().unwrap();
        assert!(seen[1].contains("cancel_session"), "got {}", seen[1]);
        assert!(seen[2].contains(r#""username":"guest""#), "got {}", seen[2]);
    }
}
