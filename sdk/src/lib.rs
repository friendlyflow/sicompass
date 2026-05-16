pub mod dashboard;
pub mod ffon;
pub mod fs_trash;
pub mod manifest;
pub mod placeholders;
pub mod platform;
pub mod plugin_loader;
pub mod provider;
pub mod tags;
pub mod timeline;
pub mod url_fetcher;

pub use dashboard::{
    CellAttrs, DashboardCell, DashboardFrame, DashboardKey, DashboardKeysym, DashboardKind,
    DashboardRequest,
};
pub use ffon::{FfonElement, FfonObject, IdArray};
pub use fs_trash::{
    restore_from_os_trash, restore_side_effect, restore_trashed_tree, snapshot_for_delete,
    TRASH_SNAPSHOT_LIMIT_BYTES,
};
pub use manifest::{
    builtin_manifests, register_builtin_manifest, BuiltinManifest, SettingDecl, SettingKind,
};
pub use placeholders::{
    is_ci_placeholder, is_i_placeholder, new_obj_with_i_placeholder, seed_i_placeholders,
    CI_PLACEHOLDER, I_PLACEHOLDER,
};
pub use provider::{
    create_provider_by_name, register_provider_factory, ListItem, Provider, SearchResultItem,
};
pub use timeline::{
    ChatOpKind, FsOpKind, FsSideEffect, ImapOpKind, NavKind, StructuralOp, StructuralPayload,
    TimelineEntry, TrashedTree,
};
pub use url_fetcher::{fetch_url_to_ffon, register_url_fetcher};
