//! Enumerating the sessions the greeter offers to start.
//!
//! Sessions are `.desktop` files under `<data dir>/wayland-sessions` and
//! `<data dir>/xsessions`. This is a deliberately small parser rather than the
//! `freedesktop-desktop-entry` crate: only one group (`[Desktop Entry]`) and a
//! handful of keys matter, and the crate brings a locale-matching dependency
//! tree for what is otherwise 100 lines of INI.
//!
//! The load-bearing part is [`split_exec`]. greetd's `start_session` takes an
//! **argv**, not a shell command line, so an `Exec=` has to be split before it
//! is sent or greetd will `execve` a file whose name is the whole line. On a
//! NixOS host the real `desicompass.desktop` is ten words long:
//!
//! ```text
//! Exec=/nix/store/…/systemd-cat --identifier=desicompass /nix/store/…/dbus-run-session \
//!      /nix/store/…/desicompass --backend tty --xkb-layout be --startup-cmd /nix/store/…-startup
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Wayland,
    X11,
}

impl SessionType {
    /// The value for `XDG_SESSION_TYPE`.
    pub fn xdg_value(self) -> &'static str {
        match self {
            SessionType::Wayland => "wayland",
            SessionType::X11 => "x11",
        }
    }
}

/// One offerable session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    /// Desktop-file stem, e.g. `desicompass`. This is the stable key: it is
    /// what gets remembered across reboots, because `name` is locale-dependent.
    pub id: String,
    /// `Name=`, locale-matched. What the list shows.
    pub name: String,
    /// `Exec=`, split into an argv with field codes removed.
    pub exec: Vec<String>,
    /// `DesktopNames=`, for `XDG_CURRENT_DESKTOP`. Empty when absent.
    pub desktop_names: String,
    pub session_type: SessionType,
}

impl SessionEntry {
    /// The environment greetd should add for this session.
    ///
    /// Without these a session comes up subtly wrong: portals pick the wrong
    /// backend, theming misses, and `loginctl` reports no desktop.
    pub fn env(&self) -> Vec<String> {
        let mut env = vec![
            format!("XDG_SESSION_TYPE={}", self.session_type.xdg_value()),
            format!("XDG_SESSION_DESKTOP={}", self.id),
        ];
        if !self.desktop_names.is_empty() {
            env.push(format!("XDG_CURRENT_DESKTOP={}", self.desktop_names));
        }
        env
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse one `.desktop` file.
///
/// Returns `None` when the entry is not an offerable session: a `Type` other
/// than `Application`, `Hidden=true`, `NoDisplay=true`, or no usable `Exec`.
/// `locale` is the value of `$LANG`-style locale to prefer for `Name[..]`.
pub fn parse_desktop_entry(
    contents: &str,
    id: &str,
    session_type: SessionType,
    locale: Option<&str>,
) -> Option<SessionEntry> {
    let mut in_entry = false;
    let mut name: Option<String> = None;
    let mut localized: Vec<(String, String)> = Vec::new();
    let mut exec: Option<String> = None;
    let mut desktop_names = String::new();
    let mut ty: Option<String> = None;
    let mut hidden = false;
    let mut no_display = false;

    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            // Only the [Desktop Entry] group matters; action groups and
            // vendor groups are ignored entirely.
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Name" => name = Some(value.to_owned()),
            "Exec" => exec = Some(value.to_owned()),
            "DesktopNames" => desktop_names = value.to_owned(),
            "Type" => ty = Some(value.to_owned()),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            k if k.starts_with("Name[") && k.ends_with(']') => {
                let tag = &k["Name[".len()..k.len() - 1];
                localized.push((tag.to_owned(), value.to_owned()));
            }
            _ => {}
        }
    }

    if hidden || no_display {
        return None;
    }
    // A missing Type is tolerated (some session files omit it); an explicit
    // non-Application one is not.
    if let Some(ty) = &ty {
        if ty != "Application" {
            return None;
        }
    }

