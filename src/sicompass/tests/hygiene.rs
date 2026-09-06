//! Drift guards for the test-isolation stubs that no compiler checks.
//!
//! The `TEST_NO_*` stubs are opt-in: a crate that grows a new side effect gets
//! no warning that it needs one, and a test that reaches the real thing still
//! passes — it just quietly writes to the developer's machine. That failure mode
//! is not hypothetical. `trash::delete` put roughly 38 fixtures per run into the
//! real `~/.local/share/Trash` for about a thousand runs, ending in a
//! 45 479-entry trash of which 37 850 were test files, and nothing failed.
//!
//! The OS trash escaped the six stubs that existed because it is not a directory
//! sicompass owns, so guarding sicompass's own paths could never have covered it.
//! These tests exist so the seventh miss is a build failure instead.
//!
//! Deliberately string-level, in the style of `packaging.rs`: pulling in a TOML
//! or syntax parser as a dev-dependency to grep for a function name would cost
//! more than it buys.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../")
}

/// Every `Cargo.toml` that could declare a dependency: the lib crates and the
/// app itself.
fn workspace_manifests() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut out = vec![root.join("src/sicompass/Cargo.toml")];
    for entry in std::fs::read_dir(root.join("lib"))
        .expect("lib/ should exist")
        .flatten()
    {
        let manifest = entry.path().join("Cargo.toml");
        if manifest.is_file() {
            out.push(manifest);
        }
    }
    out.sort();
    out
}

/// Every `.rs` file under `dir`, recursively.
fn rs_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn read_path(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// A crate that can move things to the OS trash must carry the stub that stops
/// it under test.
///
/// There is no way to notice the absence at runtime: the test still passes, it
/// just does it by putting the fixture in the developer's trash. So the check
/// has to be that the stub *exists* in any crate holding the capability.
#[test]
fn every_crate_that_can_trash_has_a_test_guard() {
    let mut checked = 0;
    for manifest in workspace_manifests() {
        let text = read_path(&manifest);
        let declares_trash = text.lines().any(|line| {
            let t = line.trim_start();
            !t.starts_with('#') && (t.starts_with("trash =") || t.starts_with("trash."))
        });
        if !declares_trash {
            continue;
        }
        checked += 1;

        let mut files = Vec::new();
        rs_files_under(&manifest.parent().unwrap().join("src"), &mut files);
        let src: String = files.iter().map(|f| read_path(f)).collect();

        assert!(
            src.contains("TEST_NO_TRASH") && src.contains("_set_test_no_trash"),
            "{} depends on the `trash` crate but has no TEST_NO_TRASH stub, so its \
             tests delete into the developer's real OS trash. Copy the block from \
             lib/lib_filebrowser/src/lib.rs, and call the setter from \
             `ensure_builtins()` in src/sicompass/tests/integration.rs.",
            manifest.display()
        );
    }

    assert_eq!(
        checked, 2,
        "expected exactly lib_filebrowser and lib_texteditor to depend on `trash`; \
         if a crate gained or lost the dependency, update this count deliberately"
    );
}

/// Every trash action goes through `os_trash_delete`, the one function the
/// `TEST_NO_TRASH` stub sits in front of.
///
/// `lib_texteditor`'s `redo` called `trash::delete` directly for a long time.
/// On macOS that silently took the slow Finder/osascript path the wrapper exists
/// to avoid, and once the stub landed it would have walked straight past it.
///
/// Known limits, so nobody trusts this further than it goes: it will not catch
/// `use trash::delete as td;` followed by a bare `td(p)`, nor a macro-generated
/// call. A determined bypass evades it; an accidental one does not, and
/// accidental is the failure mode that has actually happened.
#[test]
fn no_call_site_bypasses_the_trash_wrapper() {
    const ACTIONS: &[&str] = &[
        "trash::delete",
        "::trash::",
        "trash::TrashContext",
        "trash::os_limited",
        "TrashContextExtMacos",
    ];

    let root = workspace_root();
    let mut files = Vec::new();
    rs_files_under(&root.join("lib"), &mut files);
    rs_files_under(&root.join("src/sicompass/src"), &mut files);
    files.sort();

    let mut wrappers = 0;
    for file in files {
        let text = read_path(&file);
        // `os_trash_delete` is a top-level fn, so its body ends at the first
        // `}` in column zero. If it is ever moved inside an `impl`, this
        // over-restricts and fails loudly rather than passing silently.
        let mut in_wrapper = false;
        for (n, line) in text.lines().enumerate() {
            if line.starts_with("fn os_trash_delete(") {
                in_wrapper = true;
                wrappers += 1;
            } else if in_wrapper && line == "}" {
                in_wrapper = false;
            }
            if is_comment(line) {
                continue;
            }
            let names_trash = ACTIONS.iter().any(|a| line.contains(a))
                || line.trim_start().starts_with("use trash");
            assert!(
                !names_trash || in_wrapper,
                "{}:{} reaches the `trash` crate outside `os_trash_delete`: {line:?}\n\
                 Call `trash_delete` instead — that is what the TEST_NO_TRASH stub \
                 sits in front of.",
                file.strip_prefix(&root).unwrap_or(&file).display(),
                n + 1
            );
        }
    }

    // Two crates, each with a macOS and a non-macOS variant behind `#[cfg]`,
    // both of which this sees because it reads source rather than compiling it.
    assert_eq!(
        wrappers, 4,
        "expected 4 `os_trash_delete` definitions (2 crates x 2 platform variants)"
    );
}
