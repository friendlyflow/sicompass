//! Enumerating the users the greeter offers to log in.
//!
//! greetd owns PAM, so nothing here needs to read `/etc/shadow` and the greeter
//! needs no privileged helper: `/etc/passwd` and `/etc/login.defs` are both
//! world-readable. That is the whole reason this file is 100 lines rather than
//! a D-Bus service — cosmic-greeter runs a root daemon largely to fetch user
//! *avatars*, which a list-based greeter has no use for.
//!
//! `/etc/passwd` is parsed rather than shelled out to `getent`. `getent` would
//! additionally see NIS/LDAP users, but it costs a fork+exec on the boot path,
//! and on the machines this ships to the file is the source of truth. The
//! `--user` flag exists for installations where it is not.

/// One offerable account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserEntry {
    /// The login name. This is both the identity greetd is given and the label
    /// shown in the list: a screen-reader user should hear the name they would
    /// type, not a display name that differs from it.
    pub name: String,
    pub uid: u32,
    /// GECOS field's first comma-separated part ("full name"), if any. Kept for
    /// a future "Nico Verrijdt (nico)" rendering; not used for identity.
    pub full_name: Option<String>,
    pub shell: String,
}

/// Shells that mean "this account cannot be logged into interactively".
const NON_LOGIN_SHELLS: &[&str] = &["nologin", "false", "sync", "halt", "shutdown"];

/// `nobody`. Excluded regardless of where the UID range happens to land.
const NOBODY_UID: u32 = 65534;

/// Read `UID_MIN` / `UID_MAX` out of `/etc/login.defs`.
///
/// Defaults match shadow-utils' own (1000 / 60000) for a file that is missing
/// or silent. This is not a nicety: on a NixOS host `/etc/passwd` carries 32
/// `nixbld` build users at UID 30001-30032, all of them above 1000, and
/// `UID_MAX 29999` is what keeps them out of the login screen.
pub fn uid_range(login_defs: &str) -> (u32, u32) {
    let mut min = 1000_u32;
    let mut max = 60000_u32;
    for line in login_defs.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        match key {
            "UID_MIN" => {
                if let Ok(v) = value.parse() {
                    min = v;
                }
            }
            "UID_MAX" => {
                if let Ok(v) = value.parse() {
                    max = v;
                }
            }
            _ => {}
        }
    }
    (min, max)
}

/// Parse `/etc/passwd` contents into the accounts worth offering.
///
/// Pure, so the filtering rules can be tested against a real `/etc/passwd`
/// without touching the filesystem. Sorted by name so the list is stable.
pub fn parse_passwd(contents: &str, uid_min: u32, uid_max: u32) -> Vec<UserEntry> {
    let mut out: Vec<UserEntry> = contents
        .lines()
        .filter_map(|line| parse_line(line, uid_min, uid_max))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

fn parse_line(line: &str, uid_min: u32, uid_max: u32) -> Option<UserEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // name:passwd:uid:gid:gecos:home:shell
    let f: Vec<&str> = line.split(':').collect();
    if f.len() < 7 {
        return None;
    }
    let name = f[0];
    if name.is_empty() {
        return None;
    }
    let uid: u32 = f[2].parse().ok()?;
    if uid < uid_min || uid > uid_max || uid == NOBODY_UID {
        return None;
    }
    let shell = f[6];
    if is_non_login_shell(shell) {
        return None;
    }
    let full_name = f[4]
        .split(',')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != name)
        .map(str::to_owned);

    Some(UserEntry {
        name: name.to_owned(),
        uid,
        full_name,
        shell: shell.to_owned(),
    })
}

/// A shell's *basename* decides, so `/run/current-system/sw/bin/nologin` and
/// `/usr/sbin/nologin` are both caught.
fn is_non_login_shell(shell: &str) -> bool {
    if shell.is_empty() {
        return true;
    }
    let base = shell.rsplit('/').next().unwrap_or(shell);
    NON_LOGIN_SHELLS.contains(&base)
}

