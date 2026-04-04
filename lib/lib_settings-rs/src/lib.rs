use serde_json::{Map, Value};
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::platform;
use sicompass_sdk::provider::Provider;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Setting entry types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RadioEntry {
    pub section: String,
    pub radio_key: String,
    pub config_key: String,
    pub options: Vec<String>,
    pub current_value: String,
}

#[derive(Debug, Clone)]
pub struct TextEntry {
    pub section: String,
    pub label: String,
    pub config_key: String,
    pub current_value: String,
}

#[derive(Debug, Clone)]
pub struct CheckboxEntry {
    pub section: String,
    pub label: String,
    pub config_key: String,
    pub checked: bool,
}

// ---------------------------------------------------------------------------
// Apply callback
// ---------------------------------------------------------------------------

/// Called when a setting changes (on load and on user interaction).
/// `key` is the config key, `value` is the new value as a string
/// (`"true"` / `"false"` for checkboxes).
pub type ApplyFn = Box<dyn Fn(&str, &str) + Send + 'static>;

// ---------------------------------------------------------------------------
// SettingsProvider
// ---------------------------------------------------------------------------

pub struct SettingsProvider {
    current_path: String,
    color_scheme: String,
    sections: Vec<String>,
    priority_section: Option<String>,
    radio_entries: Vec<RadioEntry>,
    text_entries: Vec<TextEntry>,
    checkbox_entries: Vec<CheckboxEntry>,
    apply_fn: Option<ApplyFn>,
    /// Override config path (used in tests to avoid touching real config)
    config_path_override: Option<PathBuf>,
}

impl SettingsProvider {
    pub fn new(apply_fn: impl Fn(&str, &str) + Send + 'static) -> Self {
        SettingsProvider {
            current_path: "/".to_owned(),
            color_scheme: "dark".to_owned(),
            sections: Vec::new(),
            priority_section: None,
            radio_entries: Vec::new(),
            text_entries: Vec::new(),
            checkbox_entries: Vec::new(),
            apply_fn: Some(Box::new(apply_fn)),
            config_path_override: None,
        }
    }

    /// Create without an apply callback (useful for testing fetch output).
    pub fn new_headless() -> Self {
        SettingsProvider {
            current_path: "/".to_owned(),
            color_scheme: "dark".to_owned(),
            sections: Vec::new(),
            priority_section: None,
            radio_entries: Vec::new(),
            text_entries: Vec::new(),
            checkbox_entries: Vec::new(),
            apply_fn: None,
            config_path_override: None,
        }
    }

