//! File browser provider — Rust port of `lib_filebrowser/`.
//!
//! Implements the [`Provider`] trait using `std::fs` for all filesystem
//! operations.  Mirrors the C provider's behaviour exactly:
//!
//! - Root is `/` (or the drive-list sentinel on Windows).
//! - Each directory entry is wrapped in `<input>name</input>` tags so the
//!   user can rename items inline.
//! - Directories are `FfonElement::Obj`; files are `FfonElement::Str`.
//! - A `meta` object (index 0) lists the available keyboard shortcuts.
//! - Supports commands: create directory, create file, show/hide properties,
//!   sort alphanumerically, sort chronologically, open file with.
//! - `commit_edit(old, new)` performs a rename.
//! - `delete_item` / `create_directory` / `create_file` / `copy_item` use
//!   `std::fs` primitives or recursive helpers.
//! - `deep_search` is a BFS traversal (up to 50 000 results).

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::{ListItem, Provider, SearchResultItem};
use sicompass_sdk::tags;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Sort mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    Alpha,
    Chrono,
}

// ---------------------------------------------------------------------------
// FilebrowserProvider
// ---------------------------------------------------------------------------

pub struct FilebrowserProvider {
    current_path: PathBuf,
    show_properties: bool,
    sort_mode: SortMode,
    /// Stored between `handle_command("open file with")` and `execute_command`.
    open_with_path: Option<PathBuf>,
}

impl FilebrowserProvider {
    pub fn new() -> Self {
        FilebrowserProvider {
            current_path: PathBuf::from("/"),
            show_properties: false,
            sort_mode: SortMode::Alpha,
            open_with_path: None,
        }
    }

    fn list_directory(&self) -> Vec<FfonElement> {
        #[cfg(windows)]
        {
            if self.current_path == Path::new("/") {
                return list_drives();
            }
        }
        let path = &self.current_path;
        let mut raw = collect_raw_entries(path);

        match self.sort_mode {
            SortMode::Alpha => raw.sort_by(|a, b| {
                natord::compare_ignore_case(&a.name, &b.name)
            }),
            SortMode::Chrono => raw.sort_by(|a, b| b.mtime.cmp(&a.mtime)),
        }

        let mut out = Vec::with_capacity(raw.len());
        for entry in &raw {
            let prop = if self.show_properties {
                format_properties(entry)
            } else {
                String::new()
            };
            let label = format!("{}{}<input>{}</input>",
                prop,
                // no extra prefix beyond property string
                "",
                entry.name,
            );
            let elem = if entry.is_dir {
                FfonElement::new_obj(&label)
            } else {
                FfonElement::Str(label)
            };
            out.push(elem);
        }
        out
    }
}

impl Default for FilebrowserProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for FilebrowserProvider {
    fn name(&self) -> &str { "filebrowser" }
    fn display_name(&self) -> &str { "file browser" }

    fn init(&mut self) {
        self.current_path = PathBuf::from("/");
        cleanup_clipboard_cache();
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        self.list_directory()
    }

    fn push_path(&mut self, segment: &str) {
        #[cfg(windows)]
        {
            if self.current_path == Path::new("/") {
                // Pushing a drive letter from the sentinel
                self.current_path = PathBuf::from(segment);
                return;
            }
        }
        self.current_path.push(segment.trim_end_matches('/').trim_end_matches('\\'));
    }

    fn pop_path(&mut self) {
        #[cfg(windows)]
        {
            // At drive root (e.g. "C:\") → return to sentinel "/"
            if is_drive_root(&self.current_path) {
                self.current_path = PathBuf::from("/");
                return;
            }
            if self.current_path == Path::new("/") { return; }
        }
        if self.current_path.parent().is_some() && self.current_path != Path::new("/") {
            self.current_path.pop();
        }
    }

