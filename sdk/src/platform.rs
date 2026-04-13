use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Config / home / cache directories
// ---------------------------------------------------------------------------

/// Returns `$XDG_CONFIG_HOME` on Linux, `~/Library/Application Support` on macOS,
/// `%APPDATA%` on Windows. Equivalent to `platformGetConfigHome()`.
pub fn config_home() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg));
            }
        }
        home_dir().map(|h| h.join(".config"))
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library").join("Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        home_dir().map(|h| h.join(".config"))
    }
}

/// Returns the user's home directory.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// Returns the user's cache directory.
pub fn cache_home() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg));
            }
        }
        home_dir().map(|h| h.join(".cache"))
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library").join("Caches"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        home_dir().map(|h| h.join(".cache"))
    }
}

/// Returns the user's XDG state directory.
/// - Linux: `$XDG_STATE_HOME` or `~/.local/state`
/// - macOS: `~/Library/Logs`
/// - Windows: `%LOCALAPPDATA%`
pub fn state_home() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg));
            }
        }
        home_dir().map(|h| h.join(".local").join("state"))
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library").join("Logs"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        home_dir().map(|h| h.join(".local").join("state"))
    }
}

/// Returns `~/.local/state/sicompass/` (or platform equivalent) for log files.
pub fn log_dir() -> Option<PathBuf> {
    state_home().map(|s| s.join("sicompass"))
}

/// Returns the user's Downloads directory.
pub fn downloads_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join("Downloads"))
}

// ---------------------------------------------------------------------------
// Sicompass config paths
// ---------------------------------------------------------------------------

/// Returns `~/.config/sicompass/providers/` (or platform equivalent).
pub fn provider_config_dir() -> Option<PathBuf> {
    config_home().map(|c| c.join("sicompass").join("providers"))
}

/// Returns `~/.config/sicompass/providers/<name>.json`.
pub fn provider_config_path(name: &str) -> Option<PathBuf> {
    provider_config_dir().map(|d| d.join(format!("{name}.json")))
}

/// Returns `~/.config/sicompass/settings.json`.
pub fn main_config_path() -> Option<PathBuf> {
    config_home().map(|c| c.join("sicompass").join("settings.json"))
}

/// Returns `~/.config/sicompass/plugins/`.
pub fn plugins_dir() -> Option<PathBuf> {
    config_home().map(|c| c.join("sicompass").join("plugins"))
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Create all components of path (`mkdir -p`). Silently ignores existing dirs.
pub fn make_dirs(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
}

/// Returns `"/"` on Unix, `"\\"` on Windows.
pub fn path_separator() -> &'static str {
    #[cfg(target_os = "windows")]
    { "\\" }
    #[cfg(not(target_os = "windows"))]
    { "/" }
}

pub fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

// ---------------------------------------------------------------------------
// Open with default application
// ---------------------------------------------------------------------------

