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
        // On Windows use the `open` crate or a raw ShellExecute call.
        // For now, spawn `cmd /c start ""` which works for URLs and files.
        Command::new("cmd").args(["/c", "start", "", path]).spawn().is_ok()
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

/// List installed applications. On Linux, parses `.desktop` files.
pub fn get_applications() -> Vec<Application> {
    #[cfg(target_os = "linux")]
    {
        get_applications_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn get_applications_linux() -> Vec<Application> {
    let mut apps = Vec::new();
    let dirs = [
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        home_dir()
            .map(|h| h.join(".local/share/applications"))
            .unwrap_or_default(),
    ];
    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let mut name = String::new();
                let mut exec = String::new();
                let mut no_display = false;
                for line in content.lines() {
                    if line.starts_with("Name=") && name.is_empty() {
                        name = line[5..].to_owned();
                    } else if line.starts_with("Exec=") && exec.is_empty() {
                        // Strip %f %u %F %U etc.
                        exec = line[5..]
                            .split_whitespace()
                            .filter(|t| !t.starts_with('%'))
                            .collect::<Vec<_>>()
                            .join(" ");
                    } else if line == "NoDisplay=true" {
                        no_display = true;
                        break;
                    }
                }
                if !no_display && !name.is_empty() && !exec.is_empty() {
                    apps.push(Application { name, exec });
                }
            }
        }
    }
    apps
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
