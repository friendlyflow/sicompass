pub mod ffon;
pub mod manifest;
pub mod placeholders;
pub mod platform;
pub mod plugin_loader;
pub mod provider;
pub mod tags;
pub mod url_fetcher;

pub use ffon::{FfonElement, FfonObject, IdArray};
pub use manifest::{
    builtin_manifests, register_builtin_manifest, BuiltinManifest, SettingDecl, SettingKind,
};
pub use placeholders::{
    is_i_placeholder, new_obj_with_i_placeholder, seed_i_placeholders, I_PLACEHOLDER,
};
pub use provider::{
    create_provider_by_name, register_provider_factory, CoordinateKind, ListItem, Provider,
    SearchResultItem,
};
pub use url_fetcher::{fetch_url_to_ffon, register_url_fetcher};
