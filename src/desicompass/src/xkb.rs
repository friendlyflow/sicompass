//! Which keyboard layout the compositor compiles.
//!
//! The compositor owns the keymap, so whatever it compiles is what every
//! client gets. `XkbConfig::default()` means US, silently, on any machine,
//! and on a Belgian keyboard that costs you `@`, `#`, `[`, `]`, `{` and `}`.
//!
//! The layout comes from the same place COSMIC's first-login setup reads it:
//! systemd-localed (`org.freedesktop.locale1`), which is what the OS
//! installer writes (`localectl set-x11-keymap`, or on NixOS
//! `services.xserver.xkb.*`). In order:
//!
//! 1. the `--xkb-*` flags, field by field, as explicit overrides
//! 2. localed over the system bus
//! 3. the files localed itself reads, for machines without a system bus
//!    or without localed
//! 4. `XKB_DEFAULT_*`
//! 5. nothing, which libxkbcommon turns into `us`
//!
//! A source counts only if it names a layout, and the first one that does
//! supplies the whole tuple: a variant from one source paired with a layout
//! from another would be a keymap nobody configured.
//!
//! Read once at startup. A change applies at the next login.

use std::fmt;

use smithay::input::keyboard::XkbConfig;

/// The RMLVO names of a keymap. `None` and an empty string both mean
/// "not set", which libxkbcommon treats as "use the default".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XkbNames {
    pub rules: Option<String>,
    pub model: Option<String>,
    pub layout: Option<String>,
    pub variant: Option<String>,
    pub options: Option<String>,
}

impl XkbNames {
    fn has_layout(&self) -> bool {
        self.layout.as_deref().is_some_and(|l| !l.is_empty())
    }

    /// Every field set in `overrides` replaces the one here.
    fn overridden_by(self, overrides: &XkbNames) -> XkbNames {
        let pick = |o: &Option<String>, s: Option<String>| o.clone().or(s);
        XkbNames {
            rules: pick(&overrides.rules, self.rules),
            model: pick(&overrides.model, self.model),
            layout: pick(&overrides.layout, self.layout),
            variant: pick(&overrides.variant, self.variant),
            options: pick(&overrides.options, self.options),
        }
    }

    pub fn as_config(&self) -> XkbConfig<'_> {
        XkbConfig {
            rules: self.rules.as_deref().unwrap_or_default(),
            model: self.model.as_deref().unwrap_or_default(),
            layout: self.layout.as_deref().unwrap_or_default(),
            variant: self.variant.as_deref().unwrap_or_default(),
            options: self.options.clone().filter(|o| !o.is_empty()),
        }
    }
}

impl fmt::Display for XkbNames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = |v: &Option<String>| v.clone().unwrap_or_default();
        write!(
            f,
            "layout={:?} variant={:?} model={:?} options={:?}",
            if self.has_layout() {
                s(&self.layout)
            } else {
                "us (default)".to_owned()
            },
            s(&self.variant),
            s(&self.model),
            s(&self.options),
        )
    }
}

/// Where the resolved layout came from, for the startup log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    CommandLine,
    Localed,
    File(&'static str),
    Environment,
    Default,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::CommandLine => f.write_str("command line"),
            Source::Localed => f.write_str("localed"),
            Source::File(path) => f.write_str(path),
            Source::Environment => f.write_str("XKB_DEFAULT_*"),
            Source::Default => f.write_str("libxkbcommon default"),
        }
    }
}

type Parser = fn(&str) -> XkbNames;

/// The files localed reads, in the order it prefers them. Each distro's
/// installer writes one of these.
const FILES: &[(&str, Parser)] = &[
    // NixOS (generated from services.xserver.xkb.*), and localed's own
    // X11 file on every systemd distro.
    ("/etc/X11/xorg.conf.d/00-keyboard.conf", parse_xorg_conf),
    // systemd >= 258 keeps the X11 keymap alongside the console one.
    ("/etc/vconsole.conf", parse_shell_kv),
    // Debian and Ubuntu.
    ("/etc/default/keyboard", parse_shell_kv),
];

/// Resolve the keymap against the live system.
pub fn resolve(overrides: &XkbNames) -> (XkbNames, Source) {
    resolve_with(
        overrides,
        from_localed,
        |path| std::fs::read_to_string(path).ok(),
        |name| std::env::var(name).ok(),
    )
}

