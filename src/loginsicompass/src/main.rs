//! loginsicompass — greetd login screen entry point.
//!
//! Rust port of `src/loginsicompass-c/main.c`.
//!
//! Linux-only: connects to the Wayland compositor specified by `WAYLAND_DISPLAY`,
//! shows a full-screen login window, authenticates via greetd, and
//! launches the configured session command on success.

#[cfg(target_os = "linux")]
mod auth;
#[cfg(target_os = "linux")]
mod fakegreetd;
#[cfg(target_os = "linux")]
mod fallback;
#[cfg(target_os = "linux")]
mod greetd;
#[cfg(target_os = "linux")]
mod gui;
#[cfg(target_os = "linux")]
mod lastlogin;
#[cfg(target_os = "linux")]
mod power;
#[cfg(target_os = "linux")]
mod provider;
#[cfg(target_os = "linux")]
mod sessions;
#[cfg(target_os = "linux")]
mod supervisor;
#[cfg(target_os = "linux")]
mod users;

#[cfg(target_os = "linux")]
mod linux {
    use clap::Parser;
    use crate::fallback::state::AppState;
    use wayland_client::{globals::registry_queue_init, Connection};

    use crate::fallback::{color::parse_hex, renderer::RenderConfig};

    // ---------------------------------------------------------------------------
    // CLI
    // ---------------------------------------------------------------------------

    /// loginsicompass — greetd login greeter.
    #[derive(Parser, Debug)]
    #[command(version, about)]
    pub struct Args {
        // `--user` and `--command` used to live here, defaulting to `nobody`
        // and `false`. Nothing passed them, so the greeter authenticated an
        // account that cannot log in and then launched `/bin/false`. Both the
        // graphical greeter and the software fallback now enumerate users and
        // sessions themselves; `--user-extra` below is the escape hatch for a
        // host whose accounts are not in /etc/passwd.

        /// Path to a PNG/JPEG background image.
        #[arg(short = 'b', long)]
        background_image: Option<String>,

        /// Background colour as hex (#RRGGBB or #RRGGBBAA).
        #[arg(short = 'B', long, default_value = "#e3ccD2")]
        background_color: String,

        /// Border width in pixels.
        #[arg(short = 'r', long, default_value_t = 6)]
        border_width: u32,

        /// Border colour as hex.
        #[arg(short = 'R', long, default_value = "#f92672")]
        border_color: String,

        /// Outline width in pixels.
        #[arg(short = 'o', long, default_value_t = 2)]
        outline_width: u32,

        /// Outline colour as hex.
        #[arg(short = 'O', long, default_value = "#080800")]
        outline_color: String,

        /// Entry padding in pixels.
        #[arg(short = 'e', long, default_value_t = 8)]
        entry_padding: u32,

        /// Entry background colour as hex.
        #[arg(short = 'E', long, default_value = "#1b1d1e")]
        entry_color: String,

        /// Text/dot foreground colour as hex.
        #[arg(short = 'T', long, default_value = "#ffffff")]
        text_color: String,

        /// Number of character slots in the entry box.
        #[arg(short = 'n', long, default_value_t = 12)]
        width_characters: u32,

        /// Window width in pixels.
        #[arg(long, default_value_t = 640)]
        width: u32,

        /// Window height in pixels.
        #[arg(long, default_value_t = 480)]
        height: u32,

        // ---- The sicompass-stack greeter ---------------------------------
        /// Which renderer to use.
        ///
        /// Omitted, the process is the *supervisor*: it re-execs itself as
        /// `gpu` and falls back to `shm` if that dies. See `supervisor.rs`.
        #[arg(long, value_enum)]
        render_backend: Option<crate::Backend>,

        /// Directory for `last.json` (the remembered user and session).
        #[arg(long, default_value = "/var/lib/loginsicompass")]
        state_dir: std::path::PathBuf,

        /// Extra directory to scan for session `.desktop` files. Repeatable.
        #[arg(long)]
        sessions_dir: Vec<std::path::PathBuf>,

        /// Offer this user in addition to those found in /etc/passwd.
        /// Repeatable; for hosts whose accounts are not in the passwd file.
        #[arg(long = "user-extra")]
        user_extra: Vec<String>,

        #[arg(long, default_value = "systemctl suspend")]
        suspend_command: String,
        #[arg(long, default_value = "systemctl reboot")]
        reboot_command: String,
        #[arg(long, default_value = "systemctl poweroff")]
        poweroff_command: String,
    }

