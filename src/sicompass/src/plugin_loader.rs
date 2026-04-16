//! Re-exports `ScriptProvider`, `NativePlugin`, and `ProviderOpsC` from the SDK.
//!
//! These types were moved to `sicompass_sdk::plugin_loader` so that lib crates
//! (e.g. `lib_sales_demo`) can construct `ScriptProvider` without depending on
//! the app crate.  This shim keeps existing `crate::plugin_loader::*` imports
//! working during the transition.
pub use sicompass_sdk::plugin_loader::{NativePlugin, ProviderOpsC, ScriptProvider};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use sicompass_sdk::provider::Provider;

    // Smoke test: ScriptProvider is accessible through the re-export.
    #[test]
    fn script_provider_reexport_works() {
        let p = ScriptProvider::new("test", "Test", PathBuf::from("test.ts"));
        assert_eq!(p.name(), "test");
    }
}
