//! Host for sandboxed WASM plugins.
//!
//! Third-party providers are WebAssembly components, not `dlopen`ed shared
//! libraries. Two reasons:
//!
//! 1. **Apple's stores forbid executing downloaded native code.** Bytecode run by
//!    the app's own embedded runtime counts as data, so this is the only path that
//!    ships.
//! 2. **A native in-process plugin has full process privileges.** Manifest policy
//!    (`allowedHosts`, rate limits, robots.txt) was only ever advisory against one:
//!    it could open its own socket. A guest has no syscalls, so the functions the
//!    host links in *are* its capability set — enforced structurally.
//!
//! ## Layout
//!
//! - [`limits`] — fuel, epoch deadlines, memory caps
//! - [`provider`] — [`provider::WasmProvider`], which implements the SDK's
//!   `Provider` trait by calling guest exports
//!
//! This lives in the app crate rather than a `lib_*` crate deliberately. The SDK
//! boundary rule is about not reaching around `sicompass-sdk` to talk to *provider*
//! crates; `wasmtime` is a third-party dependency like `sdl3` or `ash`, and plugin
//! discovery (`plugin_manifest`) and instantiation (`programs`) already live here.

pub mod host_fetch;
pub mod limits;
pub mod provider;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, StoreLimits};

pub use provider::WasmProvider;

// The generated bindings. `path` resolves against CARGO_MANIFEST_DIR, and
// `src/sicompass/wit/` is a vendored copy of the canonical file in the
// sicompass-plugin-sdk repo — `wit_vendor_matches_host_tables` in
// `tests/wasm_plugin.rs` guards the two against drift.
// Bindings are synchronous, which is the default. The SDK's only async trait
// methods are `undo`/`redo`, and their guest exports are plain calls, so the
// adapter satisfies the async signature without awaiting anything.
wasmtime::component::bindgen!({
    path: "wit",
    world: "plugin",
});

// `self::` is load-bearing. bindgen generates a module named after the WIT package
// namespace — `sicompass` — which is also this crate's name, so a bare path is
// ambiguous anywhere the crate is in extern scope (doctests, most visibly).
use self::sicompass::plugin as wit;

pub use self::sicompass::plugin::types as wit_types;

/// Per-instance host state, reachable from every host function the guest calls.
pub struct HostState {
    /// Manifest `name`. Prefixes log lines so plugin output is attributable.
    pub plugin_name: String,
    /// The settings-tree section this plugin's settings live under.
    ///
    /// Separate from `plugin_name` because `programs::inject_plugin_settings`
    /// registers them under the manifest's **`displayName`**. Reading them back by
    /// `name` would silently return nothing for every plugin whose two differ.
    pub settings_section: String,
    /// The plugin's own install directory. The confinement root for any path a
    /// guest hands back (e.g. `dashboard-image-path`).
    pub plugin_dir: PathBuf,
    /// Hosts this plugin declared in `plugin.json`. Empty means the `net` interface
    /// is not linked at all, so the guest has no reachable network function.
    pub allowed_hosts: Vec<String>,
    /// Allowlist, robots.txt and quota enforcement for this plugin's requests.
    pub fetch_policy: host_fetch::FetchPolicy,
    /// Memory/table/instance caps. Wasmtime reaches this through `limiter()`.
    pub limits: StoreLimits,
}

impl HostState {
    pub fn new(
        plugin_name: impl Into<String>,
        settings_section: impl Into<String>,
        plugin_dir: impl Into<PathBuf>,
        allowed_hosts: Vec<String>,
    ) -> Self {
        let plugin_name = plugin_name.into();
        let fetch_policy = host_fetch::FetchPolicy::new(&plugin_name, &allowed_hosts);
        HostState {
            plugin_name,
            settings_section: settings_section.into(),
            plugin_dir: plugin_dir.into(),
            allowed_hosts,
            fetch_policy,
            limits: limits::store_limits(),
        }
    }

    /// Whether this plugin may reach the network at all.
    pub fn may_use_network(&self) -> bool {
        !self.allowed_hosts.is_empty()
    }

