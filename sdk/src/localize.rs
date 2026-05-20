//! Process-global localization layer built on [Project Fluent](https://projectfluent.org/).
//!
//! Each provider crate ships its own `locales/<lang>.ftl` files and calls
//! [`register_bundle`] at startup. The app reads the user's chosen language
//! from settings and calls [`set_locale`]. Provider code calls [`t`] /
//! [`t_args`] at render time to resolve message keys.

use fluent::bundle::FluentBundle as FluentBundleGen;
use fluent::{FluentArgs, FluentResource};
use intl_memoizer::concurrent::IntlLangMemoizer;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use unic_langid::LanguageIdentifier;

type FluentBundle = FluentBundleGen<FluentResource, IntlLangMemoizer>;

/// Source/fallback locale. Every translatable string MUST exist in this
/// locale's `.ftl` files — translation gaps in other locales fall back here.
pub const FALLBACK_LOCALE: &str = "en-US";

struct Localizer {
    /// Bundles keyed by canonical locale tag (e.g. "nl-BE").
    /// One bundle per locale aggregates resources from every provider crate.
    bundles: HashMap<String, FluentBundle>,
    active: String,
}

impl Localizer {
    fn new() -> Self {
        Self { bundles: HashMap::new(), active: FALLBACK_LOCALE.to_owned() }
    }

    fn add_resource(&mut self, locale: &str, ftl: &str) -> Result<(), String> {
        let resource = FluentResource::try_new(ftl.to_owned())
            .map_err(|(_, errs)| format!("FTL parse errors in {locale}: {errs:?}"))?;
        let entry = self.bundles.entry(locale.to_owned()).or_insert_with(|| {
            let langid: LanguageIdentifier =
                locale.parse().unwrap_or_else(|_| FALLBACK_LOCALE.parse().unwrap());
            let mut b = FluentBundle::new_concurrent(vec![langid]);
            // Suppress Unicode isolation marks (U+2068 / U+2069). They're
            // standard Fluent behavior for bidi safety but show up as
            // garbage characters in our text renderer.
            b.set_use_isolating(false);
            b
        });
        entry
            .add_resource(resource)
            .map_err(|errs| format!("FTL add errors in {locale}: {errs:?}"))
    }

    fn format(&self, key: &str, args: Option<&FluentArgs>) -> String {
        if let Some(s) = self.format_in(&self.active, key, args) {
            return s;
        }
        if self.active != FALLBACK_LOCALE {
            if let Some(s) = self.format_in(FALLBACK_LOCALE, key, args) {
                return s;
            }
        }
        // Loud failure: missing key. Returning the key itself makes gaps
        // obvious in the UI without crashing.
        key.to_owned()
    }

    fn format_in(&self, locale: &str, key: &str, args: Option<&FluentArgs>) -> Option<String> {
        let bundle = self.bundles.get(locale)?;
        let msg = bundle.get_message(key)?;
        let pattern = msg.value()?;
        let mut errors = Vec::new();
        let out = bundle.format_pattern(pattern, args, &mut errors);
        Some(out.into_owned())
    }
}

fn global() -> &'static RwLock<Localizer> {
    static G: OnceLock<RwLock<Localizer>> = OnceLock::new();
    G.get_or_init(|| RwLock::new(Localizer::new()))
}

/// Register a locale bundle from raw FTL source. Multiple calls with the
/// same locale append to that locale's bundle (so each provider crate can
/// own its own messages). Returns Err on parse/conflict failure.
pub fn register_bundle(locale: &str, ftl_source: &str) -> Result<(), String> {
    global().write().expect("localizer poisoned").add_resource(locale, ftl_source)
}

/// Set the active locale. Subsequent [`t`] / [`t_args`] calls resolve here
/// first, then fall back to [`FALLBACK_LOCALE`].
pub fn set_locale(locale: &str) {
    global().write().expect("localizer poisoned").active = locale.to_owned();
}

pub fn current_locale() -> String {
    global().read().expect("localizer poisoned").active.clone()
}

