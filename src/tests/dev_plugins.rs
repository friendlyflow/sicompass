//! A debug build links the plugins built in the checkouts beside it
//! (`sicompass::dev_plugins`), so `cargo build` there is all it takes to run
//! them.

use std::path::Path;

use sicompass::dev_plugins::link_checkouts;

/// A checkout as the plugin repos lay one out: `plugin.json`, `locales/`, and,
/// when it has been built, `target/debug/<x>-plugin`.
fn checkout(siblings: &Path, prefix: &str, name: &str, built: bool) {
    let dir = siblings.join(format!("{prefix}-plugin-sicompass"));
    std::fs::create_dir_all(dir.join("locales")).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{ "name": "{name}", "displayName": "{name}", "type": "process",
                 "entry": "plugin", "version": "0.4.0", "minAppVersion": "0.1.0" }}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("locales").join("en-US.ftl"), "x = y").unwrap();
    if built {
        let debug = dir.join("target").join("debug");
        std::fs::create_dir_all(&debug).unwrap();
        std::fs::write(
            debug.join(format!("{prefix}-plugin{}", std::env::consts::EXE_SUFFIX)),
            "program",
        )
        .unwrap();
    }
}

#[test]
fn built_checkouts_are_laid_out_as_installs_and_the_rest_left_to_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let siblings = tmp.path().join("projects");
    checkout(&siblings, "projectmanagement", "projectmanagement", true);
    checkout(&siblings, "notes", "notes", false);
    std::fs::create_dir_all(siblings.join("sicompass")).unwrap();
    let out = tmp.path().join("dev-plugins");

    let names = link_checkouts(&siblings, &out).unwrap();
    assert_eq!(names, vec!["projectmanagement"]);

    let target = sicompass_sdk::plugin_abi::plugin_target().unwrap();
    let dir = out.join("projectmanagement");
    let program = dir.join(sicompass_sdk::plugin_abi::executable_name("plugin", target));
    assert_eq!(std::fs::read_to_string(program).unwrap(), "program");
    assert!(dir.join("plugin.json").is_file());
    assert_eq!(
        std::fs::read_to_string(dir.join("locales").join("en-US.ftl")).unwrap(),
        "x = y"
    );
    assert!(
        !out.join("notes").exists(),
        "no build, so the Store's copy runs"
    );

    // What the app reads finds it there.
    let found = sicompass_sdk::installed_plugins::discover_all_in(std::slice::from_ref(&out), None);
    assert!(
        found.iter().any(|(d, _, m)| *d == dir && m.is_ok()),
        "{found:?}"
    );
}

/// The folder is rebuilt at every start, so a checkout that is gone, or whose
/// build was cleaned, stops replacing the Store's copy.
#[test]
fn a_cleaned_checkout_drops_out_at_the_next_start() {
    let tmp = tempfile::tempdir().unwrap();
    let siblings = tmp.path().join("projects");
    checkout(&siblings, "notes", "notes", true);
    let out = tmp.path().join("dev-plugins");
    assert_eq!(link_checkouts(&siblings, &out).unwrap(), vec!["notes"]);

    std::fs::remove_dir_all(siblings.join("notes-plugin-sicompass").join("target")).unwrap();
    assert!(link_checkouts(&siblings, &out).unwrap().is_empty());
    assert!(!out.join("notes").exists());
    assert!(
        siblings
            .join("notes-plugin-sicompass")
            .join("plugin.json")
            .is_file(),
        "rebuilding the folder never touches a checkout"
    );
}