/// Open a file or URL with the system default application.
pub fn open_with_default(path: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(path).spawn().is_ok()
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn().is_ok()
    }
    #[cfg(target_os = "windows")]
    {
        // `cmd /c start` interprets `&` in URLs as a command separator.
        // `rundll32 url.dll,FileProtocolHandler` passes the argument as a
        // single string to the shell's URL handler, preserving `&` intact.
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", path])
            .spawn()
            .is_ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Open a file with a specific application.
pub fn open_with(program: &str, file_path: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        Command::new(program).arg(file_path).spawn().is_ok()
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").args(["-a", program, file_path]).spawn().is_ok()
    }
    #[cfg(target_os = "windows")]
    {
        Command::new(program).arg(file_path).spawn().is_ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

// ---------------------------------------------------------------------------
// Installed applications
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Application {
    pub name: String,
    pub exec: String,
}

/// List installed applications.
/// - Linux: parses `.desktop` files from XDG application directories.
/// - macOS: scans `/Applications` and `~/Applications` for `.app` bundles.
/// - Windows: enumerates `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths`.
pub fn get_applications() -> Vec<Application> {
    #[cfg(target_os = "linux")]
    {
        get_applications_linux()
    }
    #[cfg(target_os = "macos")]
    {
        get_applications_macos()
    }
    #[cfg(target_os = "windows")]
    {
        get_applications_windows()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn get_applications_linux() -> Vec<Application> {
    let dirs = [
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        home_dir()
            .map(|h| h.join(".local/share/applications"))
            .unwrap_or_default(),
    ];
    get_applications_from_dirs(&dirs)
}

/// Parse a single `.desktop` file's text content.
/// Returns `Some((name, exec))` if the entry is a valid, visible application,
/// or `None` if it should be excluded.
#[cfg(target_os = "linux")]
fn parse_desktop_file(content: &str) -> Option<(String, String)> {
    let mut name = String::new();
    let mut exec_raw = String::new();
    let mut no_display = false;
    let mut hidden = false;
    let mut type_app = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        // Section headers
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }

        if line.starts_with("Name=") && name.is_empty() {
            name = line[5..].to_owned();
        } else if line.starts_with("Exec=") && exec_raw.is_empty() {
            exec_raw = line[5..].to_owned();
        } else if line.starts_with("NoDisplay=") {
            no_display = &line[10..] == "true";
        } else if line.starts_with("Hidden=") {
            hidden = &line[7..] == "true";
        } else if line.starts_with("Type=") {
            type_app = &line[5..] == "Application";
        }
    }

    if !type_app || no_display || hidden || name.is_empty() || exec_raw.is_empty() {
        return None;
    }

    // Strip field codes character-by-character, matching the C version.
    // Codes: %f %F %u %U %d %D %n %N %i %c %k %v %m — skip the code + optional trailing space.
    let bytes = exec_raw.as_bytes();
    let mut clean: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            let code = bytes[i + 1];
            if matches!(
                code,
                b'f' | b'F' | b'u' | b'U' | b'd' | b'D'
                    | b'n' | b'N' | b'i' | b'c' | b'k' | b'v' | b'm'
            ) {
                i += 2;
                if i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
                continue;
            }
        }
        clean.push(bytes[i]);
        i += 1;
    }

    let exec = String::from_utf8_lossy(&clean).trim_end().to_string();
    if exec.is_empty() {
        return None;
    }

    Some((name, exec))
}

#[cfg(target_os = "linux")]
fn get_applications_from_dirs(dirs: &[PathBuf]) -> Vec<Application> {
    let mut apps = Vec::new();
    let mut seen_execs: HashSet<String> = HashSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            if let Some((name, exec)) = parse_desktop_file(&content) {
                if seen_execs.contains(&exec) {
                    continue;
                }
                seen_execs.insert(exec.clone());
                apps.push(Application { name, exec });
            }
        }
    }
    apps
}

// ---------------------------------------------------------------------------
// macOS: scan .app bundles
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn get_applications_macos() -> Vec<Application> {
    let mut apps = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    let dirs: Vec<PathBuf> = {
        let mut v = vec![PathBuf::from("/Applications")];
        if let Some(h) = home_dir() {
            v.push(h.join("Applications"));
        }
        v
    };

    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            if name_str.ends_with(".app") {
                let display = name_str[..name_str.len() - 4].to_string();
                if seen_names.insert(display.clone()) {
                    apps.push(Application { name: display.clone(), exec: display });
                }
            } else {
                // One level deep into subdirectories (e.g. /Applications/Utilities/)
                let sub_path = entry.path();
                if !sub_path.is_dir() {
                    continue;
                }
                let Ok(sub_entries) = std::fs::read_dir(&sub_path) else { continue };
                for sub_entry in sub_entries.flatten() {
                    let sub_name = sub_entry.file_name();
                    let sub_str = sub_name.to_string_lossy();
                    if sub_str.starts_with('.') || !sub_str.ends_with(".app") {
                        continue;
                    }
                    let display = sub_str[..sub_str.len() - 4].to_string();
                    if seen_names.insert(display.clone()) {
                        apps.push(Application { name: display.clone(), exec: display });
                    }
                }
            }
        }
    }
    apps
}