    /// Override the settings.json path (for tests).
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path_override = Some(path);
        self
    }

    fn config_path(&self) -> Option<PathBuf> {
        self.config_path_override.clone().or_else(|| platform::main_config_path())
    }

    // ---- Registration API (mirrors settingsAdd* functions in C) -----------

    pub fn add_section(&mut self, name: &str) {
        if !self.sections.iter().any(|s| s == name) {
            self.sections.push(name.to_owned());
        }
    }

    pub fn remove_section(&mut self, name: &str) {
        self.sections.retain(|s| s != name);
        self.radio_entries.retain(|e| e.section != name);
        self.text_entries.retain(|e| e.section != name);
        self.checkbox_entries.retain(|e| e.section != name);
    }

    pub fn add_priority_section(&mut self, name: &str) {
        self.priority_section = Some(name.to_owned());
        self.add_section(name);
    }

    pub fn add_radio(
        &mut self,
        section: &str,
        radio_key: &str,
        config_key: &str,
        options: &[&str],
        default_value: &str,
    ) {
        self.radio_entries.push(RadioEntry {
            section: section.to_owned(),
            radio_key: radio_key.to_owned(),
            config_key: config_key.to_owned(),
            options: options.iter().map(|s| s.to_string()).collect(),
            current_value: default_value.to_owned(),
        });
        self.add_section(section);
    }

    pub fn add_text(&mut self, section: &str, label: &str, config_key: &str, default: &str) {
        self.text_entries.push(TextEntry {
            section: section.to_owned(),
            label: label.to_owned(),
            config_key: config_key.to_owned(),
            current_value: default.to_owned(),
        });
        self.add_section(section);
    }

    pub fn add_checkbox(
        &mut self,
        section: &str,
        label: &str,
        config_key: &str,
        default_checked: bool,
    ) {
        self.checkbox_entries.push(CheckboxEntry {
            section: section.to_owned(),
            label: label.to_owned(),
            config_key: config_key.to_owned(),
            checked: default_checked,
        });
        self.add_section(section);
    }

    /// Programmatically set a checkbox state (without firing the apply callback).
    pub fn set_checkbox_state(&mut self, config_key: &str, checked: bool) {
        if let Some(e) = self.checkbox_entries.iter_mut().find(|e| e.config_key == config_key) {
            if e.checked != checked {
                e.checked = checked;
                self.save_config_if_possible();
            }
        }
    }

    // ---- Load / save -------------------------------------------------------

    fn load_config(&mut self, path: &Path) {
        let Ok(data) = std::fs::read_to_string(path) else { return };
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&data) else { return };

        // color scheme
        if let Some(Value::Object(sc)) = root.get("sicompass") {
            if let Some(Value::String(cs)) = sc.get("colorScheme") {
                if cs == "dark" || cs == "light" {
                    self.color_scheme = cs.clone();
                }
            }
        }

        // radio entries
        for e in &mut self.radio_entries {
            if let Some(Value::Object(sec)) = root.get(&e.section) {
                if let Some(Value::String(val)) = sec.get(&e.config_key) {
                    if e.options.iter().any(|o| o == val) {
                        e.current_value = val.clone();
                    }
                }
            }
        }

        // text entries
        for e in &mut self.text_entries {
            if let Some(Value::Object(sec)) = root.get(&e.section) {
                if let Some(Value::String(val)) = sec.get(&e.config_key) {
                    if !val.is_empty() {
                        e.current_value = val.clone();
                    }
                }
            }
        }

        // checkbox entries
        for e in &mut self.checkbox_entries {
            if let Some(Value::Object(sec)) = root.get(&e.section) {
                if let Some(val) = sec.get(&e.config_key) {
                    e.checked = match val {
                        Value::Bool(b) => *b,
                        Value::String(s) => s == "true",
                        _ => e.checked,
                    };
                }
            }
        }
    }

    fn save_config_to(&self, path: &Path) {
        // Ensure parent dirs exist
        if let Some(parent) = path.parent() {
            platform::make_dirs(parent);
        }

        // Read existing file to preserve fields we don't own
        let mut root: Map<String, Value> = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
            .unwrap_or_default();

        // sicompass: colorScheme
        let sc = root.entry("sicompass".to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = sc {
            m.insert("colorScheme".to_owned(), Value::String(self.color_scheme.clone()));
        }

        for e in &self.radio_entries {
            let sec = root.entry(e.section.clone()).or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(m) = sec {
                m.insert(e.config_key.clone(), Value::String(e.current_value.clone()));
            }
        }
        for e in &self.text_entries {
            let sec = root.entry(e.section.clone()).or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(m) = sec {
                m.insert(e.config_key.clone(), Value::String(e.current_value.clone()));
            }
        }
        for e in &self.checkbox_entries {
            let sec = root.entry(e.section.clone()).or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(m) = sec {
                m.insert(e.config_key.clone(), Value::Bool(e.checked));
            }
        }

        if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
            let _ = std::fs::write(path, json);
        }
    }

    fn save_config_if_possible(&self) {
        if let Some(path) = self.config_path() {
            self.save_config_to(&path);
        }
    }

    fn fire_apply(&self, key: &str, value: &str) {
        if let Some(f) = &self.apply_fn {
            f(key, value);
        }
    }

    fn fire_all_apply(&self) {
        self.fire_apply("colorScheme", &self.color_scheme.clone());
        for e in &self.radio_entries {
            self.fire_apply(&e.config_key.clone(), &e.current_value.clone());
        }
        for e in &self.text_entries {
            self.fire_apply(&e.config_key.clone(), &e.current_value.clone());
        }
        for e in &self.checkbox_entries {
            let val = if e.checked { "true" } else { "false" };
            self.fire_apply(&e.config_key.clone(), val);
        }
    }

    // ---- FFON tree building ------------------------------------------------

    fn populate_section(&self, section_name: &str) -> FfonElement {
        let mut obj = FfonElement::new_obj(section_name);
        let o = obj.as_obj_mut().unwrap();

        // Checkboxes (sorted alphabetically by label)
        let mut checkboxes: Vec<&CheckboxEntry> = self.checkbox_entries.iter()
            .filter(|e| e.section == section_name)
            .collect();
        checkboxes.sort_by(|a, b| a.label.to_ascii_lowercase().cmp(&b.label.to_ascii_lowercase()));
        for e in checkboxes {
            let tag = if e.checked { "<checkbox checked>" } else { "<checkbox>" };
            o.push(FfonElement::Str(format!("{}{}", tag, e.label)));
        }

        // Radio groups
        for e in &self.radio_entries {
            if e.section == section_name {
                let mut radio = FfonElement::new_obj(format!("<radio>{}", e.radio_key));
                let ro = radio.as_obj_mut().unwrap();
                for opt in &e.options {
                    let s = if *opt == e.current_value {
                        format!("<checked>{opt}")
                    } else {
                        opt.clone()
                    };
                    ro.push(FfonElement::Str(s));
                }
                o.push(radio);
            }
        }

        // Text entries
        for e in &self.text_entries {
            if e.section == section_name {
                o.push(FfonElement::Str(format!(
                    "{}: <input>{}</input>",
                    e.label, e.current_value
                )));
            }
        }

        if o.children.is_empty() {
            o.push(FfonElement::Str("no settings".to_owned()));
        }

        obj
    }
}