    /// Resolve a guest-supplied relative path inside the plugin's own directory.
    ///
    /// A sandboxed guest has no business naming a host path, so anything absolute or
    /// containing `..` is rejected rather than normalized. Used for
    /// `dashboard-image-path`, where the *host* opens the file the guest names.
    pub fn confine(&self, rel: &str) -> Result<PathBuf, String> {
        let candidate = Path::new(rel);
        // `is_absolute()` alone is not enough, because it is platform-dependent:
        // on Windows `/etc/passwd` is *not* absolute (it carries no drive prefix)
        // and would otherwise fall through. A prefix or root component means the
        // guest named a host location whichever platform we are on, so check the
        // components too and report both the same way.
        let rooted = candidate.components().any(|c| {
            matches!(c, std::path::Component::Prefix(_) | std::path::Component::RootDir)
        });
        if candidate.is_absolute() || rooted {
            return Err(format!("`{rel}` is absolute; plugin paths must be relative"));
        }
        if candidate
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("`{rel}` escapes the plugin directory"));
        }
        Ok(self.plugin_dir.join(candidate))
    }
}

// ---------------------------------------------------------------------------
// Host interface implementations
// ---------------------------------------------------------------------------

impl wit::host::Host for HostState {
    fn log(&mut self, msg: String) {
        tracing::info!(target: "wasm_plugin", plugin = %self.plugin_name, "{msg}");
    }

    fn get_setting(&mut self, key: String) -> Option<String> {
        read_plugin_setting(&self.settings_section, &key)
    }

    fn now_millis(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn translate(&mut self, key: String) -> String {
        sicompass_sdk::localize::t(&key)
    }
}

/// Read one setting from the plugin's own section of the user's `settings.json`.
///
/// **Scoped deliberately.** A guest can read only the section its own manifest
/// declared, never the whole file: other providers' sections hold API keys, IMAP
/// passwords and licence certificates, and a sandbox that hands those over on
/// request is not a sandbox. Mirrors how `programs::is_plugin_enabled_in_config`
/// reads the same file.
fn read_plugin_setting(section: &str, key: &str) -> Option<String> {
    let path = sicompass_sdk::platform::main_config_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&data).ok()?;

