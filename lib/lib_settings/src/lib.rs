use serde_json::{Map, Value};
use sicompass_sdk::ffon::{FfonElement, IdArray};
use sicompass_sdk::localize;
use sicompass_sdk::platform;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::timeline::TimelineEntry;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Register this crate's translation bundles with the SDK localizer.
/// Idempotent — safe to call from `main()` and from individual tests.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

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
    /// Timeline entries accumulated since the last drain (via
    /// `take_timeline_entries`). Each mutation method (`commit_edit`,
    /// `on_radio_change`, `on_checkbox_change`) pushes a `ProviderOp`
    /// here. The `id` field is left as the empty `IdArray` — the app
    /// patches in the current_id when it drains.
    pending_timeline_entries: Vec<TimelineEntry>,
}

impl SettingsProvider {
    pub fn new(apply_fn: impl Fn(&str, &str) + Send + 'static) -> Self {
        register_translations();
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
            pending_timeline_entries: Vec::new(),
        }
    }

    /// Create without an apply callback (useful for testing fetch output).
    pub fn new_headless() -> Self {
        register_translations();
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
            pending_timeline_entries: Vec::new(),
        }
    }

    /// Build a `ProviderOp` payload for a settings mutation. Layout:
    /// `Obj { key: "section", children: [
    ///     Str("<config_key>"),
    ///     Str("<prev_value>"),
    ///     Str("<new_value>"),
    ///     Str("<kind>"),  // "text" | "radio" | "checkbox"
    /// ] }`
    /// The `kind` discriminator lets `undo`/`redo` route to write_key_string or
    /// write_key_bool. The outermost key carries the section so undo can
    /// re-target the same setting entry.
    fn build_provider_op_payload(
        section: &str,
        config_key: &str,
        prev: &str,
        new: &str,
        kind: &str,
    ) -> FfonElement {
        let mut obj = FfonElement::new_obj(section);
        if let Some(o) = obj.as_obj_mut() {
            o.push(FfonElement::Str(config_key.to_owned()));
            o.push(FfonElement::Str(prev.to_owned()));
            o.push(FfonElement::Str(new.to_owned()));
            o.push(FfonElement::Str(kind.to_owned()));
        }
        obj
    }

    fn parse_provider_op_payload(
        payload: &FfonElement,
    ) -> Option<(String, String, String, String, String)> {
        let obj = match payload {
            FfonElement::Obj(o) => o,
            _ => return None,
        };
        let section = obj.key.clone();
        let str_at = |i: usize| match obj.children.get(i)? {
            FfonElement::Str(s) => Some(s.clone()),
            _ => None,
        };
        Some((section, str_at(0)?, str_at(1)?, str_at(2)?, str_at(3)?))
    }

    fn push_settings_op(
        &mut self,
        command: &str,
        section: &str,
        config_key: &str,
        prev: &str,
        new: &str,
        kind: &str,
        label: &str,
    ) {
        let payload =
            Self::build_provider_op_payload(section, config_key, prev, new, kind);
        self.pending_timeline_entries.push(TimelineEntry::ProviderOp {
            provider_idx: 0, // patched by app on drain
            command: command.to_owned(),
            payload,
            label: label.to_owned(),
        });
        let _ = IdArray::new(); // suppress unused-import lint if IdArray ends up unused
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
        let write = if let Some(e) = self.checkbox_entries.iter_mut().find(|e| e.config_key == config_key) {
            if e.checked != checked {
                e.checked = checked;
                Some((e.section.clone(), e.config_key.clone()))
            } else {
                None
            }
        } else {
            None
        };
        if let Some((section, key)) = write {
            self.write_key_bool(&section, &key, checked);
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

    // On first run (settings.json absent), write a seed file containing only the
    // priority section's currently-checked entries (i.e. the default programs).
    // Nothing else is written — no colorScheme, no maximized, no other sections.
    fn seed_priority_section_on_disk(&self, path: &Path) {
        let Some(section_name) = self.priority_section.clone() else { return };
        if let Some(parent) = path.parent() { platform::make_dirs(parent); }
        let mut section_map = Map::new();
        for e in &self.checkbox_entries {
            if e.section == section_name && e.checked {
                section_map.insert(e.config_key.clone(), Value::Bool(true));
            }
        }
        if section_map.is_empty() { return; }
        let mut root = Map::new();
        root.insert(section_name, Value::Object(section_map));
        if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
            let _ = platform::atomic_write(path, &json);
        }
    }

    /// Load the settings root object for an in-place key write.
    ///
    /// Returns `Some(map)` when the file is absent (a fresh empty root — the
    /// legitimate first-write case) or parses cleanly as a JSON object.
    /// Returns `None` when the file exists but cannot be read or does not
    /// parse as an object. In that state another process is likely mid-write
    /// (a partial file parses as garbage), so the caller must abort rather
    /// than rebuild the file from an empty map — doing so would drop every
    /// section it is not currently touching.
    fn load_root_for_write(path: &Path) -> Option<Map<String, Value>> {
        match std::fs::read_to_string(path) {
            Ok(s) => match serde_json::from_str::<Value>(&s) {
                Ok(Value::Object(m)) => Some(m),
                _ => None,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Map::new()),
            Err(_) => None,
        }
    }

    // Write a single string key into section, preserving everything else in the file.
    fn write_key_string(&self, section: &str, key: &str, value: &str) {
        let Some(path) = self.config_path() else { return };
        if let Some(parent) = path.parent() { platform::make_dirs(parent); }
        // Abort rather than clobber: if the file exists but won't parse,
        // another process is likely mid-write — rebuilding from an empty map
        // here would drop every other section.
        let Some(mut root) = Self::load_root_for_write(&path) else {
            eprintln!(
                "sicompass: {} is unreadable or corrupt — setting not saved, \
                 file left intact for recovery",
                path.display()
            );
            return;
        };
        let sec = root.entry(section.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = sec {
            m.insert(key.to_owned(), Value::String(value.to_owned()));
        }
        if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
            let _ = platform::atomic_write(&path, &json);
        }
    }

    // Write a single boolean key into section, preserving everything else in the file.
    fn write_key_bool(&self, section: &str, key: &str, value: bool) {
        let Some(path) = self.config_path() else { return };
        if let Some(parent) = path.parent() { platform::make_dirs(parent); }
        // See `write_key_string` — abort on an unparseable file to avoid
        // clobbering a settings file another process is mid-write on.
        let Some(mut root) = Self::load_root_for_write(&path) else {
            eprintln!(
                "sicompass: {} is unreadable or corrupt — setting not saved, \
                 file left intact for recovery",
                path.display()
            );
            return;
        };
        let sec = root.entry(section.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = sec {
            m.insert(key.to_owned(), Value::Bool(value));
        }
        if let Ok(json) = serde_json::to_string_pretty(&Value::Object(root)) {
            let _ = platform::atomic_write(&path, &json);
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

    /// Resolve a radio option's display label. Convention: option value `X`
    /// under config key `Y` maps to Fluent message ID
    /// `settings-Y-option-X`. Falls back to the raw option value when no
    /// translation is registered (preserving the legacy behaviour where the
    /// stored value doubled as the display string — and avoiding garbage
    /// keys for numeric options like fontScale `"1.25"`).
    fn localize_option_label(config_key: &str, opt: &str) -> String {
        let key = format!("settings-{}-option-{}", config_key, opt);
        let label = localize::t(&key);
        if label == key { opt.to_owned() } else { label }
    }

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

        // Radio groups. The `radio_key` is treated as a Fluent message ID
        // first; if no translation is registered, `t()` falls back to the key
        // string itself (existing callers passing English literals still
        // render identically).
        for e in &self.radio_entries {
            if e.section == section_name {
                let mut radio = FfonElement::new_obj(format!(
                    "<radio>{}",
                    localize::t(&e.radio_key)
                ));
                let ro = radio.as_obj_mut().unwrap();
                for opt in &e.options {
                    let label = Self::localize_option_label(&e.config_key, opt);
                    let s = if *opt == e.current_value {
                        format!("<checked>{label}")
                    } else {
                        label
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
            let mut radio = FfonElement::new_obj(format!(
                "<radio>{}",
                localize::t("settings-radio-color-scheme")
            ));
            let ro = radio.as_obj_mut().unwrap();
            let dark_label = Self::localize_option_label("colorScheme", "dark");
            let light_label = Self::localize_option_label("colorScheme", "light");
            ro.push(FfonElement::Str(if is_dark {
                format!("<checked>{dark_label}")
            } else {
                dark_label
            }));
            ro.push(FfonElement::Str(if is_dark {
                light_label
            } else {
                format!("<checked>{light_label}")
            }));
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
        for e in &self.radio_entries {
            if e.section == "sicompass" && e.config_key != "colorScheme" {
                let mut radio = FfonElement::new_obj(format!(
                    "<radio>{}",
                    localize::t(&e.radio_key)
                ));
                let ro = radio.as_obj_mut().unwrap();
                for opt in &e.options {
                    let label = Self::localize_option_label(&e.config_key, opt);
                    let s = if *opt == e.current_value {
                        format!("<checked>{label}")
                    } else {
                        label
                    };
                    ro.push(FfonElement::Str(s));
                }
                sc_obj.as_obj_mut().unwrap().push(radio);
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
            if path.exists() {
                self.load_config(&path);
            } else {
                self.seed_priority_section_on_disk(&path);
            }
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
            let prev = e.current_value.clone();
            e.current_value = new_content.to_owned();
            let (sec, config_key, lbl) = (e.section.clone(), e.config_key.clone(), e.label.clone());
            self.write_key_string(&sec, &config_key, new_content);
            self.fire_apply(&config_key, new_content);
            self.push_settings_op(
                "settings-text",
                &sec,
                &config_key,
                &prev,
                new_content,
                "text",
                &format!("edit {lbl}"),
            );
            return true;
        }
        false
    }

    fn on_radio_change(&mut self, group_key: &str, selected_value: &str) {
        // The app extracts `group_key` and `selected_value` from the rendered
        // FFON (display strings). When translations are active, those don't
        // match the stored radio identifiers / option values any more. We
        // reverse-map both: a radio whose `radio_key` resolves through
        // `localize::t()` to the incoming `group_key`, and an option whose
        // `localize_option_label(config_key, opt)` matches the incoming
        // `selected_value`. Falls back to the raw incoming string so
        // English-literal callers (no FTL entry) still work.

        // Hardcoded color-scheme radio.
        let color_scheme_label = localize::t("settings-radio-color-scheme");
        if group_key == "color scheme" || group_key == color_scheme_label {
            let stored = if selected_value
                == Self::localize_option_label("colorScheme", "dark")
                || selected_value == "dark"
            {
                "dark"
            } else if selected_value
                == Self::localize_option_label("colorScheme", "light")
                || selected_value == "light"
            {
                "light"
            } else {
                // Unrecognized: store as-is for backward-compat / debugging.
                selected_value
            };
            if self.color_scheme == stored { return; }
            let prev = self.color_scheme.clone();
            self.color_scheme = stored.to_owned();
            self.write_key_string("sicompass", "colorScheme", stored);
            self.fire_apply("colorScheme", stored);
            self.push_settings_op(
                "settings-radio",
                "sicompass",
                "colorScheme",
                &prev,
                stored,
                "radio",
                "color scheme",
            );
            return;
        }

        // Dynamic radio entries: match group_key against either the raw
        // radio_key or its translated form.
        let entry_idx = self.radio_entries.iter().position(|e| {
            e.radio_key == group_key || localize::t(&e.radio_key) == group_key
        });
        if let Some(idx) = entry_idx {
            // Reverse-map the option label to the stored value. Match against
            // both the raw option string (legacy English-literal callers) and
            // the translated label.
            let stored = {
                let e = &self.radio_entries[idx];
                e.options
                    .iter()
                    .find(|opt| {
                        opt.as_str() == selected_value
                            || Self::localize_option_label(&e.config_key, opt) == selected_value
                    })
                    .cloned()
                    .unwrap_or_else(|| selected_value.to_owned())
            };

            let e = &mut self.radio_entries[idx];
            if e.current_value == stored { return; }
            let prev = e.current_value.clone();
            e.current_value = stored.clone();
            let (section, config_key, rkey) =
                (e.section.clone(), e.config_key.clone(), e.radio_key.clone());
            self.write_key_string(&section, &config_key, &stored);
            self.fire_apply(&config_key, &stored);
            self.push_settings_op(
                "settings-radio",
                &section,
                &config_key,
                &prev,
                &stored,
                "radio",
                &format!("set {rkey}"),
            );
        }
    }

    fn on_checkbox_change(&mut self, label: &str, checked: bool) {
        if let Some(e) = self.checkbox_entries.iter_mut().find(|e| e.label == label) {
            if e.checked == checked { return; }
            let prev = e.checked;
            e.checked = checked;
            let (section, config_key, lbl) =
                (e.section.clone(), e.config_key.clone(), e.label.clone());
            self.write_key_bool(&section, &config_key, checked);
            self.fire_apply(&config_key, if checked { "true" } else { "false" });
            self.push_settings_op(
                "settings-checkbox",
                &section,
                &config_key,
                if prev { "true" } else { "false" },
                if checked { "true" } else { "false" },
                "checkbox",
                &format!("toggle {lbl}"),
            );
        }
    }

    fn take_timeline_entries(&mut self) -> Vec<TimelineEntry> {
        std::mem::take(&mut self.pending_timeline_entries)
    }

    fn undo(&mut self, entry: &TimelineEntry, error: &mut String) {
        let payload = match entry {
            TimelineEntry::ProviderOp { command, payload, .. }
                if command.starts_with("settings-") =>
            {
                payload
            }
            _ => return,
        };
        let (section, key, prev, new, kind) = match Self::parse_provider_op_payload(payload) {
            Some(p) => p,
            None => {
                *error = "settings: malformed undo payload".to_owned();
                return;
            }
        };
        let _ = new;
        match kind.as_str() {
            "text" => {
                if let Some(e) = self
                    .text_entries
                    .iter_mut()
                    .find(|e| e.section == section && e.config_key == key)
                {
                    e.current_value = prev.clone();
                }
                self.write_key_string(&section, &key, &prev);
                self.fire_apply(&key, &prev);
            }
            "radio" => {
                if section == "sicompass" && key == "colorScheme" {
                    self.color_scheme = prev.clone();
                }
                if let Some(e) = self
                    .radio_entries
                    .iter_mut()
                    .find(|e| e.section == section && e.config_key == key)
                {
                    e.current_value = prev.clone();
                }
                self.write_key_string(&section, &key, &prev);
                self.fire_apply(&key, &prev);
            }
            "checkbox" => {
                let checked = prev == "true";
                if let Some(e) = self
                    .checkbox_entries
                    .iter_mut()
                    .find(|e| e.section == section && e.config_key == key)
                {
                    e.checked = checked;
                }
                self.write_key_bool(&section, &key, checked);
                self.fire_apply(&key, &prev);
            }
            _ => {}
        }
    }

    fn redo(&mut self, entry: &TimelineEntry, error: &mut String) {
        let payload = match entry {
            TimelineEntry::ProviderOp { command, payload, .. }
                if command.starts_with("settings-") =>
            {
                payload
            }
            _ => return,
        };
        let (section, key, _prev, new, kind) = match Self::parse_provider_op_payload(payload) {
            Some(p) => p,
            None => {
                *error = "settings: malformed redo payload".to_owned();
                return;
            }
        };
        match kind.as_str() {
            "text" => {
                if let Some(e) = self
                    .text_entries
                    .iter_mut()
                    .find(|e| e.section == section && e.config_key == key)
                {
                    e.current_value = new.clone();
                }
                self.write_key_string(&section, &key, &new);
                self.fire_apply(&key, &new);
            }
            "radio" => {
                if section == "sicompass" && key == "colorScheme" {
                    self.color_scheme = new.clone();
                }
                if let Some(e) = self
                    .radio_entries
                    .iter_mut()
                    .find(|e| e.section == section && e.config_key == key)
                {
                    e.current_value = new.clone();
                }
                self.write_key_string(&section, &key, &new);
                self.fire_apply(&key, &new);
            }
            "checkbox" => {
                let checked = new == "true";
                if let Some(e) = self
                    .checkbox_entries
                    .iter_mut()
                    .find(|e| e.section == section && e.config_key == key)
                {
                    e.checked = checked;
                }
                self.write_key_bool(&section, &key, checked);
                self.fire_apply(&key, &new);
            }
            _ => {}
        }
    }

    fn add_settings_section(&mut self, name: &str) {
        self.add_section(name);
    }

    fn remove_settings_section(&mut self, name: &str) {
        self.remove_section(name);
    }

    fn add_text_setting(&mut self, section: &str, label: &str,
                        config_key: &str, default: &str) {
        self.add_text(section, label, config_key, default);
    }

    fn add_checkbox_setting(&mut self, section: &str, label: &str,
                            config_key: &str, default_checked: bool) {
        self.add_checkbox(section, label, config_key, default_checked);
    }

    fn add_radio_setting(&mut self, section: &str, label: &str,
                         config_key: &str, options: &[&str], default: &str) {
        self.add_radio(section, label, config_key, options, default);
    }

    fn write_text_setting(&mut self, section: &str, key: &str, value: &str) {
        self.write_key_string(section, key, value);
    }

    fn add_priority_section(&mut self, name: &str) {
        // Inline the inherent add_priority_section body to avoid recursive
        // dispatch (both inherent and trait have the same name).
        self.priority_section = Some(name.to_owned());
        self.add_section(name);
    }

    fn set_apply_callback(&mut self, cb: Box<dyn Fn(&str, &str) + Send + 'static>) {
        self.apply_fn = Some(cb);
    }

    fn set_config_path(&mut self, path: std::path::PathBuf) {
        self.config_path_override = Some(path);
    }
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_settings/test_settings.c (35 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Shared temp dir for test helpers — prevents tests from writing to real config.
    fn test_config_path() -> PathBuf {
        std::env::temp_dir().join("sicompass-test-settings.json")
    }

    fn headless() -> SettingsProvider {
        SettingsProvider::new_headless()
            .with_config_path(test_config_path())
    }

    fn with_callback() -> (SettingsProvider, Arc<Mutex<Vec<(String, String)>>>) {
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let log2 = Arc::clone(&log);
        let p = SettingsProvider::new(move |k, v| {
            log2.lock().unwrap().push((k.to_owned(), v.to_owned()));
        }).with_config_path(test_config_path());
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

    /// PoC: the color-scheme radio key text flips when the active locale
    /// changes. Validates the end-to-end translation flow:
    /// constructor → register_translations → t() inside fetch() → FFON key.
    ///
    /// Mutates global locale state, so this test serializes against any
    /// other test that does the same via a process-static mutex.
    fn locale_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static L: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
        L.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn color_scheme_radio_key(p: &mut SettingsProvider) -> String {
        let elems = p.fetch();
        let sc = elems.iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key == "sicompass"))
            .expect("sicompass section");
        let radio = sc.as_obj().unwrap().children.iter()
            .find(|c| c.as_obj().map_or(false, |o| o.key.contains("<radio>")))
            .expect("color scheme radio");
        radio.as_obj().unwrap().key.clone()
    }

    /// Color-scheme radio options translate too (dark / light → donker /
    /// licht / foncé / clair / dunkel / hell). Stored value is still
    /// language-neutral ("dark"/"light"); only the displayed label flips.
    #[test]
    fn poc_color_scheme_options_translate_per_locale() {
        let _g = locale_test_lock();
        let mut p = headless();

        fn radio_option_strings(p: &mut SettingsProvider) -> Vec<String> {
            let elems = p.fetch();
            let sc = elems.iter()
                .find(|e| e.as_obj().map_or(false, |o| o.key == "sicompass"))
                .expect("sicompass section");
            let radio = sc.as_obj().unwrap().children.iter()
                .find(|c| c.as_obj().map_or(false, |o| o.key.contains("<radio>")))
                .expect("color scheme radio");
            radio.as_obj().unwrap().children.iter()
                .filter_map(|c| c.as_str().map(|s| s.to_owned()))
                .collect()
        }

        sicompass_sdk::localize::set_locale("nl-BE");
        let nl = radio_option_strings(&mut p);
        assert!(nl.iter().any(|s| s.contains("donker")), "nl-BE: {nl:?}");
        assert!(nl.iter().any(|s| s.contains("licht")),  "nl-BE: {nl:?}");

        sicompass_sdk::localize::set_locale("fr-BE");
        let fr = radio_option_strings(&mut p);
        assert!(fr.iter().any(|s| s.contains("foncé")), "fr-BE: {fr:?}");
        assert!(fr.iter().any(|s| s.contains("clair")), "fr-BE: {fr:?}");

        sicompass_sdk::localize::set_locale("de-BE");
        let de = radio_option_strings(&mut p);
        assert!(de.iter().any(|s| s.contains("dunkel")), "de-BE: {de:?}");
        assert!(de.iter().any(|s| s.contains("hell")),   "de-BE: {de:?}");

        sicompass_sdk::localize::set_locale("en-US");
    }

    /// Language-radio option labels show in each language's native form,
    /// regardless of the active locale — standard convention for language
    /// pickers so users can find their language even from a foreign UI.
    #[test]
    fn poc_language_option_labels_use_localize_helper() {
        let _g = locale_test_lock();
        // We don't have a SettingsProvider with a registered "language" radio
        // in this test crate (that registration lives in src/sicompass).
        // Verify the convention directly via the helper.
        let en = SettingsProvider::localize_option_label("language", "en-US");
        let nl = SettingsProvider::localize_option_label("language", "nl-BE");
        let fr = SettingsProvider::localize_option_label("language", "fr-BE");
        let de = SettingsProvider::localize_option_label("language", "de-BE");
        assert_eq!(en, "English");
        assert_eq!(nl, "Nederlands (België)");
        assert_eq!(fr, "Français (Belgique)");
        assert_eq!(de, "Deutsch (Belgien)");
    }

    /// Options without a translation entry (e.g. fontScale "1.25", which
    /// also contains a period that would make for an invalid Fluent ID) must
    /// fall back to the raw stored value rather than rendering the key.
    #[test]
    fn option_label_falls_back_to_raw_value_when_no_translation() {
        let _g = locale_test_lock();
        let label = SettingsProvider::localize_option_label("fontScale", "1.25");
        assert_eq!(label, "1.25");
    }

    /// Reproduces the exact registration the app uses for the language
    /// radio in `programs.rs`: add it as a dynamic radio with the four
    /// locale codes as options, then verify each option's *displayed*
    /// FFON Str text is the native language name, not the raw locale code.
    #[test]
    fn language_radio_options_render_native_names_via_dynamic_path() {
        let _g = locale_test_lock();
        let mut p = headless();
        p.add_radio(
            "sicompass",
            "settings-radio-language",
            "language",
            &["en-US", "nl-BE", "fr-BE", "de-BE"],
            "en-US",
        );

        // Default locale (en-US): all option Strs should show native names.
        sicompass_sdk::localize::set_locale("en-US");
        let elems = p.fetch();
        let sicompass_obj = elems.iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key == "sicompass"))
            .expect("sicompass section");
        let lang_radio = sicompass_obj.as_obj().unwrap().children.iter()
            .find(|c| c.as_obj().map_or(false, |o| {
                // The radio_key string is "settings-radio-language"; under en-US
                // it resolves to "language".
                o.key.contains("language") || o.key.contains("settings-radio-language")
            }))
            .expect("language radio");
        let option_strs: Vec<String> = lang_radio.as_obj().unwrap().children.iter()
            .filter_map(|c| c.as_str().map(|s| s.to_owned()))
            .collect();

        // None of the raw locale codes should appear as-is in the displayed
        // option text (each must be the native language name).
        for code in &["en-US", "nl-BE", "fr-BE", "de-BE"] {
            assert!(
                !option_strs.iter().any(|s| s.contains(code)),
                "raw locale code {code:?} leaked into displayed options: {option_strs:?}"
            );
        }
        for native in &[
            "English",
            "Nederlands (België)",
            "Français (Belgique)",
            "Deutsch (Belgien)",
        ] {
            assert!(
                option_strs.iter().any(|s| s.contains(native)),
                "native name {native:?} missing from displayed options: {option_strs:?}"
            );
        }

        sicompass_sdk::localize::set_locale("en-US");
    }

    #[test]
    fn poc_color_scheme_label_translates_for_each_belgian_locale() {
        let _g = locale_test_lock();
        let mut p = headless();

        sicompass_sdk::localize::set_locale("en-US");
        assert!(color_scheme_radio_key(&mut p).contains("color scheme"),
            "en-US should show English label");

        sicompass_sdk::localize::set_locale("nl-BE");
        assert!(color_scheme_radio_key(&mut p).contains("kleurenschema"),
            "nl-BE should show Flemish label");

        sicompass_sdk::localize::set_locale("fr-BE");
        assert!(color_scheme_radio_key(&mut p).contains("jeu de couleurs"),
            "fr-BE should show Belgian French label");

        sicompass_sdk::localize::set_locale("de-BE");
        assert!(color_scheme_radio_key(&mut p).contains("Farbschema"),
            "de-BE should show Belgian German label");

        // Reset so other tests start from a known state.
        sicompass_sdk::localize::set_locale("en-US");
    }

    #[test]
    fn test_fetch_sicompass_includes_extra_radio() {
        let mut p = headless();
        p.add_radio("sicompass", "font scale", "fontScale",
            &["0.500", "1.000", "2.000"], "1.000");
        let elems = p.fetch();
        let sc = elems.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "sicompass")).unwrap();
        let font_scale_radio = sc.as_obj().unwrap().children.iter().find(|c| {
            c.as_obj().map_or(false, |o| o.key == "<radio>font scale")
        });
        assert!(font_scale_radio.is_some(), "font scale radio missing from sicompass section");
        let selected = font_scale_radio.unwrap().as_obj().unwrap().children.iter().any(|c| {
            c.as_str().map_or(false, |s| s == "<checked>1.000")
        });
        assert!(selected, "default value 1.000 not marked as checked");
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

    /// Regression: when the UI is in a non-English locale, the radio change
    /// dispatcher passes the *translated* group key and option label into
    /// `on_radio_change`. The handler must reverse-map both back to the
    /// stored identifiers so settings.json gets "light"/"dark" — not
    /// "licht"/"donker".
    #[test]
    fn on_radio_change_color_scheme_accepts_translated_label() {
        let _g = locale_test_lock();
        let (mut p, log) = with_callback();
        sicompass_sdk::localize::set_locale("nl-BE");

        // The dispatcher would extract these display strings from the FFON.
        p.on_radio_change("kleurenschema", "licht");

        let entries = log.lock().unwrap();
        // The STORED value must still be "light", regardless of the
        // language-displayed label that triggered the change.
        assert!(
            entries.iter().any(|(k, v)| k == "colorScheme" && v == "light"),
            "expected stored value 'light', got: {:?}", *entries
        );
        drop(entries);
        assert_eq!(p.color_scheme, "light");

        sicompass_sdk::localize::set_locale("en-US");
    }

    #[test]
    fn on_radio_change_dynamic_radio_accepts_translated_label() {
        let _g = locale_test_lock();
        let (mut p, log) = with_callback();
        // Register a synthetic radio + matching FTL entries so localize() can
        // round-trip. The radio_key doubles as the Fluent message ID.
        p.add_radio("sec", "test-sort-radio", "testSortOrder",
                    &["asc", "desc"], "asc");
        let _ = sicompass_sdk::localize::register_bundle(
            "en-US",
            "test-sort-radio = sort order\n\
             settings-testSortOrder-option-asc = ascending\n\
             settings-testSortOrder-option-desc = descending\n",
        );
        let _ = sicompass_sdk::localize::register_bundle(
            "nl-BE",
            "test-sort-radio = sorteervolgorde\n\
             settings-testSortOrder-option-asc = oplopend\n\
             settings-testSortOrder-option-desc = aflopend\n",
        );

        sicompass_sdk::localize::set_locale("nl-BE");
        // Dispatcher would pass the translated group + option here.
        p.on_radio_change("sorteervolgorde", "aflopend");

        let entries = log.lock().unwrap();
        assert!(
            entries.iter().any(|(k, v)| k == "testSortOrder" && v == "desc"),
            "expected stored value 'desc', got: {:?}", *entries
        );

        sicompass_sdk::localize::set_locale("en-US");
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
        p.on_radio_change("color scheme", "light");

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
        p.on_radio_change("color scheme", "light");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("untouched"));
        assert!(content.contains("light"));
    }

    #[test]
    fn write_key_aborts_on_unparseable_file_instead_of_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        // Simulate a settings file caught mid-write by another process: a
        // truncated, unparseable fragment. The old read-modify-write fell back
        // to an empty map here and collapsed the file to a single key,
        // dropping every other section.
        let partial = r#"{"text editor": {"textEditorPath": "/home/nico/Dro"#;
        std::fs::write(&path, partial).unwrap();

        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.on_radio_change("color scheme", "light");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after, partial,
            "an unparseable settings file must be left untouched, not \
             clobbered with a one-key file"
        );
    }

    #[test]
    fn write_key_still_creates_file_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // No file on disk — a write must still seed it (the legitimate
        // first-write case must not be mistaken for corruption).
        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.on_radio_change("color scheme", "light");

        let content = std::fs::read_to_string(&path).expect("file must be created");
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

    // --- init seeds only enabled-by-default programs when file is missing ---

    #[test]
    fn test_init_seeds_only_default_programs_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.add_priority_section("Available programs:");
        p.add_checkbox("Available programs:", "tutorial",     "enable_tutorial",     true);
        p.add_checkbox("Available programs:", "sales demo",   "enable_sales demo",   false);
        p.add_checkbox("Available programs:", "chat client",  "enable_chat client",  false);
        // Unrelated settings that must NOT appear in the seeded file:
        p.add_radio("sicompass", "color scheme", "colorScheme", &["dark", "light"], "dark");
        p.add_checkbox("sicompass", "maximized", "maximized", false);
        p.add_radio("file browser", "sort order", "sortOrder",
            &["alphanumerically", "chronologically"], "alphanumerically");

        p.init();

        let data = std::fs::read_to_string(&path).expect("settings.json should have been created");
        let root: serde_json::Value = serde_json::from_str(&data).unwrap();

        // Only the enabled-by-default entry is written.
        let available = root.get("Available programs:").expect("Available programs: section missing");
        assert_eq!(available.get("enable_tutorial").and_then(|v| v.as_bool()), Some(true));
        assert!(available.get("enable_sales demo").is_none(), "disabled-by-default entries must not be written");
        assert!(available.get("enable_chat client").is_none(), "disabled-by-default entries must not be written");

        // No other sections.
        assert!(root.get("sicompass").is_none(), "sicompass section must not appear in seed");
        assert!(root.get("file browser").is_none(), "file browser section must not appear in seed");
    }

    #[test]
    fn test_init_does_not_overwrite_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"sicompass":{"colorScheme":"light"}}"#).unwrap();

        let mut p = SettingsProvider::new_headless().with_config_path(path.clone());
        p.add_priority_section("Available programs:");
        p.add_checkbox("Available programs:", "tutorial", "enable_tutorial", true);
        p.init();

        // Existing file must be unchanged (loaded, not overwritten).
        let data = std::fs::read_to_string(&path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(root["sicompass"]["colorScheme"].as_str(), Some("light"));
        // Seed must not have added Available programs: on top of the existing file.
        assert!(root.get("Available programs:").is_none());
    }

    #[test]
    fn set_apply_callback_fires_on_checkbox_change() {
        use std::sync::{Arc, Mutex};
        let fired: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let fired2 = Arc::clone(&fired);
        let mut p = SettingsProvider::new_headless()
            .with_config_path(test_config_path());
        p.set_apply_callback(Box::new(move |k, _v| {
            fired2.lock().unwrap().push(k.to_owned());
        }));
        p.add_checkbox("s", "my flag", "myFlag", false);
        p.on_checkbox_change("my flag", true);
        assert!(fired.lock().unwrap().contains(&"myFlag".to_owned()));
    }

    #[test]
    fn set_config_path_writes_to_override() {
        use sicompass_sdk::provider::Provider;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("override.json");
        let mut p = SettingsProvider::new_headless();
        p.set_config_path(path.clone());
        p.add_checkbox("sicompass", "flag", "testFlag", false);
        // on_checkbox_change should write to the override path, not the real config
        p.on_checkbox_change("flag", true);
        assert!(path.exists(), "on_checkbox_change should write to the override path");
        let data = std::fs::read_to_string(&path).unwrap();
        assert!(data.contains("testFlag"), "written config should contain the key");
    }

    #[test]
    fn add_priority_section_trait_method_registers_section() {
        use sicompass_sdk::provider::Provider;
        let mut p = SettingsProvider::new_headless()
            .with_config_path(test_config_path());
        Provider::add_priority_section(&mut p, "My Priority");
        p.add_checkbox("My Priority", "flag", "myFlag", false);
        let items = p.fetch();
        let has_section = items.iter().any(|e| {
            e.as_obj().map(|o| o.key == "My Priority").unwrap_or(false)
        });
        assert!(has_section, "priority section should appear in fetch output");
    }
}

// ---------------------------------------------------------------------------
// Default + SDK registration
// ---------------------------------------------------------------------------

impl Default for SettingsProvider {
    /// Create a headless `SettingsProvider` with no apply callback.
    /// Use `set_apply_callback` and `set_config_path` (via the `Provider` trait)
    /// to configure it after construction — enabling factory-registry creation
    /// without a direct dependency on this crate from the app.
    fn default() -> Self {
        Self::new_headless()
    }
}

/// Register the settings provider with the SDK factory registry.
///
/// The factory creates a headless `SettingsProvider`; the app configures it
/// afterwards via `Provider::set_apply_callback` and `Provider::set_config_path`.
pub fn register() {
    sicompass_sdk::register_provider_factory("settings", || {
        Box::new(SettingsProvider::default())
    });
}
