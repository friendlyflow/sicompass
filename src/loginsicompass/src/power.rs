//! Suspend, restart and shut down from the greeter.
//!
//! These shell out to `systemctl`, which *is* the logind D-Bus call, made by a
//! process that is already on the system and exits in about 40 ms. Talking to
//! logind directly would mean adding `zbus` — a large dependency tree, an async
//! runtime question in a process that has deliberately avoided one, and a
//! second D-Bus client alongside the one `accesskit_unix` already runs — to
//! save one fork.
//!
//! Permissions need no polkit rule. greetd creates the greeter's PAM session
//! with `pam_systemd` and marks it `SessionClass::Greeter` on seat0, so it is
//! an *active local* session, and systemd's shipped policy grants
//! `org.freedesktop.login1.{power-off,reboot,suspend}` to `allow_active`.
//!
//! Commands are configurable because the greeter's `PATH` under greetd is not
//! something to bet a shutdown on: the NixOS module passes absolute paths.
//! Nothing here ever goes through a shell.

use std::process::{Command, Stdio};

/// The three actions, as pre-split argv.
#[derive(Debug, Clone)]
pub struct Commands {
    suspend: Vec<String>,
    reboot: Vec<String>,
    poweroff: Vec<String>,
}

/// The `<button>` function names the provider emits.
pub const SUSPEND: &str = "suspend";
pub const REBOOT: &str = "reboot";
pub const POWEROFF: &str = "poweroff";

impl Default for Commands {
    fn default() -> Self {
        Self {
            suspend: vec!["systemctl".into(), "suspend".into()],
            reboot: vec!["systemctl".into(), "reboot".into()],
            poweroff: vec!["systemctl".into(), "poweroff".into()],
        }
    }
}

impl Commands {
    /// Split each command line into an argv once, at startup.
    ///
    /// An unusable command is a startup log line, not a surprise at the moment
    /// someone tries to shut the machine down — so the error is reported here
    /// and the built-in default is kept.
    pub fn from_args(suspend: &str, reboot: &str, poweroff: &str) -> Self {
        let d = Self::default();
        Self {
            suspend: parse_one("suspend", suspend, d.suspend),
            reboot: parse_one("reboot", reboot, d.reboot),
            poweroff: parse_one("poweroff", poweroff, d.poweroff),
        }
    }

    fn argv(&self, which: &str) -> Option<&[String]> {
        match which {
            SUSPEND => Some(&self.suspend),
            REBOOT => Some(&self.reboot),
            POWEROFF => Some(&self.poweroff),
            _ => None,
        }
    }

    /// Spawn the action. Never waits: `systemctl poweroff` returns once the job
    /// is queued, and on suspend the greeter should stay responsive.
    pub fn run(&self, which: &str) -> std::io::Result<()> {
        let Some(argv) = self.argv(which) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown power action '{which}'"),
            ));
        };
        Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }

    /// The spoken line for an action, said *before* it is spawned so a screen
    /// reader gets it out while the screen is still up.
    pub fn announcement(which: &str) -> Option<&'static str> {
        match which {
            SUSPEND => Some("Suspending"),
            REBOOT => Some("Restarting"),
            POWEROFF => Some("Shutting down"),
            _ => None,
        }
    }

    /// Whether this action takes the machine down, and so should cancel the
    /// greetd session first rather than leave it mid-configuration.
    pub fn ends_the_session(which: &str) -> bool {
        matches!(which, REBOOT | POWEROFF)
    }
}

fn parse_one(label: &str, given: &str, fallback: Vec<String>) -> Vec<String> {
    let argv = split_words(given);
    if argv.is_empty() {
        tracing::warn!("empty --{label}-command; keeping the default {fallback:?}");
        return fallback;
    }
    argv
}

/// Split on whitespace, honouring single and double quotes so a store path with
/// a space in it survives. Not a shell: no expansion, no escapes beyond quotes.
fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match c {
            '\'' | '"' if quote.is_none() => {
                quote = Some(c);
                has = true;
            }
            c if Some(c) == quote => quote = None,
            c if c.is_whitespace() && quote.is_none() => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_systemctl() {
        let c = Commands::default();
        assert_eq!(c.argv(SUSPEND).unwrap(), ["systemctl", "suspend"]);
        assert_eq!(c.argv(REBOOT).unwrap(), ["systemctl", "reboot"]);
        assert_eq!(c.argv(POWEROFF).unwrap(), ["systemctl", "poweroff"]);
    }

    #[test]
    fn splits_absolute_paths_the_nix_module_passes() {
        let c = Commands::from_args(
            "/nix/store/abc-systemd/bin/systemctl suspend",
            "/nix/store/abc-systemd/bin/systemctl reboot",
            "/nix/store/abc-systemd/bin/systemctl poweroff",
        );
        assert_eq!(
            c.argv(POWEROFF).unwrap(),
            ["/nix/store/abc-systemd/bin/systemctl", "poweroff"]
        );
    }

    #[test]
    fn an_empty_command_keeps_the_default() {
        let c = Commands::from_args("", "   ", "systemctl poweroff");
        assert_eq!(c.argv(SUSPEND).unwrap(), ["systemctl", "suspend"]);
        assert_eq!(c.argv(REBOOT).unwrap(), ["systemctl", "reboot"]);
    }

    #[test]
    fn quoted_paths_survive_a_space() {
        assert_eq!(
            split_words(r#""/opt/my tools/systemctl" poweroff"#),
            vec!["/opt/my tools/systemctl", "poweroff"]
        );
        assert_eq!(
            split_words("'/opt/my tools/systemctl' reboot"),
            vec!["/opt/my tools/systemctl", "reboot"]
        );
    }

    #[test]
    fn unknown_action_is_an_error_not_a_panic() {
        let c = Commands::default();
        assert!(c.argv("format-disk").is_none());
        let err = c.run("format-disk").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn only_reboot_and_poweroff_end_the_session() {
        assert!(!Commands::ends_the_session(SUSPEND));
        assert!(Commands::ends_the_session(REBOOT));
        assert!(Commands::ends_the_session(POWEROFF));
    }

    #[test]
    fn every_action_has_something_to_say() {
        for a in [SUSPEND, REBOOT, POWEROFF] {
            assert!(Commands::announcement(a).is_some(), "{a} must be spoken");
        }
        assert_eq!(Commands::announcement("nonsense"), None);
    }
}