    pub fn main_impl() -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        } else {
            tracing_subscriber::fmt().init();
        }

        let args = Args::parse();

        // Three roles in one binary, selected by one flag. See `crate::Backend`.
        match args.render_backend {
            Some(crate::Backend::Gpu) => return run_gpu(&args),
            // `shm` is the software fallback: today's tiny-skia password box,
            // reached when the Vulkan path could not start.
            Some(crate::Backend::Shm) => return run_shm(args),
            // No flag: this process is the supervisor. It re-execs itself as
            // `gpu`, and falls back to `shm` once if that dies without having
            // started a session.
            None => std::process::exit(crate::supervisor::run(&supervisor_argv())),
        }
    }

    /// The arguments to hand a supervised child: everything this process was
    /// given, minus its own name. `--render-backend` is added by the
    /// supervisor, so it must not already be there — and it cannot be, because
    /// the supervisor role is the one with no `--render-backend`.
    fn supervisor_argv() -> Vec<String> {
        std::env::args().skip(1).collect()
    }

    /// The sicompass-stack greeter.
    fn run_gpu(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
        let opts = crate::gui::Options {
            state_dir: args.state_dir.clone(),
            session_dirs: args.sessions_dir.clone(),
            extra_users: args.user_extra.clone(),
            power: crate::power::Commands::from_args(
                &args.suspend_command,
                &args.reboot_command,
                &args.poweroff_command,
            ),
        };
        let started = crate::gui::run(&opts)?;
        if started {
            // Tell the supervisor before exiting, so it does not read this as
            // a crash and start the fallback over the top of the session that
            // is now coming up.
            crate::supervisor::signal_done();
        } else {
            tracing::warn!("the greeter exited without starting a session");
        }
        Ok(())
    }

    /// Who the software fallback should log in, and into what.
    ///
    /// The fallback cannot show a picker (it draws no text at all), so it has to
    /// be *told*. Before this existed it read `--user` and `--command`, whose
    /// defaults were `nobody` and `false` — which is precisely the bug that
    /// made the old greeter unable to log anyone in. It now resolves the same
    /// way the graphical greeter does, so falling back changes how the login
    /// screen looks and not who it logs in.
    fn resolve_fallback_target(args: &Args) -> (String, Vec<String>, Vec<String>) {
        let last = crate::lastlogin::Store::load(&args.state_dir);

        let users = crate::users::enumerate();
        let names: Vec<String> = users.iter().map(|u| u.name.clone()).collect();
        let username = args
            .user_extra
            .first()
            .cloned()
            .or_else(|| names.get(crate::lastlogin::index_of(&names, last.user())).cloned())
            .unwrap_or_default();

        let sessions = crate::sessions::enumerate(&args.sessions_dir);
        let ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
        let session = sessions.get(crate::lastlogin::index_of(&ids, last.session()));

        match session {
            Some(s) => (username, s.exec.clone(), s.env()),
            None => {
                tracing::error!("no session to start; the fallback can only show a prompt");
                (username, Vec::new(), Vec::new())
            }
        }
    }

    fn run_shm(args: Args) -> Result<(), Box<dyn std::error::Error>> {
        // ---- Wayland connection ----
        let conn = Connection::connect_to_env()?;
        let (globals, mut event_queue) = registry_queue_init(&conn)?;
        let qh = event_queue.handle();

        // ---- Build render config ----
        let mut cfg = RenderConfig {
            width: args.width,
            height: args.height,
            border_width: args.border_width,
            outline_width: args.outline_width,
            padding: args.entry_padding,
            num_characters: args.width_characters,
            ..RenderConfig::default()
        };

        if let Some(c) = parse_hex(&args.background_color) {
            cfg.background_color = c;
        }
        if let Some(c) = parse_hex(&args.border_color) {
            cfg.border_color = c;
        }
        if let Some(c) = parse_hex(&args.outline_color) {
            cfg.outline_color = c;
        }
        if let Some(c) = parse_hex(&args.entry_color) {
            cfg.entry_background = c;
        }
        if let Some(c) = parse_hex(&args.text_color) {
            cfg.entry_foreground = c;
        }

        if let Some(path) = &args.background_image {
            match image::open(path) {
                Ok(img) => cfg.background_image = Some(img.to_rgba8()),
                Err(e) => tracing::warn!("failed to load background image {path}: {e}"),
            }
        }

        // ---- Who to log in, and into what ----
        let (username, session_cmd, session_env) = resolve_fallback_target(&args);
        tracing::info!(
            "software fallback will authenticate {username:?} into {:?}",
            session_cmd.first()
        );

        // ---- Build app state ----
        let mut app = AppState::new(
            &conn,
            &globals,
            &qh,
            cfg,
            username.clone(),
            session_cmd,
            session_env,
        );

        // ---- Connect to greetd ----
        match crate::greetd::GreetdClient::connect() {
            Ok(mut client) => {
                match client.create_session(&username) {
                    Ok(resp) => {
                        app.greetd = Some(client);
                        app.handle_response_pub(resp);
                    }
                    Err(e) => {
                        tracing::error!("greetd create_session failed: {e}");
                        app.greetd = Some(client);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("could not connect to greetd ({e}); running without authentication");
            }
        }

        // ---- Main event loop ----
        loop {
            event_queue.blocking_dispatch(&mut app)?;

            if app.exit {
                break;
            }

            // Submit the password outside the keyboard handler to avoid borrow issues.
            if app.submit_pending {
                app.submit_pending = false;
                app.submit();
            }
        }

        tracing::info!("loginsicompass exiting");
        Ok(())
    }
}

/// Which renderer a child process should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Backend {
    /// `sicompass-ui`: SDL3 + Vulkan, radio lists, AccessKit, embedded fonts.
    Gpu,
    /// Shared-memory software rendering: the original tiny-skia password box.
    /// No text, no accessibility — a last resort so a Vulkan failure at boot
    /// is not a black screen.
    Shm,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    return linux::main_impl();

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("loginsicompass is Linux-only (Wayland/greetd greeter)");
        std::process::exit(1);
    }
}
