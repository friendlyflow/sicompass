//! Sales demo provider — thin Rust wrapper around `sales_demo.ts`.
//!
//! The actual provider logic lives in `lib/lib_sales_demo/sales_demo.ts` and
//! is executed via `bun run`.  This crate's job is to register the provider
//! with the SDK factory and manifest registries so the app can instantiate it
//! without importing this crate directly.

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the sales demo with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("sales demo", || {
        let script = sicompass_sdk::platform::resolve_repo_asset(
            "lib/lib_sales_demo/sales_demo.ts",
        );
        let p = sicompass_sdk::plugin_loader::ScriptProvider::new(
            "sales demo",
            "sales demo",
            script,
        )
        .with_supports_config_files(true);
        Box::new(p)
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("sales demo", "sales demo").with_settings(vec![
            sicompass_sdk::SettingDecl::text(
                "sales demo",
                "save folder (product configuration)",
                "saveFolder",
                "Downloads",
            ),
        ]),
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn register_does_not_panic() {
        // Double-registration is safe (the registry is append-only).
        super::register();
    }
}