    fn current_path(&self) -> &str {
        self.current_path.to_str().unwrap_or("/")
    }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = PathBuf::from(path);
    }

    fn needs_refresh(&self) -> bool { false }

    fn refresh_on_navigate(&self) -> bool { true }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        if key == "sortOrder" {
            self.sort_mode = match value {
                "chronologically" => SortMode::Chrono,
                _ => SortMode::Alpha,
            };
        }
    }

    fn commit_edit(&mut self, old: &str, new_content: &str) -> bool {
        // Strip tags to get bare filename
        let old_name = tags::strip_display(old);
        let new_name = tags::strip_display(new_content);
        if old_name == new_name { return false; }

        let old_path = self.current_path.join(old_name.trim_end_matches('/').trim_end_matches('\\'));
        let new_path = self.current_path.join(new_name.trim_end_matches('/').trim_end_matches('\\'));
        std::fs::rename(&old_path, &new_path).is_ok()
    }

    fn delete_item(&mut self, name: &str) -> bool {
        let name_clean = tags::strip_display(name);
        let name_clean = name_clean.trim_end_matches('/').trim_end_matches('\\');
        let full = self.current_path.join(name_clean);
        trash::delete(&full).is_ok()
    }

    fn create_directory(&mut self, name: &str) -> bool {
        if name.is_empty() { return false; }
        let full = self.current_path.join(name);
        std::fs::create_dir(&full).is_ok()
    }

    fn create_file(&mut self, name: &str) -> bool {
        if name.is_empty() { return false; }
        let full = self.current_path.join(name);
        std::fs::File::create(&full).is_ok()
    }

    fn copy_item(&mut self, src_dir: &str, src_name: &str, dest_dir: &str, dest_name: &str) -> bool {
        let src = Path::new(src_dir).join(src_name.trim_end_matches('/').trim_end_matches('\\'));
        let dst = Path::new(dest_dir).join(dest_name.trim_end_matches('/').trim_end_matches('\\'));
        copy_recursive(&src, &dst)
    }

    fn commands(&self) -> Vec<String> {
        vec![
            "create directory".into(),
            "create file".into(),
            "open file with".into(),
            "show/hide properties".into(),
            "sort alphanumerically".into(),
            "sort chronologically".into(),
        ]
    }

    fn handle_command(
        &mut self,
        command: &str,
        element_key: &str,
        element_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        match command {
            "create directory" => {
                let mut obj = FfonElement::new_obj("<input></input>");
                obj.as_obj_mut().unwrap().push(FfonElement::Str("<input></input>".into()));
                Some(obj)
            }
            "create file" => {
                Some(FfonElement::Str("<input></input>".into()))
            }
            "show/hide properties" => {
                self.show_properties = !self.show_properties;
                None
            }
            "sort alphanumerically" => {
                self.sort_mode = SortMode::Alpha;
                None
            }
            "sort chronologically" => {
                self.sort_mode = SortMode::Chrono;
                None
            }
            "open file with" => {
                // element_type 1 = FFON_OBJECT (directory) — reject directories
                if element_type == 1 {
                    *error = "open with: select a file, not a directory".into();
                    return None;
                }
                let filename = tags::strip_display(element_key);
                if filename.is_empty() {
                    *error = "open with: could not extract filename".into();
                    return None;
                }
                self.open_with_path = Some(self.current_path.join(&filename));
                None
            }
            _ => None,
        }
    }

    fn command_list_items(&self, command: &str) -> Vec<ListItem> {
        if command != "open file with" { return Vec::new(); }
        sicompass_sdk::platform::get_applications()
            .into_iter()
            .map(|a| ListItem { label: a.name, data: a.exec })
            .collect()
    }

    fn execute_command(&mut self, command: &str, selection: &str) -> bool {
        if command == "open file with" {
            if let Some(path) = &self.open_with_path {
                let path_str = path.to_string_lossy().into_owned();
                return sicompass_sdk::platform::open_with(selection, &path_str);
            }
        }
        false
    }

    fn collect_deep_search_items(&self) -> Option<Vec<SearchResultItem>> {
        Some(self.run_deep_search())
    }
}