/// Read the real files. Anything unreadable yields an empty list rather than an
/// error: the caller falls back to a typed-in user name, which is also what
/// happens on a host whose accounts are not in `/etc/passwd` at all.
pub fn enumerate() -> Vec<UserEntry> {
    let login_defs = std::fs::read_to_string("/etc/login.defs").unwrap_or_default();
    let (min, max) = uid_range(&login_defs);
    let passwd = match std::fs::read_to_string("/etc/passwd") {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("cannot read /etc/passwd: {e}");
            return Vec::new();
        }
    };
    parse_passwd(&passwd, min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of this machine's real `/etc/passwd`: a system account,
    /// the one human, `nobody`, and three of the 32 `nixbld` users that make
    /// the UID_MAX rule matter.
    const PASSWD: &str = "\
root:x:0:0:System administrator:/root:/run/current-system/sw/bin/fish
greeter:x:989:985::/var/empty:/run/current-system/sw/bin/nologin
cosmic-greeter:x:990:986:COSMIC login greeter user:/var/lib/cosmic-greeter:/run/current-system/sw/bin/nologin
nico:x:1000:100:nico:/home/nico:/run/current-system/sw/bin/fish
nobody:x:65534:65534:Unprivileged account:/var/empty:/run/current-system/sw/bin/nologin
nixbld1:x:30001:30000:Nix build user 1:/var/empty:/run/current-system/sw/bin/nologin
nixbld2:x:30002:30000:Nix build user 2:/var/empty:/run/current-system/sw/bin/nologin
nixbld32:x:30032:30000:Nix build user 32:/var/empty:/run/current-system/sw/bin/nologin
";

    /// This machine's real `/etc/login.defs` values.
    const LOGIN_DEFS: &str = "\
# comment
SYS_UID_MIN  400
SYS_UID_MAX  999
UID_MIN   1000
UID_MAX  29999
";

    fn names(v: &[UserEntry]) -> Vec<&str> {
        v.iter().map(|u| u.name.as_str()).collect()
    }

    #[test]
    fn reads_uid_range_from_login_defs() {
        assert_eq!(uid_range(LOGIN_DEFS), (1000, 29999));
    }

    #[test]
    fn uid_range_defaults_when_absent() {
        assert_eq!(uid_range(""), (1000, 60000));
        assert_eq!(uid_range("# nothing useful here\n"), (1000, 60000));
    }

    #[test]
    fn offers_exactly_the_human_account() {
        let (min, max) = uid_range(LOGIN_DEFS);
        assert_eq!(names(&parse_passwd(PASSWD, min, max)), vec!["nico"]);
    }

    /// The two defences against the `nixbld` accounts are independent, and
    /// each is tested alone so neither can silently become the only one.
    /// Here UID_MAX is removed; the shell filter must still catch them.
    #[test]
    fn shell_filter_alone_excludes_nixbld() {
        assert_eq!(names(&parse_passwd(PASSWD, 1000, u32::MAX)), vec!["nico"]);
    }

    /// And here the shells are made interactive; UID_MAX must still catch them.
    #[test]
    fn uid_max_alone_excludes_nixbld() {
        let interactive = PASSWD.replace(
            "/run/current-system/sw/bin/nologin",
            "/run/current-system/sw/bin/fish",
        );
        assert_eq!(names(&parse_passwd(&interactive, 1000, 29999)), vec!["nico"]);
    }

    #[test]
    fn excludes_nobody_even_inside_the_range() {
        // nobody's shell made interactive and the range widened past 65534:
        // the explicit UID check is what is left.
        let line = "nobody:x:65534:65534:Unprivileged account:/var/empty:/bin/sh\n";
        assert!(parse_passwd(line, 1000, u32::MAX).is_empty());
    }

    #[test]
    fn skips_malformed_lines() {
        let s = "\n#comment\nbroken:x:1001\n:x:1002:100::/home/x:/bin/sh\nok:x:1003:100::/home/ok:/bin/sh\n";
        assert_eq!(names(&parse_passwd(s, 1000, 60000)), vec!["ok"]);
    }

    #[test]
    fn non_numeric_uid_is_skipped() {
        let s = "weird:x:notanumber:100::/home/w:/bin/sh\n";
        assert!(parse_passwd(s, 1000, 60000).is_empty());
    }

    #[test]
    fn sorted_and_deduplicated() {
        let s = "\
zoe:x:1002:100::/home/zoe:/bin/sh
amy:x:1001:100::/home/amy:/bin/sh
amy:x:1001:100::/home/amy:/bin/sh
";
        assert_eq!(names(&parse_passwd(s, 1000, 60000)), vec!["amy", "zoe"]);
    }

    #[test]
    fn full_name_comes_from_gecos_but_never_shadows_the_login_name() {
        let s = "\
nico:x:1000:100:nico:/home/nico:/bin/sh
vera:x:1001:100:Vera Verrijdt,,,:/home/vera:/bin/sh
";
        let users = parse_passwd(s, 1000, 60000);
        // GECOS equal to the login name carries no information.
        assert_eq!(users[0].name, "nico");
        assert_eq!(users[0].full_name, None);
        assert_eq!(users[1].full_name.as_deref(), Some("Vera Verrijdt"));
        // Identity is always the login name.
        assert_eq!(users[1].name, "vera");
    }

    #[test]
    fn empty_shell_is_not_a_login_shell() {
        let s = "svc:x:1004:100::/var/empty:\n";
        assert!(parse_passwd(s, 1000, 60000).is_empty());
    }
}
