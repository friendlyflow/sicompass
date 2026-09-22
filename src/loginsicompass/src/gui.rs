//! The greeter, drawn by `sicompass-ui`.
//!
//! This is the `--render-backend gpu` role: an SDL3 + Vulkan window running the
//! app's own renderer with exactly one provider in it. Everything that makes
//! the login screen usable by a screen-reader user — the AccessKit tree, the
//! masked `<password>`, the spoken per-keystroke echo, the live region — comes
//! from that renderer and is not reimplemented here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sicompass_sdk::ffon::IdArray;
use sicompass_ui::app_state::{AppConfig, AppState};
use sicompass_ui::registry::HostHooks;

use crate::auth::GreetdWorker;
use crate::greetd::GreetdClient;
use crate::provider::LoginProvider;
use crate::{lastlogin, power, sessions, users};

/// The greeter's answers to the renderer.
///
/// Five of the six defaults are already right: there is no settings file, no
/// updater and no second tab to open. Only quitting needs an answer of its own.
struct GreeterHooks {
    done: Arc<AtomicBool>,
}

impl HostHooks for GreeterHooks {
    fn should_quit(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
}

/// What the greeter needs from the command line.
pub struct Options {
    pub state_dir: std::path::PathBuf,
    pub session_dirs: Vec<std::path::PathBuf>,
    pub extra_users: Vec<String>,
    pub power: power::Commands,
}

/// Build and run the login screen. Returns once greetd has taken the session
/// (or the user powered the machine off).
pub fn run(opts: &Options) -> Result<bool, String> {
    let mut users = users::enumerate();
    for name in &opts.extra_users {
        if !users.iter().any(|u| &u.name == name) {
            users.push(users::UserEntry {
                name: name.clone(),
                uid: 0,
                full_name: None,
                shell: String::new(),
            });
        }
    }
    let sessions = sessions::enumerate(&opts.session_dirs);
    tracing::info!(
        "offering {} user(s) and {} session(s)",
        users.len(),
        sessions.len()
    );

    let last = lastlogin::Store::load(&opts.state_dir);

    // No greetd is not fatal: the UI still comes up, which is what makes it
    // possible to run the greeter nested during development.
    let worker = match GreetdClient::connect() {
        Ok(client) => Some(GreetdWorker::spawn(client)),
        Err(e) => {
            tracing::warn!("no greetd connection ({e}); authentication is disabled");
            None
        }
    };

    let provider = LoginProvider::new(users, sessions, last, opts.power.clone(), worker);
    let password_row = provider.password_row();
    let done = Arc::clone(provider.done_flag());

    let cfg = AppConfig {
        title: "Sign in".to_owned(),
        app_name: "Loginsicompass".to_owned(),
        app_id: "loginsicompass".to_owned(),
        vulkan_app_name: "loginsicompass".to_owned(),
        // The compositor gives the greeter the whole output, but say so anyway:
        // this also works under a nested compositor for testing.
        fullscreen: true,
        // There is no pointer to click a titlebar with and nothing to minimise
        // to, so the app's own window controls would be dead pixels.
        custom_titlebar: false,
        maximized: false,
        window_icon: false,
        ..AppConfig::default()
    };

    let mut app = AppState::with_providers(
        &cfg,
        vec![Box::new(provider)],
        Box::new(GreeterHooks {
            done: Arc::clone(&done),
        }),
    )
    .map_err(|e| format!("could not start the graphical greeter: {e}"))?;

    // Land the cursor on the password field, the way the app lands a first-run
    // user on the onboarding line (`programs::focus_onboarding`).
    let mut id = IdArray::new();
    id.push(0);
    id.push(password_row);
    app.renderer.current_id = id;
    sicompass_ui::list::create_list_current_layer(&mut app.renderer);

    app.run();

    Ok(done.load(Ordering::Relaxed))
}