impl Provider for SettingsProvider {
    fn name(&self) -> &str { "settings" }

    fn meta(&self) -> Vec<String> {
        vec![
            "/   Search".to_owned(),
            "F5  Refresh".to_owned(),
        ]
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let mut result = Vec::new();

        // Priority section first
        if let Some(ref prio) = self.priority_section.clone() {
            result.push(self.populate_section(prio));
        }

        // sicompass section: color scheme radio group
        let is_dark = self.color_scheme == "dark";
        let mut sc_obj = FfonElement::new_obj("sicompass");
        {
            let mut radio = FfonElement::new_obj("<radio>color scheme");
            let ro = radio.as_obj_mut().unwrap();
            ro.push(FfonElement::Str(if is_dark { "<checked>dark".to_owned() } else { "dark".to_owned() }));
            ro.push(FfonElement::Str(if is_dark { "light".to_owned() } else { "<checked>light".to_owned() }));
            sc_obj.as_obj_mut().unwrap().push(radio);
        }
        // Also add any registered sicompass section entries
        let prio = self.priority_section.clone();
        for e in &self.checkbox_entries {
            if e.section == "sicompass" {
                let tag = if e.checked { "<checkbox checked>" } else { "<checkbox>" };
                sc_obj.as_obj_mut().unwrap().push(FfonElement::Str(format!("{}{}", tag, e.label)));
            }
        }
        for e in &self.text_entries {
            if e.section == "sicompass" {
                sc_obj.as_obj_mut().unwrap().push(FfonElement::Str(format!(
                    "{}: <input>{}</input>",
                    e.label, e.current_value
                )));
            }
        }
        result.push(sc_obj);

        // Other sections (skip sicompass and priority — already rendered), sorted alphabetically
        let mut other_sections: Vec<String> = self.sections.iter()
            .filter(|s| s.as_str() != "sicompass" && prio.as_deref() != Some(s.as_str()))
            .cloned()
            .collect();
        other_sections.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        for section in other_sections {
            result.push(self.populate_section(&section));
        }

        result
    }