// ---------------------------------------------------------------------------
// Windows: enumerate App Paths registry key
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn get_applications_windows() -> Vec<Application> {
    use winreg::enums::*;
    use winreg::RegKey;

    let mut apps = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(key) = hklm.open_subkey(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths",
    ) else {
        return apps;
    };

    for sub_key_name in key.enum_keys().flatten() {
        // Display name: strip .exe extension (case-insensitive)
        let display = if sub_key_name.to_ascii_lowercase().ends_with(".exe") {
            sub_key_name[..sub_key_name.len() - 4].to_string()
        } else {
            sub_key_name.clone()
        };

        if seen_names.insert(display.to_ascii_lowercase()) {
            apps.push(Application { name: display, exec: sub_key_name });
        }
    }
    apps
}

// ---------------------------------------------------------------------------
// bun executable discovery (Windows PATH injection)
// ---------------------------------------------------------------------------

/// On Windows, `bun` is installed to `%USERPROFILE%\.bun\bin\bun.exe`, but a
/// process started before that install (or from a shell whose PATH was cached)
/// won't see it. This prepends the bun bin directory to the current process'
/// PATH so `Command::new("bun")` works in child processes.
///
/// Idempotent (runs once per process). No-op on non-Windows.
/// Mirrors `getBunExecutable` in lib/lib_provider/src/provider.c:303-333.
pub fn ensure_bun_on_path() {
    #[cfg(target_os = "windows")]
    {
        use std::sync::OnceLock;
        static DONE: OnceLock<()> = OnceLock::new();
        DONE.get_or_init(|| {
            let Some(home) = home_dir() else { return; };
            let bun_dir = home.join(".bun").join("bin");
            if !bun_dir.join("bun.exe").exists() {
                return;
            }
            let existing = std::env::var_os("PATH").unwrap_or_default();
            let mut parts: Vec<std::ffi::OsString> = vec![bun_dir.into_os_string()];
            if !existing.is_empty() {
                parts.push(existing);
            }
            if let Ok(joined) = std::env::join_paths(parts) {
                std::env::set_var("PATH", joined);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_provider/test_provider_platform.c (10 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_home_dir_exists() {
        let h = home_dir();
        assert!(h.is_some(), "home_dir should return Some");
    }

    #[test]
    fn test_config_home_exists() {
        let c = config_home();
        assert!(c.is_some(), "config_home should return Some");
    }

    #[test]
    fn test_main_config_path_ends_with_settings_json() {
        let p = main_config_path().unwrap();
        assert!(p.to_string_lossy().ends_with("settings.json"));
    }

    #[test]
    fn test_main_config_path_contains_sicompass() {
        let p = main_config_path().unwrap();
        assert!(p.to_string_lossy().contains("sicompass"));
    }

    #[test]
    fn test_provider_config_dir_contains_providers() {
        let p = provider_config_dir().unwrap();
        assert!(p.to_string_lossy().contains("providers"));
    }

    #[test]
    fn test_provider_config_path() {
        let p = provider_config_path("filebrowser").unwrap();
        assert!(p.to_string_lossy().ends_with("filebrowser.json"));
    }

    #[test]
    fn test_plugins_dir_contains_plugins() {
        let p = plugins_dir().unwrap();
        assert!(p.to_string_lossy().contains("plugins"));
    }

    #[test]
    fn test_downloads_dir_ends_with_downloads() {
        let p = downloads_dir().unwrap();
        assert!(p.to_string_lossy().contains("Downloads") || p.to_string_lossy().contains("downloads"));
    }

    #[test]
    fn test_path_separator_not_empty() {
        assert!(!path_separator().is_empty());
    }

    #[test]
    fn test_make_dirs_creates_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        make_dirs(&nested);
        assert!(nested.exists());
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;
    use std::fs;

    fn desktop(body: &str) -> String {
        format!("[Desktop Entry]\n{}", body)
    }

    #[test]
    fn test_basic_valid_entry() {
        let content = desktop("Type=Application\nName=Foo\nExec=foo\n");
        let result = parse_desktop_file(&content);
        assert_eq!(result, Some(("Foo".into(), "foo".into())));
    }

    #[test]
    fn test_missing_type_filtered() {
        let content = desktop("Name=Foo\nExec=foo\n");
        assert_eq!(parse_desktop_file(&content), None);
    }

    #[test]
    fn test_wrong_type_filtered() {
        let content = desktop("Type=Link\nName=Foo\nExec=foo\n");
        assert_eq!(parse_desktop_file(&content), None);
    }

    #[test]
    fn test_hidden_true_filtered() {
        let content = desktop("Type=Application\nName=Foo\nExec=foo\nHidden=true\n");
        assert_eq!(parse_desktop_file(&content), None);
    }

    #[test]
    fn test_no_display_true_filtered() {
        let content = desktop("Type=Application\nName=Foo\nExec=foo\nNoDisplay=true\n");
        assert_eq!(parse_desktop_file(&content), None);
    }

    #[test]
    fn test_no_display_does_not_prevent_type_check() {
        // NoDisplay appears before Type — must still read all fields (no early break)
        let content = desktop("NoDisplay=true\nType=Application\nName=Foo\nExec=foo\n");
        assert_eq!(parse_desktop_file(&content), None);
    }

    #[test]
    fn test_action_section_ignored() {
        let content = "[Desktop Entry]\nType=Application\nName=Real\nExec=real\n\
                       [Desktop Action NewWindow]\nName=Shadow\nExec=shadow\n";
        let result = parse_desktop_file(content);
        assert_eq!(result, Some(("Real".into(), "real".into())));
    }

    #[test]
    fn test_fields_before_desktop_entry_ignored() {
        // Fields appearing before any section header are outside [Desktop Entry]
        let content = "Name=Ghost\nExec=ghost\n[Desktop Entry]\nType=Application\nName=Real\nExec=real\n";
        let result = parse_desktop_file(content);
        assert_eq!(result, Some(("Real".into(), "real".into())));
    }

    #[test]
    fn test_field_codes_stripped() {
        let content = desktop("Type=Application\nName=Foo\nExec=foo %f bar %U\n");
        let result = parse_desktop_file(&content);
        assert_eq!(result, Some(("Foo".into(), "foo bar".into())));
    }

    #[test]
    fn test_field_code_at_end_of_exec() {
        let content = desktop("Type=Application\nName=Foo\nExec=foo %f\n");
        let result = parse_desktop_file(&content);
        assert_eq!(result, Some(("Foo".into(), "foo".into())));
    }

    #[test]
    fn test_trailing_whitespace_trimmed() {
        let content = desktop("Type=Application\nName=Foo\nExec=foo   \n");
        let result = parse_desktop_file(&content);
        assert_eq!(result, Some(("Foo".into(), "foo".into())));
    }

    #[test]
    fn test_percent_percent_preserved() {
        // %% is not a known field code — should be kept as-is
        let content = desktop("Type=Application\nName=Foo\nExec=foo %%bar\n");
        let result = parse_desktop_file(&content);
        assert_eq!(result, Some(("Foo".into(), "foo %%bar".into())));
    }

    #[test]
    fn test_dedup_same_exec() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.desktop");
        let b = tmp.path().join("b.desktop");
        fs::write(&a, "[Desktop Entry]\nType=Application\nName=AppA\nExec=myapp\n").unwrap();
        fs::write(&b, "[Desktop Entry]\nType=Application\nName=AppB\nExec=myapp\n").unwrap();
        let apps = get_applications_from_dirs(&[tmp.path().to_path_buf()]);
        assert_eq!(apps.len(), 1, "duplicate exec should be deduplicated");
    }

    #[test]
    fn test_different_execs_both_included() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.desktop");
        let b = tmp.path().join("b.desktop");
        fs::write(&a, "[Desktop Entry]\nType=Application\nName=AppA\nExec=app-a\n").unwrap();
        fs::write(&b, "[Desktop Entry]\nType=Application\nName=AppB\nExec=app-b\n").unwrap();
        let apps = get_applications_from_dirs(&[tmp.path().to_path_buf()]);
        assert_eq!(apps.len(), 2);
    }
}