pub fn available_locales() -> Vec<String> {
    let mut v: Vec<String> =
        global().read().expect("localizer poisoned").bundles.keys().cloned().collect();
    v.sort();
    v
}

/// Resolve a Fluent message key in the active locale. Falls back to en-US,
/// then to the key itself. Use [`t_args`] for messages with `{ $param }`
/// placeholders.
pub fn t(key: &str) -> String {
    global().read().expect("localizer poisoned").format(key, None)
}

/// Resolve a Fluent message key with named parameters (e.g. `{ $err }`).
pub fn t_args(key: &str, args: &FluentArgs) -> String {
    global().read().expect("localizer poisoned").format(key, Some(args))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The Localizer is a process-global; tests must serialize so they don't
    // race on `active` or stomp each other's bundles.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset() {
        let mut g = global().write().unwrap();
        g.bundles.clear();
        g.active = FALLBACK_LOCALE.to_owned();
    }

    #[test]
    fn register_and_resolve_basic_key() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "hello = Hello").unwrap();
        assert_eq!(t("hello"), "Hello");
    }

    #[test]
    fn missing_key_returns_key_itself() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "hello = Hello").unwrap();
        assert_eq!(t("does-not-exist"), "does-not-exist");
    }

    #[test]
    fn set_locale_flips_resolution() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "hello = Hello").unwrap();
        register_bundle("nl-BE", "hello = Hallo").unwrap();
        register_bundle("fr-BE", "hello = Bonjour").unwrap();
        register_bundle("de-BE", "hello = Hallo").unwrap();

        set_locale("nl-BE");
        assert_eq!(t("hello"), "Hallo");
        set_locale("fr-BE");
        assert_eq!(t("hello"), "Bonjour");
        set_locale("de-BE");
        assert_eq!(t("hello"), "Hallo");
        set_locale("en-US");
        assert_eq!(t("hello"), "Hello");
    }

    #[test]
    fn missing_translation_falls_back_to_en_us() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "only-english = Only English").unwrap();
        register_bundle("nl-BE", "something-else = Iets anders").unwrap();
        set_locale("nl-BE");
        assert_eq!(t("only-english"), "Only English");
    }

    #[test]
    fn parameterized_message_substitutes() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "greet = Hello, { $name }!").unwrap();
        register_bundle("nl-BE", "greet = Hallo, { $name }!").unwrap();

        let mut args = FluentArgs::new();
        args.set("name", "Wereld");
        set_locale("nl-BE");
        assert_eq!(t_args("greet", &args), "Hallo, Wereld!");
    }

    #[test]
    fn multiple_provider_resources_merge_into_one_bundle() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "provider-a-name = File browser").unwrap();
        register_bundle("en-US", "provider-b-name = Web browser").unwrap();
        assert_eq!(t("provider-a-name"), "File browser");
        assert_eq!(t("provider-b-name"), "Web browser");
    }

    #[test]
    fn malformed_ftl_returns_error() {
        let _g = test_lock();
        reset();
        // Unterminated reference is a parse error in Fluent.
        let result = register_bundle("en-US", "= no key here");
        assert!(result.is_err(), "expected parse error, got {result:?}");
    }

    #[test]
    fn available_locales_lists_registered() {
        let _g = test_lock();
        reset();
        register_bundle("en-US", "k = v").unwrap();
        register_bundle("nl-BE", "k = v").unwrap();
        register_bundle("fr-BE", "k = v").unwrap();
        let mut got = available_locales();
        got.sort();
        assert_eq!(got, vec!["en-US", "fr-BE", "nl-BE"]);
    }

    #[test]
    fn no_isolation_marks_in_parameterized_output() {
        // Default Fluent wraps parameter substitutions in U+2068/U+2069 for
        // bidi safety. Our renderer can't handle those. Verify we suppress.
        let _g = test_lock();
        reset();
        register_bundle("en-US", "msg = before { $x } after").unwrap();
        let mut args = FluentArgs::new();
        args.set("x", "MIDDLE");
        let out = t_args("msg", &args);
        assert_eq!(out, "before MIDDLE after");
    }
}