    fn init(&mut self) {
        self.current_path = "/".to_owned();
        if let Some(path) = self.config_path() {
            self.load_config(&path);
        }
        self.fire_all_apply();
    }

    fn push_path(&mut self, segment: &str) {
        let segment = segment.trim_end_matches('/');
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path.push('/');
            self.current_path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        if self.current_path.len() <= 1 { return; }
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        // Path format: /<section>/<label>
        let path = self.current_path.clone();
        let parts: Vec<&str> = path.trim_start_matches('/').splitn(2, '/').collect();
        if parts.len() < 2 { return false; }
        let (section, label) = (parts[0], parts[1]);

        if let Some(e) = self.text_entries.iter_mut()
            .find(|e| e.section == section && e.label == label)
        {
            if e.current_value == new_content { return true; }
            e.current_value = new_content.to_owned();
            let config_key = e.config_key.clone();
            self.save_config_if_possible();
            self.fire_apply(&config_key, new_content);
            return true;
        }
        false
    }

    fn on_radio_change(&mut self, group_key: &str, selected_value: &str) {
        if group_key == "color scheme" {
            if self.color_scheme == selected_value { return; }
            self.color_scheme = selected_value.to_owned();
            self.save_config_if_possible();
            self.fire_apply("colorScheme", selected_value);
            return;
        }
        if let Some(e) = self.radio_entries.iter_mut().find(|e| e.radio_key == group_key) {
            if e.current_value == selected_value { return; }
            e.current_value = selected_value.to_owned();
            let config_key = e.config_key.clone();
            self.save_config_if_possible();
            self.fire_apply(&config_key, selected_value);
        }
    }

    fn on_checkbox_change(&mut self, label: &str, checked: bool) {
        if let Some(e) = self.checkbox_entries.iter_mut().find(|e| e.label == label) {
            if e.checked == checked { return; }
            e.checked = checked;
            let config_key = e.config_key.clone();
            self.save_config_if_possible();
            self.fire_apply(&config_key, if checked { "true" } else { "false" });
        }
    }

    fn add_settings_section(&mut self, name: &str) {
        self.add_section(name);
    }

