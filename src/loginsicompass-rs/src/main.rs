//! loginsicompass — greetd login screen entry point.
//!
//! Rust port of `src/loginsicompass/main.c`.
//!
//! Connects to the Wayland compositor specified by `WAYLAND_DISPLAY`,
//! shows a full-screen login window, authenticates via greetd, and
//! launches the configured session command on success.

mod color;
mod entry;
mod greetd;
mod renderer;
mod state;

use clap::Parser;
use state::AppState;
use wayland_client::{globals::registry_queue_init, Connection};

use crate::{color::parse_hex, renderer::RenderConfig};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// loginsicompass — greetd login greeter.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Username to authenticate.
    #[arg(short = 'u', long, default_value = "nobody")]
    user: String,

    /// Shell command to run after successful login.
    #[arg(short = 'c', long, default_value = "false")]
    command: String,

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
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().init();
    }

    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
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

    // ---- Build app state ----
    let mut app = AppState::new(&conn, &globals, &qh, cfg, args.user.clone(), args.command.clone());

    // ---- Connect to greetd ----
    match greetd::GreetdClient::connect() {
        Ok(mut client) => {
            match client.create_session(&args.user) {
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