fn resolve_with(
    overrides: &XkbNames,
    localed: impl FnOnce() -> Option<XkbNames>,
    read_file: impl Fn(&str) -> Option<String>,
    env: impl Fn(&str) -> Option<String>,
) -> (XkbNames, Source) {
    // A layout on the command line is an explicit choice: do not go asking
    // the system for one, and do not borrow its variant either.
    if overrides.has_layout() {
        return (overrides.clone(), Source::CommandLine);
    }

    let found = localed()
        .filter(XkbNames::has_layout)
        .map(|n| (n, Source::Localed))
        .or_else(|| {
            FILES.iter().find_map(|&(path, parse)| {
                read_file(path)
                    .map(|text| parse(&text))
                    .filter(XkbNames::has_layout)
                    .map(|n| (n, Source::File(path)))
            })
        })
        .or_else(|| {
            let names = XkbNames {
                rules: env("XKB_DEFAULT_RULES"),
                model: env("XKB_DEFAULT_MODEL"),
                layout: env("XKB_DEFAULT_LAYOUT"),
                variant: env("XKB_DEFAULT_VARIANT"),
                options: env("XKB_DEFAULT_OPTIONS"),
            };
            names.has_layout().then_some((names, Source::Environment))
        });

    match found {
        Some((names, source)) => (names.overridden_by(overrides), source),
        None => (XkbNames::default().overridden_by(overrides), Source::Default),
    }
}

/// Ask localed. Any failure (no system bus, no localed, a property
/// missing) is `None`, and the file fallback takes over.
fn from_localed() -> Option<XkbNames> {
    let conn = zbus::blocking::Connection::system().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.locale1",
        "/org/freedesktop/locale1",
        "org.freedesktop.locale1",
    )
    .ok()?;
    let get = |name: &str| -> Option<String> { proxy.get_property::<String>(name).ok() };
    Some(XkbNames {
        rules: None,
        model: get("X11Model"),
        layout: Some(get("X11Layout")?),
        variant: get("X11Variant"),
        options: get("X11Options"),
    })
}

/// `Option "XkbLayout" "be"` lines from an xorg.conf `InputClass` section.
fn parse_xorg_conf(text: &str) -> XkbNames {
    let mut names = XkbNames::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let Some(rest) = line.strip_prefix("Option") else {
            continue;
        };
        let mut quoted = rest.split('"').skip(1).step_by(2);
        let (Some(key), Some(value)) = (quoted.next(), quoted.next()) else {
            continue;
        };
        let slot = match key {
            "XkbRules" => &mut names.rules,
            "XkbModel" => &mut names.model,
            "XkbLayout" => &mut names.layout,
            "XkbVariant" => &mut names.variant,
            "XkbOptions" => &mut names.options,
            _ => continue,
        };
        *slot = Some(value.to_owned());
    }
    names
}

/// `XKBLAYOUT=be` lines, as in `/etc/default/keyboard` and
/// `/etc/vconsole.conf`. Values may be single- or double-quoted.
fn parse_shell_kv(text: &str) -> XkbNames {
    let mut names = XkbNames::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let slot = match key.trim() {
            "XKBRULES" => &mut names.rules,
            "XKBMODEL" => &mut names.model,
            "XKBLAYOUT" => &mut names.layout,
            "XKBVARIANT" => &mut names.variant,
            "XKBOPTIONS" => &mut names.options,
            _ => continue,
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        *slot = Some(value.to_owned());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_owned())
    }

    fn be() -> XkbNames {
        XkbNames {
            rules: None,
            model: s("pc104"),
            layout: s("be"),
            variant: s(""),
            options: s("terminate:ctrl_alt_bksp"),
        }
    }

    /// Exactly what NixOS generates from `services.xserver.xkb`.
    const NIXOS_XORG_CONF: &str = r#"Section "InputClass"
  Identifier "Keyboard catchall"
  MatchIsKeyboard "on"
  Option "XkbModel" "pc104"
  Option "XkbLayout" "be"
  Option "XkbOptions" "terminate:ctrl_alt_bksp"
  Option "XkbVariant" ""