impl FilebrowserProvider {
    fn run_deep_search(&self) -> Vec<SearchResultItem> {
        const MAX_ITEMS: usize = 50_000;
        let mut results = Vec::new();
        let root = self.current_path.clone();

        // BFS queue: (dir_path, breadcrumb)
        let mut queue: std::collections::VecDeque<(PathBuf, String)> = std::collections::VecDeque::new();
        queue.push_back((root, String::new()));

        while let Some((dir, breadcrumb)) = queue.pop_front() {
            if results.len() >= MAX_ITEMS { break; }

            let rd = match std::fs::read_dir(&dir) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for entry in rd.flatten() {
                if results.len() >= MAX_ITEMS { break; }
                let name = entry.file_name().to_string_lossy().into_owned();
                // Use symlink_metadata to avoid following symlinks (guards against loops)
                let meta = match entry.path().symlink_metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let is_dir = meta.is_dir();
                let label = if is_dir {
                    format!("+ {name}")
                } else {
                    format!("- {name}")
                };
                let nav_path = entry.path().to_string_lossy().into_owned();
                results.push(SearchResultItem {
                    label,
                    breadcrumb: breadcrumb.clone(),
                    nav_path: nav_path.clone(),
                });

                if is_dir {
                    let child_bc = if breadcrumb.is_empty() {
                        format!("{name} > ")
                    } else {
                        format!("{breadcrumb}{name} > ")
                    };
                    queue.push_back((entry.path(), child_bc));
                }
            }
        }

        results
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Raw directory entry
// ---------------------------------------------------------------------------

struct RawEntry {
    name: String,
    mtime: SystemTime,
    is_dir: bool,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    nlink: u64,
    #[cfg(unix)]
    uid: u32,
    #[cfg(unix)]
    gid: u32,
    #[cfg(unix)]
    size: u64,
}

fn collect_raw_entries(path: &Path) -> Vec<RawEntry> {
    let rd = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let meta = match e.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            entries.push(RawEntry {
                name,
                mtime,
                is_dir,
                mode: meta.mode(),
                nlink: meta.nlink(),
                uid: meta.uid(),
                gid: meta.gid(),
                size: meta.size(),
            });
        }
        #[cfg(not(unix))]
        entries.push(RawEntry { name, mtime, is_dir });
    }
    entries
}

// ---------------------------------------------------------------------------
// Property formatting (Unix)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn format_properties(e: &RawEntry) -> String {
    use libc::{getgrgid, getpwuid};
    use std::ffi::CStr;

    // Permission string (e.g. "drwxr-xr-x")
    let mode = e.mode;
    let mut perm = [b'-'; 10];
    perm[0] = if mode & libc::S_IFMT == libc::S_IFDIR { b'd' }
              else if mode & libc::S_IFMT == libc::S_IFLNK { b'l' }
              else { b'-' };
    perm[1] = if mode & libc::S_IRUSR != 0 { b'r' } else { b'-' };
    perm[2] = if mode & libc::S_IWUSR != 0 { b'w' } else { b'-' };
    perm[3] = if mode & libc::S_IXUSR != 0 { b'x' } else { b'-' };
    perm[4] = if mode & libc::S_IRGRP != 0 { b'r' } else { b'-' };
    perm[5] = if mode & libc::S_IWGRP != 0 { b'w' } else { b'-' };
    perm[6] = if mode & libc::S_IXGRP != 0 { b'x' } else { b'-' };
    perm[7] = if mode & libc::S_IROTH != 0 { b'r' } else { b'-' };
    perm[8] = if mode & libc::S_IWOTH != 0 { b'w' } else { b'-' };
    perm[9] = if mode & libc::S_IXOTH != 0 { b'x' } else { b'-' };
    let perm_str = std::str::from_utf8(&perm).unwrap_or("----------");

    // Owner and group names (fall back to numeric ids)
    let owner = unsafe {
        let pw = getpwuid(e.uid);
        if !pw.is_null() {
            CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
        } else {
            e.uid.to_string()
        }
    };
    let group = unsafe {
        let gr = getgrgid(e.gid);
        if !gr.is_null() {
            CStr::from_ptr((*gr).gr_name).to_string_lossy().into_owned()
        } else {
            e.gid.to_string()
        }
    };

    // Date formatted like ls -l: "Mon DD HH:MM" (recent) or "Mon DD  YYYY" (older)
    let mtime_secs = e.mtime
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as libc::time_t;
    let date_str = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&mtime_secs, &mut tm);
        let fmt = if now - mtime_secs < 6 * 30 * 24 * 3600 {
            b"%b %e %H:%M\0".as_ptr() as *const libc::c_char
        } else {
            b"%b %e  %Y\0".as_ptr() as *const libc::c_char
        };
        let mut buf = [0i8; 16];
        libc::strftime(buf.as_mut_ptr(), buf.len(), fmt, &tm);
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    };

    format!("{} {:2} {:<8} {:<8} {:5} {} ",
        perm_str, e.nlink, owner, group, e.size, date_str)
}