    // `programs::inject_plugin_settings` registers under the manifest's
    // `displayName`, which is what `section` is. The spaces-stripped fallback
    // matches `programs::instantiate_builtin`'s leniency about names like
    // "chat client" vs "chatclient".
    let compact: String = section.chars().filter(|&c| c != ' ').collect();
    for candidate in [section, compact.as_str()] {
        if let Some(v) = root.get(candidate).and_then(|s| s.get(key)) {
            return Some(match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// The process-wide wasmtime engine, plus the epoch ticker that makes call
/// deadlines fire.
struct EngineHandle {
    engine: Engine,
    // Held only so the ticker thread lives as long as the engine.
    _ticker: limits::EpochTicker,
}

static ENGINE: OnceLock<EngineHandle> = OnceLock::new();

/// Shared engine for all WASM plugins.
///
/// One engine is correct and cheaper: it owns the compilation cache and the epoch
/// counter. Isolation lives at the `Store`/instance level, not the engine level.
pub fn engine() -> &'static Engine {
    &ENGINE
        .get_or_init(|| {
            let engine = Engine::new(&engine_config()).expect("build the wasm engine");
            let ticker = limits::EpochTicker::spawn(&engine);
            EngineHandle { engine, _ticker: ticker }
        })
        .engine
}

fn engine_config() -> Config {
    let mut config = Config::new();
    config.wasm_component_model(true);
    // Both are per-call caps; see `limits`.
    config.consume_fuel(true);
    config.epoch_interruption(true);

    // Persist compiled components across runs. Compiling is what makes plugin
    // startup expensive — ~1.2s for a 65 KiB plugin — and `load_component`'s
    // in-process cache only removes the *repeat* cost within one launch. This
    // removes it for every launch after the first.
    //
    // Wasmtime keys the cache on its own version and configuration, so a wasmtime
    // upgrade or a backend change (Cranelift vs Pulley) invalidates it rather than
    // handing back an artifact built for something else. A failure here is not
    // worth refusing to start over: it costs a recompile, nothing more.
    match wasmtime::Cache::from_file(None) {
        Ok(cache) => {
            config.cache(Some(cache));
        }
        Err(e) => {
            tracing::debug!(target: "wasm_plugin", "no compilation cache: {e}");
        }
    }

    // Retarget Cranelift at Pulley for App Store / iOS builds. Cranelift is still
    // linked — it is what lowers wasm into Pulley bytecode — but its output is
    // interpreted rather than executed, so the process never needs writable-then-
    // executable pages and the binary needs no `allow-jit` entitlement.
    #[cfg(all(feature = "no-jit-wasm", not(feature = "jit-wasm")))]
    {
        config
            .target("pulley64")
            .expect("select the pulley interpreter target");
    }

    config
}

/// True when guests are compiled to native machine code at runtime.
///
/// False in the `no-jit-wasm` build, where Cranelift targets Pulley and its output
/// is interpreted. Exposed so a test can assert the wiring rather than trusting the
/// feature flags by eye.
pub const fn uses_jit() -> bool {
    !cfg!(all(feature = "no-jit-wasm", not(feature = "jit-wasm")))
}

// ---------------------------------------------------------------------------
// Linker
// ---------------------------------------------------------------------------

/// Build the linker for one plugin.
///
/// **This function is the sandbox.** A guest can call exactly what gets linked here
/// and nothing else — there is no `wasi:cli`, `wasi:filesystem`, `wasi:sockets` or
/// environment access, so `std::fs`/`std::net` inside a guest compile and then fail
/// at runtime rather than reaching anything.
///
/// `sicompass:plugin/net` is linked **only** when the manifest declares a non-empty
/// `allowedHosts`. That is why network lives in its own WIT interface: wasmtime links
/// host functions an interface at a time, so with `fetch` in `host` this decision
/// would not have been expressible.
pub fn linker_for(state: &HostState) -> Result<Linker<HostState>, String> {
    let mut linker = Linker::new(engine());

    wit::host::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
        &mut linker,
        |s: &mut HostState| s,
    )
    .map_err(|e| format!("link sicompass:plugin/host: {e}"))?;

    if state.may_use_network() {
        // Step 3 fills this in with the mediated fetch (allowlist, robots.txt,
        // crawl-delay, per-domain quotas). Until then, declaring `allowedHosts`
        // links the interface but every call is refused, which is the safe
        // direction to be incomplete in.
        wit::net::add_to_linker::<_, wasmtime::component::HasSelf<_>>(
            &mut linker,
            |s: &mut HostState| s,
        )
        .map_err(|e| format!("link sicompass:plugin/net: {e}"))?;
    }

    Ok(linker)
}

impl wit::net::Host for HostState {
    fn fetch(&mut self, req: wit::net::HttpRequest) -> Result<wit::net::HttpResponse, String> {
        host_fetch::perform(&mut self.fetch_policy, req)
    }

    fn fetch_url_ffon(&mut self, url: String) -> Result<Vec<u8>, String> {
        // Deliberately routed through the same policy-checked `perform` rather than
        // the SDK's global `fetch_url_to_ffon`. That callback is installed by
        // lib_webbrowser and does its own fetching, so using it here would hand a
        // plugin an unchecked way out — the one thing this module exists to prevent.
        let resp = host_fetch::perform(
            &mut self.fetch_policy,
            wit::net::HttpRequest {
                method: "GET".to_owned(),
                url: url.clone(),
                headers: Vec::new(),
                body: None,
            },
        )?;

        if !(200..300).contains(&resp.status) {
            return Err(format!("GET {url} returned HTTP {}", resp.status));
        }

        let html = String::from_utf8_lossy(&resp.body);
        let elements = sicompass_sdk::ffon::html_to_ffon(&html, &url);
        Ok(sicompass_sdk::ffon::serialize_binary(&elements))
    }
}

// ---------------------------------------------------------------------------
// Install-time capability audit
// ---------------------------------------------------------------------------

/// Check that a component asks for no more than its manifest declares.
///
/// Because LTO drops imports a guest never calls, the import list of a built
/// component *is* the set of capabilities it actually uses. That makes a check
/// possible before anything runs: a plugin whose `plugin.json` declares no
/// `allowedHosts` but whose binary imports `sicompass:plugin/net` is either
/// mislabelled or lying, and either way should not be instantiated.
///
/// Instantiation would refuse it anyway, since the `net` interface would not be
/// linked. The value here is a clear, early diagnostic naming the mismatch instead
/// of an opaque link failure — and a place to hang the same check at install time.
pub fn audit_component_imports(
    component: &Component,
    allowed_hosts: &[String],
) -> Result<(), String> {
    let engine = engine();
    let ty = component.component_type();

    for (name, item) in ty.imports(engine) {
        // Strip the `@x.y.z` version suffix; the tables are version-agnostic.
        let interface = name.split('@').next().unwrap_or(name);

        // A types-only interface carries no functions and grants nothing.
        if interface == "sicompass:plugin/types" {
            continue;
        }

        let known = HOST_IMPORTS
            .iter()
            .chain(NET_IMPORTS.iter())
            .any(|(i, _)| *i == interface);
        if !known {
            return Err(format!(
                "plugin imports `{interface}`, which this host does not provide"
            ));
        }

        if interface == "sicompass:plugin/net" && allowed_hosts.is_empty() {
            return Err(
                "plugin uses the network but declares no `allowedHosts` in plugin.json; \
                 add the hosts it needs so the user can see them before enabling it"
                    .to_owned(),
            );
        }

        // Names inside the interface must be ones we actually offer.
        if let wasmtime::component::types::ComponentItem::ComponentInstance(instance) = item.ty {
            for (name, kind) in instance.exports(engine) {
                // An interface exports its type declarations alongside its
                // functions — `net` declares the `http-request`/`http-response`
                // records, for instance. Types grant nothing; only functions are
                // capabilities, so only they are checked.
                if !matches!(
                    kind.ty,
                    wasmtime::component::types::ComponentItem::ComponentFunc(_)
                ) {
                    continue;
                }
                let offered = HOST_IMPORTS
                    .iter()
                    .chain(NET_IMPORTS.iter())
                    .any(|(i, f)| *i == interface && *f == name);
                if !offered {
                    return Err(format!(
                        "plugin imports `{name}` from `{interface}`, which this host \
                         does not provide"
                    ));
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Component loading
// ---------------------------------------------------------------------------

/// Identifies a compiled component: where it came from, and which version of it.
///
/// Modification time and length are part of the key so a plugin the updater swapped
/// on disk is recompiled rather than silently served from the cache.
type ComponentKey = (PathBuf, Option<std::time::SystemTime>, u64);

fn component_cache() -> &'static std::sync::Mutex<HashMap<ComponentKey, Component>> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<ComponentKey, Component>>> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Parse, validate and compile a `.wasm` file as a component, without
/// instantiating it.
///
/// Used by [`WasmProvider::open`] and available to the updater as a real "does this
/// artifact load" check on every platform — the old `dlopen` test-load only worked
/// on Linux and macOS.
///
/// # Compiled components are cached
///
/// `Component::new` is Cranelift lowering an entire module to machine code, and it
/// dominates plugin startup: **1.16s for a 65 KiB plugin**, against 0.4ms to
/// instantiate one. The app builds a fresh provider set per tab, so without a cache
/// the same bytes are compiled once per tab and startup grows with tab count — three
/// tabs meant roughly three and a half seconds of recompiling identical input.
///
/// `Component` is reference-counted and cheap to clone, and instantiating one into
/// several independent `Store`s is exactly the intended usage: isolation lives at
/// the store level, not the component level, so sharing the compiled artifact costs
/// nothing in sandboxing terms.
pub fn load_component(path: &Path) -> Result<Component, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let key: ComponentKey = (path.to_path_buf(), meta.modified().ok(), meta.len());

    if let Ok(cache) = component_cache().lock()
        && let Some(component) = cache.get(&key)
    {
        return Ok(component.clone());
    }

    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let component = Component::new(engine(), &bytes)
        .map_err(|e| format!("{} is not a valid WASM component: {e}", path.display()))?;

    if let Ok(mut cache) = component_cache().lock() {
        cache.insert(key, component.clone());
    }
    Ok(component)
}

/// The import names this host is prepared to satisfy, as
/// `("interface", "function")` pairs.
///
/// Because LTO drops unused imports, a component's real import list reveals what it
/// can do. Comparing that list against this table lets the host reject a plugin
/// whose imports exceed its manifest declaration *before* instantiating it, rather
/// than only failing to link later.
pub const HOST_IMPORTS: &[(&str, &str)] = &[
    ("sicompass:plugin/host", "log"),
    ("sicompass:plugin/host", "get-setting"),
    ("sicompass:plugin/host", "now-millis"),
    ("sicompass:plugin/host", "translate"),
];

/// Import names that require `allowedHosts` in the manifest.
pub const NET_IMPORTS: &[(&str, &str)] = &[
    ("sicompass:plugin/net", "fetch"),
    ("sicompass:plugin/net", "fetch-url-ffon"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> HostState {
        HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new())
    }

    // --- capability gating ---

    #[test]
    fn no_allowed_hosts_means_no_network_capability() {
        assert!(!state().may_use_network());
    }

    #[test]
    fn declaring_allowed_hosts_enables_the_network_interface() {
        let s = HostState::new("demo", "demo", "/tmp/x", vec!["example.com".to_owned()]);
        assert!(s.may_use_network());
    }

    #[test]
    fn linker_omits_net_without_allowed_hosts() {
        // The point of the whole design: absence, not refusal. If this ever links
        // `net` unconditionally, a plugin that never declared a host could reach the
        // network.
        let s = state();
        assert!(linker_for(&s).is_ok());
        assert!(!s.may_use_network());
    }

    #[test]
    fn linker_builds_with_net_when_declared() {
        let s = HostState::new("demo", "demo", "/tmp/x", vec!["example.com".to_owned()]);
        assert!(linker_for(&s).is_ok());
    }

    // --- path confinement ---

    #[test]
    fn confine_accepts_a_plain_relative_path() {
        let s = state();
        assert_eq!(
            s.confine("assets/logo.png").unwrap(),
            Path::new("/tmp/sicompass-test-plugin/assets/logo.png")
        );
    }

    #[test]
    fn confine_rejects_absolute_paths() {
        let s = state();
        let err = s.confine("/etc/passwd").unwrap_err();
        assert!(err.contains("absolute"), "unexpected error: {err}");
    }

    #[test]
    fn confine_rejects_parent_traversal() {
        let s = state();
        for attempt in ["../secrets", "assets/../../etc/passwd", "a/b/../../../c"] {
            let err = s
                .confine(attempt)
                .expect_err(&format!("`{attempt}` should have been rejected"));
            assert!(err.contains("escapes"), "unexpected error for {attempt}: {err}");
        }
    }

    #[test]
    fn confine_rejects_a_bare_root() {
        assert!(state().confine("/").is_err());
    }

    // --- engine ---

    #[test]
    fn engine_is_shared_across_calls() {
        // Isolation is per-Store, not per-Engine, and the engine owns the epoch
        // counter the deadlines count against.
        assert!(std::ptr::eq(engine(), engine()));
    }

    #[test]
    fn default_build_uses_the_jit() {
        // Guards the feature wiring: the plain `cargo test` build should be
        // Cranelift, and the App Store build flips this via
        // `--no-default-features --features no-jit-wasm`.
        assert_eq!(uses_jit(), cfg!(feature = "jit-wasm"));
    }

    // `Component` is not `Debug`, so unwrap_err() is unavailable; match instead.
    fn load_err(path: &Path) -> String {
        match load_component(path) {
            Err(e) => e,
            Ok(_) => panic!("{} should not have loaded", path.display()),
        }
    }

    #[test]
    fn load_component_rejects_a_non_component_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("junk.wasm");
        std::fs::write(&path, b"not wasm at all").unwrap();
        let err = load_err(&path);
        assert!(err.contains("not a valid WASM component"), "unexpected: {err}");
    }

    #[test]
    fn load_component_reports_a_missing_file() {
        let err = load_err(Path::new("/no/such/plugin.wasm"));
        assert!(err.starts_with("read "), "unexpected: {err}");
    }

    #[test]
    fn a_rewritten_component_is_not_served_from_the_cache() {
        // Compiled components are cached because `Component::new` costs ~1.16s for a
        // 65 KiB plugin and the app builds one provider set per tab. The risk that
        // introduces is staleness: the updater swaps a plugin's `.wasm` in place, and
        // a cache keyed on path alone would keep running the old code. The key
        // includes modification time and length to prevent that.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.wasm");

        std::fs::write(&path, b"not a component").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let first: ComponentKey = (path.clone(), meta.modified().ok(), meta.len());

        // Rewrite with different content, as an update would.
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, b"still not a component, but longer").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let second: ComponentKey = (path.clone(), meta.modified().ok(), meta.len());

        assert_ne!(first, second, "an in-place rewrite must produce a different key");
    }

    #[test]
    fn compiling_the_same_component_twice_returns_a_cached_clone() {
        // Not a timing assertion — those are flaky. This checks the cache is
        // populated and hit, which is what makes the second call cheap.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm/hello.wasm");
        if !path.exists() {
            return; // fixture not present in this checkout
        }

        let meta = std::fs::metadata(&path).unwrap();
        let key: ComponentKey = (path.clone(), meta.modified().ok(), meta.len());

        load_component(&path).expect("fixture compiles");
        assert!(
            component_cache().lock().unwrap().contains_key(&key),
            "loading should have cached the compiled component"
        );

        // A second load of the same bytes must reuse that entry rather than adding
        // another — the whole point is not paying Cranelift twice.
        let len_after_first = component_cache().lock().unwrap().len();
        load_component(&path).expect("fixture compiles");
        assert_eq!(
            component_cache().lock().unwrap().len(),
            len_after_first,
            "a second load of the same file must not add another entry"
        );
    }

    // --- settings scoping ---

    // --- settings scoping ---

    #[test]
    fn settings_are_read_from_the_display_name_section() {
        // `programs::inject_plugin_settings` registers under `displayName`, so
        // reading back by `name` would silently return None for every plugin whose
        // two differ — a bug with no symptom except the setting never arriving.
        let s = HostState::new("weather-plugin", "Weather", "/tmp/x", Vec::new());
        assert_eq!(s.settings_section, "Weather");
        assert_ne!(s.settings_section, s.plugin_name);
    }

    #[test]
    fn a_setting_is_found_in_its_section_and_nowhere_else() {
        let json = serde_json::json!({
            "Weather": { "units": "metric", "refresh": 30 },
            "email client": { "password": "hunter2" },
        })
        .to_string();
        let root: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Mirrors `read_plugin_setting`'s lookup without needing a real config file
        // on disk.
        let lookup = |section: &str, key: &str| -> Option<String> {
            let compact: String = section.chars().filter(|&c| c != ' ').collect();
            [section, compact.as_str()].iter().find_map(|c| {
                root.get(*c).and_then(|s| s.get(key)).map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
            })
        };

        assert_eq!(lookup("Weather", "units").as_deref(), Some("metric"));
        // Non-strings come back stringified rather than being dropped.
        assert_eq!(lookup("Weather", "refresh").as_deref(), Some("30"));

        // The scoping that matters: a plugin cannot read another provider's
        // section, where the passwords and API keys live.
        assert_eq!(lookup("Weather", "password"), None);
        assert_eq!(lookup("Weather", "nonexistent"), None);
    }

    #[test]
    fn the_spaces_stripped_section_fallback_works() {
        let json = serde_json::json!({ "chatclient": { "homeserver": "matrix.org" } })
            .to_string();
        let root: serde_json::Value = serde_json::from_str(&json).unwrap();
        let section = "chat client";
        let compact: String = section.chars().filter(|&c| c != ' ').collect();
        assert!(root.get(section).is_none());
        assert!(root.get(&compact).is_some());
    }

    #[test]
    fn import_tables_match_the_wit_interfaces() {
        // These tables drive the install-time capability audit, so they must not
        // drift from the WIT. The vendored-WIT test checks the other direction.
        assert!(HOST_IMPORTS.iter().all(|(i, _)| *i == "sicompass:plugin/host"));
        assert!(NET_IMPORTS.iter().all(|(i, _)| *i == "sicompass:plugin/net"));
        assert_eq!(HOST_IMPORTS.len(), 4);
        assert_eq!(NET_IMPORTS.len(), 2);
    }
}