    fn remove_settings_section(&mut self, name: &str) {
        self.remove_section(name);
    }
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_settings/test_settings.c (35 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn headless() -> SettingsProvider {
        SettingsProvider::new_headless()
    }

    fn with_callback() -> (SettingsProvider, Arc<Mutex<Vec<(String, String)>>>) {
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let p = SettingsProvider::new(move |k, v| {
            log2.lock().unwrap().push((k.to_owned(), v.to_owned()));
        });
        (p, log)
    }

    // --- fetch structure ---

    #[test]
    fn test_fetch_has_sicompass_section() {
        let mut p = headless();
        let elems = p.fetch();
        let has_sc = elems.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "sicompass"));
        assert!(has_sc);
    }

    #[test]
    fn test_fetch_sicompass_has_color_scheme_radio() {
        let mut p = headless();
        let elems = p.fetch();
        let sc = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "sicompass")).unwrap();
        let radio = sc.as_obj().unwrap().children.iter().find(|c| {
            c.as_obj().map_or(false, |o| o.key.contains("<radio>"))
        });
        assert!(radio.is_some());
    }

    #[test]
    fn test_fetch_dark_scheme_is_checked() {
        let mut p = headless(); // default is dark
        let elems = p.fetch();
        let sc = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "sicompass")).unwrap();
        let radio = sc.as_obj().unwrap().children.iter()
            .find(|c| c.as_obj().map_or(false, |o| o.key.contains("<radio>")))
            .unwrap();
        let dark_checked = radio.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s.contains("<checked>") && s.contains("dark"))
        });
        assert!(dark_checked);
    }

    // --- add_section / remove_section ---

    #[test]
    fn test_add_section_appears_in_fetch() {
        let mut p = headless();
        p.add_section("my section");
        let elems = p.fetch();
        let has = elems.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "my section"));
        assert!(has);
    }

    #[test]
    fn test_add_section_idempotent() {
        let mut p = headless();
        p.add_section("x");
        p.add_section("x");
        assert_eq!(p.sections.iter().filter(|s| *s == "x").count(), 1);
    }

    #[test]
    fn test_remove_section() {
        let mut p = headless();
        p.add_section("removable");
        p.remove_section("removable");
        let elems = p.fetch();
        let has = elems.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "removable"));
        assert!(!has);
    }

    #[test]
    fn test_remove_section_removes_its_entries() {
        let mut p = headless();
        p.add_text("sec", "label", "key", "value");
        p.remove_section("sec");
        assert!(p.text_entries.is_empty());
    }

    // --- add_radio ---

    #[test]
    fn test_add_radio_creates_section() {
        let mut p = headless();
        p.add_radio("my_sec", "sort order", "sortOrder", &["name", "date"], "name");
        assert!(p.sections.contains(&"my_sec".to_owned()));
    }

    #[test]
    fn test_add_radio_appears_in_fetch() {
        let mut p = headless();
        p.add_radio("test_sec", "sort", "sortOrder", &["name", "date"], "date");
        p.add_section("test_sec");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "test_sec")).unwrap();
        let radio = sec.as_obj().unwrap().children.iter().find(|c| {
            c.as_obj().map_or(false, |o| o.key.contains("<radio>"))
        });
        assert!(radio.is_some());
    }

    #[test]
    fn test_radio_default_value_is_checked() {
        let mut p = headless();
        p.add_radio("s", "sort", "sortOrder", &["name", "date"], "date");
        p.add_section("s");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "s")).unwrap();
        let radio = sec.as_obj().unwrap().children.iter()
            .find(|c| c.as_obj().map_or(false, |o| o.key.contains("<radio>")))
            .unwrap();
        let date_checked = radio.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s.contains("<checked>") && s.contains("date"))
        });
        assert!(date_checked);
    }

    // --- add_text ---

    #[test]
    fn test_add_text_appears_in_fetch() {
        let mut p = headless();
        p.add_text("s", "Server URL", "serverUrl", "https://example.com");
        p.add_section("s");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "s")).unwrap();
        let has_text = sec.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s.contains("Server URL") && s.contains("<input>"))
        });
        assert!(has_text);
    }

    #[test]
    fn test_add_text_default_value_in_input_tag() {
        let mut p = headless();
        p.add_text("s", "Host", "host", "localhost");
        p.add_section("s");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "s")).unwrap();
        let has = sec.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s.contains("<input>localhost</input>"))
        });
        assert!(has);
    }

    // --- add_checkbox ---

    #[test]
    fn test_add_checkbox_unchecked() {
        let mut p = headless();
        p.add_checkbox("s", "Enable feature", "enableFeature", false);
        p.add_section("s");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "s")).unwrap();
        let has = sec.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s == "<checkbox>Enable feature")
        });
        assert!(has);
    }

    #[test]
    fn test_add_checkbox_checked() {
        let mut p = headless();
        p.add_checkbox("s", "Feature", "feature", true);
        p.add_section("s");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "s")).unwrap();
        let has = sec.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s.starts_with("<checkbox checked>"))
        });
        assert!(has);
    }

    // --- priority section ---

    #[test]
    fn test_priority_section_comes_after_meta_and_before_sicompass() {
        let mut p = headless();
        p.add_checkbox("prio", "item", "key", false);
        p.add_priority_section("prio");
        let elems = p.fetch();
        // Order: [prio, sicompass, ...]
        assert_eq!(elems[0].as_obj().unwrap().key, "prio");
        assert_eq!(elems[1].as_obj().unwrap().key, "sicompass");
    }

    // --- on_radio_change ---

    #[test]
    fn test_on_radio_change_color_scheme() {
        let (mut p, log) = with_callback();
        p.on_radio_change("color scheme", "light");
        assert_eq!(p.color_scheme, "light");
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, v)| k == "colorScheme" && v == "light"));
    }

    #[test]
    fn test_on_radio_change_custom_radio() {
        let (mut p, log) = with_callback();
        p.add_radio("sec", "sort", "sortOrder", &["name", "date"], "name");
        p.on_radio_change("sort", "date");
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, v)| k == "sortOrder" && v == "date"));
    }

    #[test]
    fn test_on_radio_change_color_scheme_same_value_is_noop() {
        let (mut p, log) = with_callback();
        // default is "dark" — changing to "dark" again must not fire callback or save
        p.on_radio_change("color scheme", "dark");
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn test_on_radio_change_custom_same_value_is_noop() {
        let (mut p, log) = with_callback();
        p.add_radio("sec", "sort", "sortOrder", &["name", "date"], "name");
        // "name" is the default — firing same value must not fire callback or save
        p.on_radio_change("sort", "name");
        assert!(log.lock().unwrap().is_empty());
    }

    // --- on_checkbox_change ---

    #[test]
    fn test_on_checkbox_change() {
        let (mut p, log) = with_callback();
        p.add_checkbox("sec", "my label", "myKey", false);
        p.on_checkbox_change("my label", true);
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, v)| k == "myKey" && v == "true"));
    }

    #[test]
    fn test_on_checkbox_change_same_value_is_noop() {
        let (mut p, log) = with_callback();
        p.add_checkbox("sec", "my label", "myKey", false);
        p.on_checkbox_change("my label", false); // same as default — no callback, no save
        assert!(log.lock().unwrap().is_empty());
    }

    // --- commit_edit (text entries) ---

    #[test]
    fn test_commit_edit_updates_text_entry() {
        let (mut p, log) = with_callback();
        p.add_text("sec", "Server", "serverKey", "old");
        p.set_current_path("/sec/Server");
        let ok = p.commit_edit("old", "new_value");
        assert!(ok);
        assert_eq!(p.text_entries[0].current_value, "new_value");
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, v)| k == "serverKey" && v == "new_value"));
    }

    #[test]
    fn test_commit_edit_same_value_is_noop() {
        let (mut p, log) = with_callback();
        p.add_text("sec", "Server", "serverKey", "existing");
        p.set_current_path("/sec/Server");
        // Same value — must return true but not fire callback or save
        let ok = p.commit_edit("existing", "existing");
        assert!(ok);
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn test_commit_edit_unknown_returns_false() {
        let mut p = headless();
        p.set_current_path("/nosection/nolabel");
        assert!(!p.commit_edit("x", "y"));
    }

    // --- load / save config ---

    #[test]
    fn test_save_and_load_color_scheme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.color_scheme = "light".to_owned();
        p.save_config_to(&path);

        let mut p2 = SettingsProvider::new_headless().with_config_path(path.clone());
        p2.init();
        assert_eq!(p2.color_scheme, "light");
    }

    #[test]
    fn test_save_and_load_radio_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.add_radio("file browser", "sort", "sortOrder", &["name", "date"], "name");
        p.on_radio_change("sort", "date");

        let mut p2 = SettingsProvider::new_headless().with_config_path(path.clone());
        p2.add_radio("file browser", "sort", "sortOrder", &["name", "date"], "name");
        p2.init();
        assert_eq!(p2.radio_entries[0].current_value, "date");
    }

    #[test]
    fn test_save_and_load_text_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.add_text("sec", "Host", "host", "");
        p.set_current_path("/sec/Host");
        p.commit_edit("", "myserver.com");

        let mut p2 = SettingsProvider::new_headless().with_config_path(path.clone());
        p2.add_text("sec", "Host", "host", "");
        p2.init();
        assert_eq!(p2.text_entries[0].current_value, "myserver.com");
    }

    #[test]
    fn test_save_and_load_checkbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.add_checkbox("sec", "feature", "featureKey", false);
        p.on_checkbox_change("feature", true);

        let mut p2 = SettingsProvider::new_headless().with_config_path(path.clone());
        p2.add_checkbox("sec", "feature", "featureKey", false);
        p2.init();
        assert!(p2.checkbox_entries[0].checked);
    }

    #[test]
    fn test_save_preserves_existing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        // Write a file with an existing unknown key
        std::fs::write(
            &path,
            r#"{"other": {"untouched": "value"}, "sicompass": {}}"#,
        ).unwrap();

        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.color_scheme = "light".to_owned();
        p.save_config_to(&path);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("untouched"));
        assert!(content.contains("light"));
    }

    // --- set_checkbox_state ---

    #[test]
    fn test_set_checkbox_state() {
        let mut p = headless();
        p.add_checkbox("sec", "feat", "featKey", false);
        p.set_checkbox_state("featKey", true);
        assert!(p.checkbox_entries[0].checked);
    }

    // --- path management ---

    #[test]
    fn test_push_pop_path() {
        let mut p = headless();
        p.push_path("sicompass");
        assert_eq!(p.current_path(), "/sicompass");
        p.push_path("color scheme");
        assert_eq!(p.current_path(), "/sicompass/color scheme");
        p.pop_path();
        assert_eq!(p.current_path(), "/sicompass");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    // --- no settings placeholder ---

    #[test]
    fn test_empty_section_shows_no_settings() {
        let mut p = headless();
        p.add_section("empty_sec");
        let elems = p.fetch();
        let sec = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "empty_sec")).unwrap();
        let has_placeholder = sec.as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s == "no settings")
        });
        assert!(has_placeholder);
    }

    // --- init fires apply callback ---

    #[test]
    fn test_init_fires_apply_for_color_scheme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let mut p = SettingsProvider::new(move |k, v| {
            log2.lock().unwrap().push((k.to_owned(), v.to_owned()));
        }).with_config_path(path);
        p.init();
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, _)| k == "colorScheme"));
    }

    #[test]
    fn test_init_fires_apply_for_text_entries() {
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let mut p = SettingsProvider::new(move |k, v| {
            log2.lock().unwrap().push((k.to_owned(), v.to_owned()));
        });
        p.add_text("sales demo", "save folder", "saveFolder", "Downloads");
        p.init();
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, _)| k == "colorScheme"));
        assert!(entries.iter().any(|(k, v)| k == "saveFolder" && v == "Downloads"));
    }

    #[test]
    fn test_init_fires_apply_for_checkbox_entries() {
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let mut p = SettingsProvider::new(move |k, v| {
            log2.lock().unwrap().push((k.to_owned(), v.to_owned()));
        });
        p.add_checkbox("programs", "tutorial", "enable_tutorial", true);
        p.add_checkbox("programs", "file browser", "enable_file browser", false);
        p.init();
        let entries = log.lock().unwrap();
        assert!(entries.iter().any(|(k, _)| k == "colorScheme"));
        assert!(entries.iter().any(|(k, v)| k == "enable_tutorial" && v == "true"));
        assert!(entries.iter().any(|(k, v)| k == "enable_file browser" && v == "false"));
    }

    #[test]
    fn test_section_with_radio_and_text() {
        let mut p = SettingsProvider::new_headless();
        p.add_radio("mixed", "radio group", "radioKey", &["a", "b"], "a");
        p.add_text("mixed", "text field", "textKey", "hello");
        let items = p.fetch();
        // sicompass + mixed
        assert_eq!(items.len(), 2);
        let mixed = items[1].as_obj().unwrap();
        assert_eq!(mixed.key, "mixed");
        assert_eq!(mixed.children.len(), 2); // radio group + text entry
    }

    #[test]
    fn test_priority_section_not_duplicated() {
        let mut p = SettingsProvider::new_headless();
        p.add_priority_section("programs");
        p.add_checkbox("programs", "tutorial", "enable_tutorial", true);
        let items = p.fetch();
        // programs + sicompass — programs not duplicated
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_obj().unwrap().key, "programs");
        assert_eq!(items[1].as_obj().unwrap().key, "sicompass");
    }

    #[test]
    fn test_set_checkbox_state_no_change_skips() {
        let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cc2 = Arc::clone(&call_count);
        let mut p = SettingsProvider::new(move |_, _| {
            cc2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        p.add_checkbox("sicompass", "maximized", "maximized", false);
        // reset counter after construction (construction doesn't call set_checkbox_state)
        call_count.store(0, std::sync::atomic::Ordering::SeqCst);
        p.set_checkbox_state("maximized", false); // already false — should not call apply_fn
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn test_remove_section_removes_text_entries() {
        let mut p = SettingsProvider::new_headless();
        p.add_text("sales demo", "save folder", "saveFolder", "Downloads");
        p.remove_section("sales demo");
        // Re-add empty section to verify text entries are gone
        p.add_section("sales demo");
        let items = p.fetch();
        let sd = items.iter().find(|e| e.as_obj().map(|o| o.key == "sales demo").unwrap_or(false));
        assert!(sd.is_some());
        let children = &sd.unwrap().as_obj().unwrap().children;
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].as_str().unwrap(), "no settings");
    }

    #[test]
    fn test_remove_section_removes_checkbox_entries() {
        let mut p = SettingsProvider::new_headless();
        p.add_checkbox("programs", "tutorial", "enable_tutorial", true);
        p.remove_section("programs");
        p.add_section("programs");
        let items = p.fetch();
        let prog = items.iter().find(|e| e.as_obj().map(|o| o.key == "programs").unwrap_or(false));
        assert!(prog.is_some());
        let children = &prog.unwrap().as_obj().unwrap().children;
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].as_str().unwrap(), "no settings");
    }

    #[test]
    fn test_remove_section_nonexistent() {
        let mut p = SettingsProvider::new_headless();
        p.add_section("file browser");
        p.remove_section("nonexistent");
        let items = p.fetch();
        // sicompass + file browser still present
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_remove_section_leaves_other_sections() {
        let mut p = SettingsProvider::new_headless();
        p.add_radio("section A", "radio", "key", &["a", "b"], "a");
        p.add_text("section B", "label", "textKey", "value");
        p.remove_section("section A");
        let items = p.fetch();
        // sicompass + section B
        assert_eq!(items.len(), 2);
        assert!(!items.iter().any(|e| e.as_obj().map(|o| o.key == "section A").unwrap_or(false)));
        let sb = items.iter().find(|e| e.as_obj().map(|o| o.key == "section B").unwrap_or(false));
        assert!(sb.is_some());
        // section B still has its text entry
        assert_eq!(sb.unwrap().as_obj().unwrap().children.len(), 1);
    }

    #[test]
    fn test_other_sections_sorted_alphabetically() {
        let mut p = SettingsProvider::new_headless();
        p.add_priority_section("Available programs:");
        p.add_checkbox("Available programs:", "tutorial", "enable_tutorial", true);
        p.add_text("tutorial", "label", "key", "val");
        p.add_text("chat client", "label", "key", "val");
        p.add_text("email client", "label", "key", "val");
        p.add_text("web browser", "label", "key", "val");
        let items = p.fetch();
        // Expected order: Available programs:, sicompass, chat client, email client, tutorial, web browser
        let keys: Vec<&str> = items.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert_eq!(keys[0], "Available programs:");
        assert_eq!(keys[1], "sicompass");
        assert_eq!(keys[2], "chat client");
        assert_eq!(keys[3], "email client");
        assert_eq!(keys[4], "tutorial");
        assert_eq!(keys[5], "web browser");
    }
}