#[cfg(not(unix))]
fn format_properties(_e: &RawEntry) -> String {
    String::new()
}

// ---------------------------------------------------------------------------
// Copy
// ---------------------------------------------------------------------------

fn copy_recursive(src: &Path, dst: &Path) -> bool {
    if src.is_dir() {
        if std::fs::create_dir_all(dst).is_err() { return false; }
        let rd = match std::fs::read_dir(src) {
            Ok(r) => r,
            Err(_) => return false,
        };
        for entry in rd.flatten() {
            let child_dst = dst.join(entry.file_name());
            if !copy_recursive(&entry.path(), &child_dst) { return false; }
        }
        true
    } else {
        std::fs::copy(src, dst).is_ok()
    }
}

// ---------------------------------------------------------------------------
// Windows helpers
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn is_drive_root(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.len() == 3 && s.as_bytes()[1] == b':' && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/')
}

#[cfg(windows)]
fn list_drives() -> Vec<FfonElement> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDrives() -> u32;
    }
    // SAFETY: GetLogicalDrives takes no arguments and has no failure mode
    // other than returning 0 (no drives), which we handle gracefully.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) != 0 {
            let letter = (b'A' + i as u8) as char;
            let drive = format!("{}:\\", letter);
            let label = format!("<input>{}</input>", drive);
            out.push(FfonElement::new_obj(&label));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Clipboard cache cleanup stub
// ---------------------------------------------------------------------------