EndSection
"#;

    #[test]
    fn parses_the_nixos_xorg_conf() {
        assert_eq!(parse_xorg_conf(NIXOS_XORG_CONF), be());
    }

    #[test]
    fn xorg_conf_ignores_comments_and_other_options() {
        let text = r#"
# Option "XkbLayout" "us"
Option "XkbLayout" "fr" # trailing
Option "AutoRepeat" "200 25"
"#;
        let names = parse_xorg_conf(text);
        assert_eq!(names.layout, s("fr"));
        assert_eq!(names.model, None);
    }

    #[test]
    fn parses_debian_default_keyboard() {
        let text = r#"# KEYBOARD CONFIGURATION FILE
XKBMODEL="pc105"
XKBLAYOUT="be"
XKBVARIANT=""
XKBOPTIONS=""

BACKSPACE="guess"
"#;
        let names = parse_shell_kv(text);
        assert_eq!(names.layout, s("be"));
        assert_eq!(names.model, s("pc105"));
        assert_eq!(names.variant, s(""));
        assert_eq!(names.as_config().options, None);
    }

    #[test]
    fn vconsole_without_xkb_keys_names_no_layout() {
        // This machine's vconsole.conf: a console keymap, no X11 keys.
        let names = parse_shell_kv("KEYMAP=be-latin1\n");
        assert!(!names.has_layout());
    }

    #[test]
    fn shell_kv_accepts_unquoted_and_single_quoted() {
        let names = parse_shell_kv("XKBLAYOUT=de\nXKBVARIANT='nodeadkeys'\n");
        assert_eq!(names.layout, s("de"));
        assert_eq!(names.variant, s("nodeadkeys"));
    }

    fn no_files(_: &str) -> Option<String> {
        None
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn localed_wins_over_files_and_env() {
        let (names, source) = resolve_with(
            &XkbNames::default(),
            || Some(be()),
            |_| Some("XKBLAYOUT=us\n".into()),
            |_| Some("us".into()),
        );
        assert_eq!(source, Source::Localed);
        assert_eq!(names, be());
    }

    #[test]
    fn a_layout_on_the_command_line_is_used_as_is() {
        let overrides = XkbNames {
            layout: s("fr"),
            ..Default::default()
        };
        let (names, source) = resolve_with(&overrides, || Some(be()), no_files, no_env);
        assert_eq!(source, Source::CommandLine);
        assert_eq!(names, overrides, "no model or options borrowed from localed");
    }

    #[test]
    fn other_flags_override_the_system_field_by_field() {
        let overrides = XkbNames {
            variant: s("nodeadkeys"),
            ..Default::default()
        };
        let (names, source) = resolve_with(&overrides, || Some(be()), no_files, no_env);
        assert_eq!(source, Source::Localed);
        assert_eq!(names.layout, s("be"));
        assert_eq!(names.model, s("pc104"));
        assert_eq!(names.variant, s("nodeadkeys"));
    }

    #[test]
    fn an_empty_localed_layout_falls_through_to_the_files() {
        let empty = XkbNames {
            layout: s(""),
            model: s("pc105"),
            ..Default::default()
        };
        let (names, source) = resolve_with(
            &XkbNames::default(),
            || Some(empty),
            |path| (path == "/etc/X11/xorg.conf.d/00-keyboard.conf").then(|| NIXOS_XORG_CONF.into()),
            no_env,
        );
        assert_eq!(source, Source::File("/etc/X11/xorg.conf.d/00-keyboard.conf"));
        assert_eq!(names, be(), "the whole tuple comes from the file");
    }

    #[test]
    fn the_first_file_with_a_layout_wins() {
        let (names, source) = resolve_with(
            &XkbNames::default(),
            || None,
            |path| match path {
                "/etc/vconsole.conf" => Some("KEYMAP=be-latin1\n".into()),
                "/etc/default/keyboard" => Some("XKBLAYOUT=\"be\"\n".into()),
                _ => None,
            },
            no_env,
        );
        assert_eq!(source, Source::File("/etc/default/keyboard"));
        assert_eq!(names.layout, s("be"));
    }

    #[test]
    fn falls_back_to_the_environment_then_the_default() {
        let (names, source) = resolve_with(
            &XkbNames::default(),
            || None,
            no_files,
            |name| (name == "XKB_DEFAULT_LAYOUT").then(|| "nl".into()),
        );
        assert_eq!(source, Source::Environment);
        assert_eq!(names.layout, s("nl"));

        let (names, source) = resolve_with(&XkbNames::default(), || None, no_files, no_env);
        assert_eq!(source, Source::Default);
        assert_eq!(names.as_config().layout, "");
    }
}