    let exec = exec?;
    let argv = split_exec(&exec);
    if argv.is_empty() {
        return None;
    }

    let name = pick_name(name, &localized, locale)?;

    Some(SessionEntry {
        id: id.to_owned(),
        name,
        exec: argv,
        desktop_names,
        session_type,
    })
}

/// Choose `Name`, preferring an exact locale match then its language part.
///
/// Full locale matching (the spec's `lang_COUNTRY.ENCODING@MODIFIER` ordering)
/// is more than a session list needs: `Name[nl_BE]`, then `Name[nl]`, then the
/// bare `Name` covers every real session file.
fn pick_name(
    base: Option<String>,
    localized: &[(String, String)],
    locale: Option<&str>,
) -> Option<String> {
    if let Some(loc) = locale {
        let loc = loc.split('.').next().unwrap_or(loc);
        if let Some((_, v)) = localized.iter().find(|(t, _)| t == loc) {
            return Some(v.clone());
        }
        let lang = loc.split('_').next().unwrap_or(loc);
        if let Some((_, v)) = localized.iter().find(|(t, _)| t == lang) {
            return Some(v.clone());
        }
    }
    base
}

/// Split an `Exec=` value into an argv, per the Desktop Entry specification.
///
/// Handles double-quote grouping, backslash escapes inside quotes, `%%` as a
/// literal `%`, and drops the field codes (`%f %F %u %U %i %c %k` and the
/// deprecated `%d %D %n %N %v %m`), which have no meaning for a session.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_cur = false;
    let mut in_quotes = false;
    let mut chars = exec.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                // An empty "" is still an argument.
                has_cur = true;
            }
            '\\' if in_quotes => {
                // Inside quotes the spec escapes ", `, $ and \ itself.
                if let Some(next) = chars.next() {
                    cur.push(next);
                    has_cur = true;
                }
            }
            '%' => match chars.next() {
                Some('%') => {
                    cur.push('%');
                    has_cur = true;
                }
                // A field code expands to nothing here.
                Some(_) => {}
                None => {}
            },
            c if c.is_whitespace() && !in_quotes => {
                if has_cur {
                    out.push(std::mem::take(&mut cur));
                    has_cur = false;
                }
            }
            c => {
                cur.push(c);
                has_cur = true;
            }
        }
    }
    if has_cur {
        out.push(cur);
    }
    // A field code alone (`foo %U`) leaves an empty trailing argument behind.
    while out.last().is_some_and(|s| s.is_empty()) {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// The directories to search, in precedence order, each tagged with the kind of
/// session it holds.
///
/// `$XDG_DATA_DIRS` when set, else the spec default. `/run/current-system/sw/share`
/// is appended unconditionally because that is where a NixOS host keeps them and
/// it is not always in `$XDG_DATA_DIRS` for a greeter's minimal environment.
pub fn search_dirs(xdg_data_dirs: Option<&str>, extra: &[PathBuf]) -> Vec<(PathBuf, SessionType)> {
    let mut bases: Vec<PathBuf> = Vec::new();
    let list = xdg_data_dirs.filter(|s| !s.is_empty());
    match list {
        Some(s) => bases.extend(s.split(':').filter(|p| !p.is_empty()).map(PathBuf::from)),
        None => {
            bases.push(PathBuf::from("/usr/local/share"));
            bases.push(PathBuf::from("/usr/share"));
        }
    }
    let nixos = PathBuf::from("/run/current-system/sw/share");
    if !bases.contains(&nixos) {
        bases.push(nixos);
    }

    let mut out: Vec<(PathBuf, SessionType)> = Vec::new();
    // Explicit --sessions-dir entries win over the search path.
    for dir in extra {
        out.push((dir.clone(), classify_dir(dir)));
    }
    for base in bases {
        out.push((base.join("wayland-sessions"), SessionType::Wayland));
        out.push((base.join("xsessions"), SessionType::X11));
    }
    out
}

/// A `--sessions-dir` may point straight at a `wayland-sessions` or
/// `xsessions` directory; the trailing component says which.
fn classify_dir(dir: &Path) -> SessionType {
    match dir.file_name().and_then(|s| s.to_str()) {
        Some("xsessions") => SessionType::X11,
        _ => SessionType::Wayland,
    }
}

/// Scan the search path. First occurrence of a desktop-file id wins.
pub fn enumerate(extra_dirs: &[PathBuf]) -> Vec<SessionEntry> {
    let xdg = std::env::var("XDG_DATA_DIRS").ok();
    let locale = std::env::var("LANG").ok();
    let dirs = search_dirs(xdg.as_deref(), extra_dirs);
    let mut out: Vec<SessionEntry> = Vec::new();

    for (dir, ty) in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("desktop"))
            .collect();
        entries.sort();
        for path in entries {
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if out.iter().any(|s| s.id == id) {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            match parse_desktop_entry(&contents, id, ty, locale.as_deref()) {
                Some(entry) => out.push(entry),
                None => tracing::debug!("skipping session file {}", path.display()),
            }
        }
    }
    disambiguate(&mut out);
    out
}