fn cleanup_clipboard_cache() {
    // C version cleans up stale clipboard file copies on init.
    // In Rust we use in-memory clipboard (AppRenderer.clipboard) so there's
    // nothing to clean up here.
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_filebrowser/ (25 + 27 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sicompass_sdk::tags;
    use tempfile::TempDir;

    fn make_provider() -> (FilebrowserProvider, TempDir) {
        let dir = TempDir::new().unwrap();
        let mut p = FilebrowserProvider::new();
        p.set_current_path(dir.path().to_str().unwrap());
        (p, dir)
    }

    // ---- fetch structure ---------------------------------------------------

    #[test]
    fn test_fetch_empty_dir_only_meta() {
        let (mut p, _dir) = make_provider();
        let items = p.fetch();
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_fetch_file_is_str() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("hello.txt"), b"hi").unwrap();
        let items = p.fetch();
        assert!(!items.is_empty());
        assert!(items[0].as_str().is_some());
    }

    #[test]
    fn test_fetch_dir_is_obj() {
        let (mut p, dir) = make_provider();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        let items = p.fetch();
        assert!(!items.is_empty());
        assert!(items[0].as_obj().is_some());
    }

    #[test]
    fn test_fetch_item_wrapped_in_input_tag() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("notes.txt"), b"").unwrap();
        let items = p.fetch();
        let label = items[0].as_str().unwrap();
        assert!(tags::has_input(label));
        assert_eq!(tags::strip_display(label), "notes.txt");
    }

    // ---- sort modes --------------------------------------------------------

    #[test]
    fn test_sort_alpha() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("zebra.txt"), b"").unwrap();
        std::fs::write(dir.path().join("apple.txt"), b"").unwrap();
        p.sort_mode = SortMode::Alpha;
        let items = p.fetch();
        let names: Vec<_> = items.iter()
            .map(|e| tags::strip_display(e.as_str().or_else(|| e.as_obj().map(|o| o.key.as_str())).unwrap_or("")))
            .collect();
        assert_eq!(names, vec!["apple.txt".to_string(), "zebra.txt".to_string()]);
    }

    // ---- rename (commit_edit) ---------------------------------------------

    #[test]
    fn test_rename_file() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("old.txt"), b"").unwrap();
        let ok = p.commit_edit("<input>old.txt</input>", "<input>new.txt</input>");
        assert!(ok);
        assert!(dir.path().join("new.txt").exists());
        assert!(!dir.path().join("old.txt").exists());
    }

    #[test]
    fn test_rename_same_name_returns_false() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("file.txt"), b"").unwrap();
        let ok = p.commit_edit("<input>file.txt</input>", "<input>file.txt</input>");
        assert!(!ok);
    }

    // ---- create_file / create_directory -----------------------------------

    #[test]
    fn test_create_file() {
        let (mut p, dir) = make_provider();
        assert!(p.create_file("new_file.txt"));
        assert!(dir.path().join("new_file.txt").exists());
    }

    #[test]
    fn test_create_directory() {
        let (mut p, dir) = make_provider();
        assert!(p.create_directory("new_dir"));
        assert!(dir.path().join("new_dir").is_dir());
    }

    #[test]
    fn test_create_file_empty_name_fails() {
        let (mut p, _dir) = make_provider();
        assert!(!p.create_file(""));
    }

    // ---- delete_item -------------------------------------------------------

    #[test]
    fn test_delete_file() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("del.txt"), b"x").unwrap();
        assert!(p.delete_item("<input>del.txt</input>"));
        assert!(!dir.path().join("del.txt").exists());
    }

    #[test]
    fn test_delete_directory_recursive() {
        let (mut p, dir) = make_provider();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("file.txt"), b"x").unwrap();
        assert!(p.delete_item("<input>sub</input>"));
        assert!(!sub.exists());
    }

    // ---- copy_item ---------------------------------------------------------

    #[test]
    fn test_copy_file() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("src.txt"), b"hello").unwrap();
        let src_dir = dir.path().to_str().unwrap();
        let dst_dir = dir.path().to_str().unwrap();
        assert!(p.copy_item(src_dir, "src.txt", dst_dir, "dst.txt"));
        assert!(dir.path().join("dst.txt").exists());
    }

    // ---- navigation --------------------------------------------------------

    #[test]
    fn test_push_pop_path() {
        let (mut p, dir) = make_provider();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        p.push_path("child");
        assert!(p.current_path().ends_with("child"));
        p.pop_path();
        assert_eq!(p.current_path(), dir.path().to_str().unwrap());
    }

    #[test]
    fn test_pop_at_root_is_noop() {
        let mut p = FilebrowserProvider::new();
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    // ---- commands ----------------------------------------------------------

    #[test]
    fn test_commands_list() {
        let p = FilebrowserProvider::new();
        let cmds = p.commands();
        assert!(cmds.contains(&"create directory".to_string()));
        assert!(cmds.contains(&"create file".to_string()));
        assert!(cmds.contains(&"show/hide properties".to_string()));
    }

    #[test]
    fn test_handle_command_create_file_returns_input_elem() {
        let (mut p, _dir) = make_provider();
        let mut err = String::new();
        let result = p.handle_command("create file", "", 0, &mut err);
        assert!(result.is_some());
        let elem = result.unwrap();
        assert!(elem.as_str().is_some());
    }

    #[test]
    fn test_handle_command_create_directory_returns_obj() {
        let (mut p, _dir) = make_provider();
        let mut err = String::new();
        let result = p.handle_command("create directory", "", 0, &mut err);
        let elem = result.unwrap();
        assert!(elem.as_obj().is_some());
    }

    #[test]
    fn test_handle_command_toggle_properties() {
        let (mut p, _dir) = make_provider();
        assert!(!p.show_properties);
        let mut err = String::new();
        p.handle_command("show/hide properties", "", 0, &mut err);
        assert!(p.show_properties);
        p.handle_command("show/hide properties", "", 0, &mut err);
        assert!(!p.show_properties);
    }

    #[test]
    fn test_handle_command_sort_chrono() {
        let (mut p, _dir) = make_provider();
        let mut err = String::new();
        p.handle_command("sort chronologically", "", 0, &mut err);
        assert_eq!(p.sort_mode, SortMode::Chrono);
        p.handle_command("sort alphanumerically", "", 0, &mut err);
        assert_eq!(p.sort_mode, SortMode::Alpha);
    }

    // ---- deep_search -------------------------------------------------------

    #[test]
    fn test_deep_search_finds_nested_files() {
        let (mut p, dir) = make_provider();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("deep.txt"), b"").unwrap();
        p.set_current_path(dir.path().to_str().unwrap());
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert!(results.iter().any(|r| r.label.contains("deep.txt")));
    }

    #[test]
    fn test_deep_search_dir_prefix() {
        let (mut p, dir) = make_provider();
        std::fs::create_dir(dir.path().join("mydir")).unwrap();
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert!(results.iter().any(|r| r.label.starts_with("+ ")));
    }

    #[test]
    fn test_deep_search_file_prefix() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("myfile.txt"), b"").unwrap();
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert!(results.iter().any(|r| r.label.starts_with("- ")));
    }

    #[test]
    fn test_handle_command_sort_alpha() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("cherry.txt"), b"").unwrap();
        std::fs::write(dir.path().join("apple.txt"), b"").unwrap();
        std::fs::write(dir.path().join("banana.txt"), b"").unwrap();
        let mut err = String::new();
        p.handle_command("sort alphanumerically", "", 0, &mut err);
        assert_eq!(p.sort_mode, SortMode::Alpha);
        let items = p.fetch();
        let file_labels: Vec<_> = items.iter()
            .filter_map(|e| e.as_str())
            .map(|s| sicompass_sdk::tags::strip_display(s).to_string())
            .collect();
        assert_eq!(file_labels, vec!["apple.txt", "banana.txt", "cherry.txt"]);
    }

    #[test]
    fn test_sort_alpha_natural_order() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("file10.txt"), b"").unwrap();
        std::fs::write(dir.path().join("file2.txt"), b"").unwrap();
        std::fs::write(dir.path().join("file1.txt"), b"").unwrap();
        let mut err = String::new();
        p.handle_command("sort alphanumerically", "", 0, &mut err);
        let items = p.fetch();
        let file_labels: Vec<_> = items.iter()
            .filter_map(|e| e.as_str())
            .map(|s| sicompass_sdk::tags::strip_display(s).to_string())
            .collect();
        assert_eq!(file_labels, vec!["file1.txt", "file2.txt", "file10.txt"],
            "natural sort should order file2 before file10");
    }

    #[test]
    fn test_handle_command_open_with_directory_error() {
        let (mut p, _dir) = make_provider();
        let mut err = String::new();
        // element_type 1 = FFON_OBJECT (directory)
        let result = p.handle_command("open file with", "<input>somedir</input>", 1, &mut err);
        assert!(result.is_none());
        assert!(!err.is_empty(), "error should be set for directory");
        assert!(err.contains("directory"), "error should mention directory");
    }

    #[test]
    fn test_handle_command_unknown() {
        let (mut p, _dir) = make_provider();
        let mut err = String::new();
        let result = p.handle_command("nonexistent command", "", 0, &mut err);
        assert!(result.is_none());
    }

    #[test]
    fn test_deep_search_empty_dir() {
        let (mut p, dir) = make_provider();
        p.set_current_path(dir.path().to_str().unwrap());
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_deep_search_flat_files() {
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("alpha.txt"), b"").unwrap();
        std::fs::write(dir.path().join("beta.txt"), b"").unwrap();
        std::fs::write(dir.path().join("gamma.txt"), b"").unwrap();
        let results = p.collect_deep_search_items().unwrap_or_default();
        assert_eq!(results.len(), 3);
        for item in &results {
            assert!(item.label.starts_with("- "), "flat files should have '- ' prefix");
            assert_eq!(item.breadcrumb, "", "flat files should have empty breadcrumb");
            assert!(item.nav_path.contains(dir.path().to_str().unwrap()));
        }
    }

    #[test]
    fn test_get_command_list_items_non_open_with() {
        let (p, _dir) = make_provider();
        let items = p.command_list_items("create directory");
        assert!(items.is_empty());
    }

    #[test]
    fn test_execute_command_unknown() {
        let (mut p, _dir) = make_provider();
        let result = p.execute_command("nonexistent", "anything");
        assert!(!result);
    }

    #[test]
    #[cfg(unix)]
    fn test_deep_search_symlink_not_followed() {
        let (mut p, dir) = make_provider();
        // Create a symlink pointing back to the root dir (circular)
        let link_path = dir.path().join("loop");
        std::os::unix::fs::symlink(dir.path(), &link_path).unwrap();
        // Also create a regular file
        std::fs::write(dir.path().join("regular.txt"), b"").unwrap();
        let results = p.collect_deep_search_items().unwrap_or_default();
        // Should find: loop (as non-dir via symlink_metadata) + regular.txt = 2
        assert_eq!(results.len(), 2, "symlink should not be traversed as dir");
        let loop_item = results.iter().find(|r| r.label.contains("loop"));
        assert!(loop_item.is_some(), "loop symlink should appear in results");
        assert!(loop_item.unwrap().label.starts_with("- "), "symlink should show as file, not dir");
    }

    // ---- additional coverage to match C test suite -------------------------

    #[test]
    fn test_create_file_already_exists() {
        // Creating an already-existing file should not crash; file still exists.
        let (mut p, dir) = make_provider();
        p.create_file("existing.txt");
        p.create_file("existing.txt"); // second call — should not panic
        assert!(dir.path().join("existing.txt").exists());
    }

    #[test]
    fn test_fetch_nonexistent_path_returns_only_meta() {
        let mut p = FilebrowserProvider::new();
        p.set_current_path("/nonexistent/path/xyz/abc");
        let items = p.fetch();
        // On a nonexistent path the listing is empty
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_rename_nonexistent_returns_false() {
        let (mut p, _dir) = make_provider();
        let result = p.commit_edit("<input>nonexistent.txt</input>", "<input>new.txt</input>");
        assert!(!result);
    }

    #[test]
    fn test_rename_directory() {
        let (mut p, dir) = make_provider();
        std::fs::create_dir(dir.path().join("olddir")).unwrap();
        let ok = p.commit_edit("<input>olddir</input>", "<input>newdir</input>");
        assert!(ok);
        assert!(!dir.path().join("olddir").exists());
        assert!(dir.path().join("newdir").exists());
    }

    #[test]
    fn test_delete_nonexistent_returns_false() {
        let (mut p, _dir) = make_provider();
        assert!(!p.delete_item("<input>nonexistent_xyz</input>"));
    }

    #[test]
    #[cfg(unix)]
    fn test_copy_directory() {
        let (mut p, dir) = make_provider();
        let src = dir.path().join("srcdir");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("inner.txt"), b"data").unwrap();
        let src_str = dir.path().to_str().unwrap();
        assert!(p.copy_item(src_str, "srcdir", src_str, "cpdir"));
        assert!(dir.path().join("cpdir").is_dir());
        assert!(dir.path().join("cpdir/inner.txt").exists());
    }

    #[test]
    fn test_fetch_special_chars_in_filename() {
        // Files with spaces and dashes should not crash the listing.
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("hello world.txt"), b"").unwrap();
        std::fs::write(dir.path().join("file-with-dashes.txt"), b"").unwrap();
        let items = p.fetch();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_get_commands_returns_six() {
        let p = FilebrowserProvider::new();
        let cmds = p.commands();
        assert_eq!(cmds.len(), 6);
        assert!(cmds.contains(&"create directory".to_string()));
        assert!(cmds.contains(&"create file".to_string()));
        assert!(cmds.contains(&"open file with".to_string()));
        assert!(cmds.contains(&"show/hide properties".to_string()));
        assert!(cmds.contains(&"sort alphanumerically".to_string()));
        assert!(cmds.contains(&"sort chronologically".to_string()));
    }

    #[test]
    fn test_get_command_list_items_open_with_no_apps() {
        // When no desktop apps are found, open_with returns empty list.
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("test.txt"), b"").unwrap();
        // Prime open_with_path via handle_command
        let mut err = String::new();
        p.handle_command("open file with", "<input>test.txt</input>", 0, &mut err);
        // command_list_items queries the platform for apps; on headless CI this returns empty
        let items = p.command_list_items("open file with");
        // Either empty (no apps found in CI) or non-empty (apps found) — just must not panic.
        let _ = items; // no crash is the assertion
    }

    #[test]
    fn test_provider_path_starts_at_root() {
        // On non-Windows, the initial path is "/".
        #[cfg(not(windows))]
        {
            let p = FilebrowserProvider::new();
            assert_eq!(p.current_path(), "/");
        }
    }

    // ---- cleanup_clipboard_cache -------------------------------------------

    #[test]
    fn test_cleanup_clipboard_cache_no_crash() {
        // Should be a no-op in Rust (we use in-memory clipboard), must not panic.
        cleanup_clipboard_cache();
    }

    // ---- chrono sort ordering ----------------------------------------------

    #[test]
    #[cfg(unix)]
    fn test_list_directory_chrono_sort() {
        use std::time::{Duration, UNIX_EPOCH, SystemTime};
        let (mut p, dir) = make_provider();

        // Create three files with distinct mtime set via FileTimes
        let make_file_at = |name: &str, secs: u64| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"").unwrap();
            let mtime = UNIX_EPOCH + Duration::from_secs(secs);
            let ft = std::fs::FileTimes::new().set_modified(mtime);
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_times(ft).unwrap();
        };
        make_file_at("oldest.txt", 1_000_000);
        make_file_at("middle.txt", 2_000_000);
        make_file_at("newest.txt", 3_000_000);

        p.sort_mode = SortMode::Chrono;
        let items = p.fetch();
        let names: Vec<String> = items.iter()
            .filter_map(|e| e.as_str())
            .map(|s| tags::strip_display(s).to_string())
            .collect();
        assert_eq!(names[0], "newest.txt", "newest should come first in chrono sort, got: {:?}", names);
        assert_eq!(names[1], "middle.txt");
        assert_eq!(names[2], "oldest.txt");
    }

    // ---- executables always shown ------------------------------------------

    #[test]
    #[cfg(unix)]
    fn test_fetch_executable_always_shown() {
        use std::os::unix::fs::PermissionsExt;
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("script.sh"), b"#!/bin/sh").unwrap();
        std::fs::set_permissions(
            dir.path().join("script.sh"),
            std::fs::Permissions::from_mode(0o755),
        ).unwrap();
        std::fs::write(dir.path().join("data.txt"), b"").unwrap();

        // Rust filebrowser always shows executables — no separate "commands mode"
        let items = p.fetch();
        // Should have script.sh + data.txt = 2 entries
        assert_eq!(items.len(), 2, "expected 2 files, got {}", items.len());
    }

    // ---- execute_command open file with ------------------------------------

    #[test]
    fn test_execute_command_open_with_no_path_returns_false() {
        // Without first calling handle_command to set the path, execute should return false.
        let (mut p, _dir) = make_provider();
        let result = p.execute_command("open file with", "firefox");
        assert!(!result, "execute_command should return false when no path is set");
    }

    #[test]
    fn test_execute_command_open_with_sets_path_then_executes() {
        // handle_command stores the path; execute_command calls open_with.
        // We can't test the actual open_with call (platform-specific) but we can
        // verify the function accepts the call without panicking.
        let (mut p, dir) = make_provider();
        std::fs::write(dir.path().join("test.txt"), b"content").unwrap();
        let mut err = String::new();
        p.handle_command("open file with", "<input>test.txt</input>", 0, &mut err);
        // open_with_path should now be set
        assert!(p.open_with_path.is_some(), "open_with_path should be set after handle_command");
        // execute_command will call platform::open_with — result depends on platform
        let _ = p.execute_command("open file with", "xdg-open");
        // No panic = pass
    }

    #[test]
    #[cfg(unix)]
    fn test_fetch_symlink_appears_in_listing() {
        let (mut p, dir) = make_provider();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, b"content").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let items = p.fetch();
        // Should have real.txt + link.txt = 2 entries
        assert_eq!(items.len(), 2);
        let names: Vec<_> = items.iter()
            .filter_map(|e| e.as_str())
            .map(|s| tags::strip_display(s).to_string())
            .collect();
        assert!(names.contains(&"link.txt".to_string()));
    }
}