/// Two sessions with the same `Name=` would be indistinguishable in the list,
/// and `on_radio_change` identifies a choice by its label. Append the id to
/// every member of a colliding set.
fn disambiguate(sessions: &mut [SessionEntry]) {
    let names: Vec<String> = sessions.iter().map(|s| s.name.clone()).collect();
    for (i, s) in sessions.iter_mut().enumerate() {
        if names.iter().enumerate().any(|(j, n)| j != i && *n == names[i]) {
            s.name = format!("{} ({})", s.name, s.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from `/run/current-system/sw/share/wayland-sessions/` on the
    /// machine this was written on. The `Exec=` is the ten-word line that
    /// makes `split_exec` necessary.
    const DESICOMPASS: &str = r#"[Desktop Entry]
Name=Desicompass
Comment=Use your whole computer from the keyboard, with no mouse needed
Exec=/nix/store/psv7zqls5fm0glvlnq7i29s3yr4m6rkg-systemd-260.4/bin/systemd-cat --identifier=desicompass /nix/store/w9gn9sy71j4v3jia681vvx5j4d7f5ly7-dbus-1.16.2/bin/dbus-run-session /nix/store/ixhal5zkn1d5vif47hp2rk50i3dsdcw4-desicompass-0.1.21/bin/desicompass --backend tty --xkb-layout be --startup-cmd /nix/store/xgf5kk9zvc1fhygcw7nn8252as8d96k6-desicompass-startup
Type=Application
DesktopNames=Desicompass
"#;

    const COSMIC: &str = r#"[Desktop Entry]
Name=COSMIC
Comment=This session logs you into the COSMIC desktop
Comment[sv]=Denna session loggar in dig till skrivbordsmiljön COSMIC
Exec=/nix/store/pnsza0q2zzy9l2abz96q8s7cdfng0l3y-cosmic-session-1.2.0/bin/start-cosmic
Type=Application
DesktopNames=COSMIC
"#;

    // ---- Real files ----

    #[test]
    fn parses_the_real_desicompass_entry() {
        let e = parse_desktop_entry(DESICOMPASS, "desicompass", SessionType::Wayland, None)
            .expect("desicompass.desktop must parse");
        assert_eq!(e.id, "desicompass");
        assert_eq!(e.name, "Desicompass");
        assert_eq!(e.desktop_names, "Desicompass");
        // Ten words, and the first must be an executable path on its own.
        assert_eq!(e.exec.len(), 10, "argv was {:?}", e.exec);
        assert!(e.exec[0].ends_with("/bin/systemd-cat"));
        assert_eq!(e.exec[1], "--identifier=desicompass");
        assert!(e.exec[2].ends_with("/bin/dbus-run-session"));
        assert!(e.exec[3].ends_with("/bin/desicompass"));
        assert_eq!(e.exec[4], "--backend");
        assert_eq!(e.exec[5], "tty");
        assert_eq!(e.exec[8], "--startup-cmd");
        assert_eq!(
            e.exec[9],
            "/nix/store/xgf5kk9zvc1fhygcw7nn8252as8d96k6-desicompass-startup"
        );
    }

    #[test]
    fn parses_the_real_cosmic_entry() {
        let e = parse_desktop_entry(COSMIC, "cosmic", SessionType::Wayland, None).unwrap();
        assert_eq!(e.name, "COSMIC");
        assert_eq!(e.exec.len(), 1);
        assert!(e.exec[0].ends_with("/bin/start-cosmic"));
    }

    #[test]
    fn builds_the_session_environment() {
        let e = parse_desktop_entry(DESICOMPASS, "desicompass", SessionType::Wayland, None).unwrap();
        assert_eq!(
            e.env(),
            vec![
                "XDG_SESSION_TYPE=wayland",
                "XDG_SESSION_DESKTOP=desicompass",
                "XDG_CURRENT_DESKTOP=Desicompass",
            ]
        );
    }

    #[test]
    fn x11_session_environment_says_x11() {
        let e = parse_desktop_entry(COSMIC, "cosmic", SessionType::X11, None).unwrap();
        assert_eq!(e.env()[0], "XDG_SESSION_TYPE=x11");
    }

    #[test]
    fn desktop_names_absent_omits_current_desktop() {
        let s = "[Desktop Entry]\nName=Bare\nExec=/bin/bare\nType=Application\n";
        let e = parse_desktop_entry(s, "bare", SessionType::Wayland, None).unwrap();
        assert_eq!(e.env().len(), 2);
        assert!(!e.env().iter().any(|v| v.starts_with("XDG_CURRENT_DESKTOP")));
    }

    // ---- split_exec ----

    #[test]
    fn exec_quoting_and_field_codes() {
        assert_eq!(split_exec(r#"foo "a b" %U"#), vec!["foo", "a b"]);
        assert_eq!(split_exec("prog --flag  value"), vec!["prog", "--flag", "value"]);
        assert_eq!(split_exec("prog %f %F %u %i %c %k"), vec!["prog"]);
        assert_eq!(split_exec("prog 100%%"), vec!["prog", "100%"]);
        assert_eq!(split_exec(r#"prog "quoted \"inner\"""#), vec!["prog", r#"quoted "inner""#]);
        assert_eq!(split_exec(""), Vec::<String>::new());
        assert_eq!(split_exec("   "), Vec::<String>::new());
    }

    #[test]
    fn exec_keeps_an_intentionally_empty_argument() {
        assert_eq!(split_exec(r#"prog "" tail"#), vec!["prog", "", "tail"]);
    }

    // ---- Rejection ----

    #[test]
    fn rejects_hidden_nodisplay_wrong_type_and_missing_exec() {
        let cases = [
            "[Desktop Entry]\nName=X\nExec=/bin/x\nType=Application\nHidden=true\n",
            "[Desktop Entry]\nName=X\nExec=/bin/x\nType=Application\nNoDisplay=true\n",
            "[Desktop Entry]\nName=X\nExec=/bin/x\nType=Link\n",
            "[Desktop Entry]\nName=X\nType=Application\n",
            "[Desktop Entry]\nName=X\nExec=%U\nType=Application\n",
        ];
        for (i, c) in cases.iter().enumerate() {
            assert!(
                parse_desktop_entry(c, "x", SessionType::Wayland, None).is_none(),
                "case {i} should have been rejected"
            );
        }
    }

    #[test]
    fn hidden_false_is_not_hidden() {
        let s = "[Desktop Entry]\nName=X\nExec=/bin/x\nType=Application\nHidden=false\n";
        assert!(parse_desktop_entry(s, "x", SessionType::Wayland, None).is_some());
    }

    #[test]
    fn missing_type_is_tolerated() {
        let s = "[Desktop Entry]\nName=X\nExec=/bin/x\n";
        assert!(parse_desktop_entry(s, "x", SessionType::Wayland, None).is_some());
    }

    #[test]
    fn keys_outside_the_desktop_entry_group_are_ignored() {
        let s = "\
[Desktop Entry]
Name=Real
Exec=/bin/real
Type=Application

[Desktop Action Other]
Name=Decoy
Exec=/bin/decoy
";
        let e = parse_desktop_entry(s, "x", SessionType::Wayland, None).unwrap();
        assert_eq!(e.name, "Real");
        assert_eq!(e.exec, vec!["/bin/real"]);
    }

    // ---- Locale ----

    #[test]
    fn locale_picks_the_translated_name() {
        let s = "\
[Desktop Entry]
Name=Session
Name[nl]=Sessie
Name[nl_BE]=Sessie (BE)
Exec=/bin/s
Type=Application
";
        let pick = |loc: Option<&str>| {
            parse_desktop_entry(s, "s", SessionType::Wayland, loc)
                .unwrap()
                .name
        };
        assert_eq!(pick(Some("nl_BE.UTF-8")), "Sessie (BE)");
        assert_eq!(pick(Some("nl_NL.UTF-8")), "Sessie"); // falls back to language
        assert_eq!(pick(Some("fr_BE.UTF-8")), "Session"); // falls back to bare Name
        assert_eq!(pick(None), "Session");
    }

    // ---- Search path ----

    #[test]
    fn search_path_defaults_when_xdg_data_dirs_is_unset() {
        let dirs = search_dirs(None, &[]);
        let paths: Vec<String> = dirs.iter().map(|(p, _)| p.display().to_string()).collect();
        assert!(paths.contains(&"/usr/share/wayland-sessions".to_string()));
        assert!(paths.contains(&"/usr/share/xsessions".to_string()));
        // NixOS' location is always searched, even though it is not in the
        // spec's default, because that is where this machine keeps them.
        assert!(paths.contains(&"/run/current-system/sw/share/wayland-sessions".to_string()));
    }

    #[test]
    fn search_path_honours_xdg_data_dirs() {
        let dirs = search_dirs(Some("/a:/b"), &[]);
        let paths: Vec<String> = dirs.iter().map(|(p, _)| p.display().to_string()).collect();
        assert!(paths.contains(&"/a/wayland-sessions".to_string()));
        assert!(paths.contains(&"/b/xsessions".to_string()));
        assert!(!paths.contains(&"/usr/share/wayland-sessions".to_string()));
    }

    #[test]
    fn explicit_dirs_come_first_and_are_classified_by_name() {
        let extra = vec![PathBuf::from("/e/xsessions"), PathBuf::from("/e/wayland-sessions")];
        let dirs = search_dirs(Some("/a"), &extra);
        assert_eq!(dirs[0], (PathBuf::from("/e/xsessions"), SessionType::X11));
        assert_eq!(dirs[1], (PathBuf::from("/e/wayland-sessions"), SessionType::Wayland));
    }

    // ---- Disambiguation ----

    #[test]
    fn colliding_display_names_get_their_id_appended() {
        let mk = |id: &str, name: &str| SessionEntry {
            id: id.into(),
            name: name.into(),
            exec: vec!["/bin/x".into()],
            desktop_names: String::new(),
            session_type: SessionType::Wayland,
        };
        let mut v = vec![mk("a", "Plasma"), mk("b", "Plasma"), mk("c", "Sway")];
        disambiguate(&mut v);
        assert_eq!(v[0].name, "Plasma (a)");
        assert_eq!(v[1].name, "Plasma (b)");
        assert_eq!(v[2].name, "Sway", "a unique name is left alone");
    }
}
