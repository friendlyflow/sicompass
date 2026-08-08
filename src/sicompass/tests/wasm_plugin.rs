//! End-to-end tests for the WASM plugin host, against a real component.
//!
//! The unit tests in `wasm_host` cover pure logic. These instantiate an actual
//! guest and drive it through the `Provider` trait, which is the only way to check
//! the parts that matter: that the capability model holds, and that a misbehaving
//! guest cannot take the host down.
//!
//! # The fixture
//!
//! `tests/fixtures/wasm/hello.wasm` is `examples/hello-plugin` from the
//! sicompass-plugin-sdk repo, built for `wasm32-unknown-unknown` and componentized:
//!
//! ```text
//! cd ../sicompass-plugin-sdk/examples/hello-plugin
//! cargo build --release --target wasm32-unknown-unknown
//! wasm-tools component new target/wasm32-unknown-unknown/release/hello_plugin.wasm \
//!     -o plugin.wasm
//! cp plugin.wasm <this repo>/src/sicompass/tests/fixtures/wasm/hello.wasm
//! ```
//!
//! It is committed rather than built here on purpose: building it would need the
//! wasm target and `wasm-tools` present for every `cargo test` run, including CI
//! jobs that have no business compiling guests. `wit_vendor_matches_host_tables`
//! below catches the drift that committing a binary would otherwise risk.

use std::path::{Path, PathBuf};

use sicompass::wasm_host::{self, WasmProvider};
use sicompass_sdk::{FfonElement, Provider, TimelineEntry};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm")
}

fn hello_wasm() -> PathBuf {
    fixture_dir().join("hello.wasm")
}

/// Instantiate the fixture with no network capability (the default for a plugin
/// that declares no `allowedHosts`).
fn open_hello() -> WasmProvider {
    WasmProvider::open(&hello_wasm(), "hello", "hello", &fixture_dir(), Vec::new())
        .expect("the hello fixture should load and instantiate")
}

// ---------------------------------------------------------------------------
// Loading and identity
// ---------------------------------------------------------------------------

#[test]
fn a_real_component_loads_and_reports_its_identity() {
    let p = open_hello();
    assert_eq!(p.name(), "hello");
    // `display_name` comes from `describe()`, which the guest builds by calling the
    // host's `translate` import — so a non-empty answer also proves a host function
    // was reachable from inside the guest.
    assert!(!p.display_name().is_empty());

    // The guest reports its own crate version, not the host's. Checked loosely so
    // bumping the fixture does not break the test, but strictly enough to catch the
    // descriptor arriving empty.
    let version = p.version().expect("the fixture reports a version");
    assert!(
        version.split('.').count() >= 2
            && version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "implausible version from the guest: {version:?}"
    );

    assert!(!p.is_poisoned());
}

#[test]
fn fetch_returns_a_decodable_ffon_tree() {
    let mut p = open_hello();
    let elems = p.fetch();
    assert!(!elems.is_empty(), "guest returned no elements");

    // The guest builds one Obj with several children; if the binary codec were
    // mismatched across the boundary this would come back as garbage or empty.
    let obj = elems[0].as_obj().expect("first element should be an Obj");
    assert!(
        obj.children
            .iter()
            .any(|c| c.as_str().is_some_and(|s| s.contains("current path"))),
        "expected a `current path` line, got {:?}",
        obj.children
    );
}

#[test]
fn the_guest_can_reach_the_host_clock() {
    // `now_millis` is a host import; `SystemTime::now()` inside a guest compiles and
    // then fails, so a plausible timestamp proves the import path works.
    let mut p = open_hello();
    let elems = p.fetch();
    let obj = elems[0].as_obj().unwrap();
    let clock = obj
        .children
        .iter()
        .find_map(|c| c.as_str().filter(|s| s.starts_with("host clock: ")))
        .expect("no host clock line");
    let millis: u64 = clock.trim_start_matches("host clock: ").parse().unwrap();
    // Sometime after 2020, i.e. a real clock rather than zero.
    assert!(millis > 1_577_836_800_000, "implausible timestamp {millis}");
}

// ---------------------------------------------------------------------------
// The capability model
// ---------------------------------------------------------------------------

#[test]
fn a_plugin_without_allowed_hosts_gets_no_network_import() {
    // The fixture never calls `fetch`, so LTO dropped the import entirely and it
    // instantiates fine with the `net` interface unlinked. This is the common case:
    // most plugins need no network and structurally cannot reach one.
    let p = WasmProvider::open(&hello_wasm(), "hello", "hello", &fixture_dir(), Vec::new());
    assert!(
        p.is_ok(),
        "should instantiate without network: {:?}",
        p.err()
    );
}

#[test]
fn declaring_allowed_hosts_still_instantiates() {
    let p = WasmProvider::open(
        &hello_wasm(),
        "hello",
        "hello",
        &fixture_dir(),
        vec!["example.com".to_owned()],
    );
    assert!(
        p.is_ok(),
        "should instantiate with network linked: {:?}",
        p.err()
    );
}

#[test]
fn the_component_imports_nothing_outside_the_host_capability_set() {
    // Because LTO drops unused imports, a component's real import list reveals what
    // it can do. This is the check the host will run at install time to reject a
    // plugin whose imports exceed its manifest declaration.
    // Decode the component's WIT-level world rather than scraping printed text: the
    // printed form is full of canonical-ABI plumbing (`(import "" "0" ...)`,
    // `import-func-*` lowerings) that is internal wiring, not capability.
    let bytes = std::fs::read(hello_wasm()).unwrap();
    let decoded = wit_component::decode(&bytes).expect("fixture should decode as a component");
    let (resolve, world) = match &decoded {
        wit_component::DecodedWasm::Component(resolve, world) => (resolve, *world),
        wit_component::DecodedWasm::WitPackage(..) => {
            panic!("fixture is a WIT package, not a component")
        }
    };

    let allowed: Vec<&str> = wasm_host::HOST_IMPORTS
        .iter()
        .chain(wasm_host::NET_IMPORTS.iter())
        .map(|(_, f)| *f)
        .collect();

    let mut saw_a_function_import = false;

    for (_, item) in &resolve.worlds[world].imports {
        let wit_parser::WorldItem::Interface { id, .. } = *item else {
            continue;
        };
        let iface = resolve.id_of(id).unwrap_or_default();

        // The guest must never import WASI. Targeting wasm32-unknown-unknown rather
        // than wasip2 is what keeps this true.
        assert!(
            !iface.contains("wasi:"),
            "component imports {iface}: WASI would void the capability model"
        );
        assert!(
            iface.starts_with("sicompass:plugin/"),
            "component imports an unexpected interface: {iface}"
        );

        for func in resolve.interfaces[id].functions.keys() {
            saw_a_function_import = true;
            assert!(
                allowed.contains(&func.as_str()),
                "component imports `{func}` from {iface}, which the host does not \
                 offer (allowed: {allowed:?})"
            );
        }
    }

    // Guard against the walk silently matching nothing and the test passing
    // vacuously — the whole check would then be worthless.
    assert!(
        saw_a_function_import,
        "no function imports found; the walk is wrong"
    );
}

// ---------------------------------------------------------------------------
// Trap containment — a misbehaving guest must not take the host down
// ---------------------------------------------------------------------------

#[test]
fn an_infinite_loop_in_a_guest_is_stopped() {
    let mut p = open_hello();

    // `spin` loops forever. Reaching the next line at all is the assertion: without
    // a cap this test would hang the suite.
    let ok = p.execute_command("spin", "");
    assert!(!ok, "a trapped call must not report success");

    assert!(p.is_poisoned(), "a runaway plugin must be disabled");
    let err = p
        .take_error()
        .expect("the trap should surface as an error row");
    assert!(err.contains("hello"), "error should name the plugin: {err}");

    // Deliberately not asserting *which* limit fired. Both are real, and which one
    // wins depends on the backend: under Cranelift the guest burns the fuel budget
    // in well under the wall-clock deadline, while under Pulley (the no-JIT App
    // Store build) the interpreter is slow enough that the 10s epoch deadline
    // arrives first. Pinning one mechanism would make this test fail on a
    // configuration where containment is working perfectly well.
    assert!(
        err.contains("too much CPU") || err.contains("took too long"),
        "error should explain the cause: {err}"
    );
    assert!(err.contains("disabled"), "{err}");
}

#[test]
fn a_guest_panic_is_contained_and_disables_the_plugin() {
    let mut p = open_hello();
    assert!(!p.execute_command("explode", ""));
    assert!(p.is_poisoned());

    let err = p.take_error().expect("panic should surface as an error");
    assert!(err.contains("hello"), "{err}");
    assert!(err.contains("disabled"), "{err}");
}

#[test]
fn a_poisoned_plugin_stays_inert_instead_of_returning_garbage() {
    // After a trap the guest's linear memory is in an arbitrary state, so every
    // later call must be skipped rather than produce plausible-looking nonsense.
    let mut p = open_hello();
    p.execute_command("explode", "");
    assert!(p.is_poisoned());
    let _ = p.take_error();

    assert!(
        p.fetch().is_empty(),
        "a poisoned plugin must not return data"
    );
    assert!(p.commands().is_empty());
    assert!(!p.commit_edit("a", "b"));
    assert!(!p.create_file("x"));

    // The failure is reported once, not once per call. `tick` runs every frame, so
    // re-queueing on each skipped call would push a fresh error row ~60 times a
    // second and bury whatever the user was reading.
    p.tick();
    p.tick();
    assert!(
        p.take_error().is_none(),
        "a disabled plugin must stay quiet after its failure has been reported"
    );
}

#[test]
fn one_plugin_trapping_does_not_affect_another_instance() {
    // Isolation is per-Store. Two instances of the same component share an Engine,
    // so a trap must not leak across.
    let mut doomed = open_hello();
    let mut healthy = open_hello();

    doomed.execute_command("explode", "");
    assert!(doomed.is_poisoned());

    assert!(!healthy.is_poisoned());
    assert!(
        !healthy.fetch().is_empty(),
        "the healthy instance still works"
    );
    assert!(healthy.take_error().is_none());
}

// ---------------------------------------------------------------------------
// Navigation, commands, timeline
// ---------------------------------------------------------------------------

#[test]
fn path_navigation_stays_in_step_with_the_guest() {
    // Path mutators return the resulting path so the host can cache it; if that
    // caching drifted, `current_path` would lie.
    let mut p = open_hello();
    assert_eq!(p.current_path(), "/");

    p.push_path("alpha");
    assert_eq!(p.current_path(), "/alpha");
    p.push_path("beta");
    assert_eq!(p.current_path(), "/alpha/beta");

    p.pop_path();
    assert_eq!(p.current_path(), "/alpha");
    p.pop_path();
    assert_eq!(p.current_path(), "/");

    p.set_current_path("/gamma");
    assert_eq!(p.current_path(), "/gamma");

    // And the guest agrees, rather than the host merely believing its own cache.
    let elems = p.fetch();
    let obj = elems[0].as_obj().unwrap();
    assert!(
        obj.children
            .iter()
            .any(|c| c.as_str().is_some_and(|s| s == "current path: /gamma")),
        "guest disagrees about the path: {:?}",
        obj.children
    );
}

#[test]
fn commands_and_labels_come_from_the_guest() {
    let p = open_hello();
    let cmds = p.commands();
    assert!(cmds.contains(&"greet".to_owned()), "got {cmds:?}");

    // `command_label` is a `&self` method that still reaches the guest, which is
    // what the RefCell in WasmProvider is for.
    assert!(!p.command_label("greet").is_empty());
    assert_eq!(p.command_label("no-such-command"), "no-such-command");
}

#[test]
fn command_list_items_cross_the_boundary() {
    let p = open_hello();
    let items = p.command_list_items("greet");
    assert_eq!(items.len(), 2, "got {items:?}");
    assert!(
        items
            .iter()
            .any(|i| i.label == "world" && i.data == "world")
    );
}

#[test]
fn executing_a_command_emits_an_undoable_timeline_entry() {
    let mut p = open_hello();
    assert!(p.execute_command("greet", "world"));

    let entries = p.take_timeline_entries();
    assert_eq!(entries.len(), 1, "got {entries:?}");

    match &entries[0] {
        TimelineEntry::ProviderOp {
            command,
            payload,
            label,
            ..
        } => {
            assert_eq!(command, "greet");
            assert_eq!(label, "greet world");
            assert_eq!(payload, &FfonElement::new_str("world"));
        }
        other => panic!("a guest may only emit ProviderOp, got {other:?}"),
    }

    // Draining is destructive, as the trait requires.
    assert!(p.take_timeline_entries().is_empty());
}

#[test]
fn undo_and_redo_reach_the_guest_and_report_refusals() {
    let mut p = open_hello();
    p.execute_command("greet", "world");
    let entry = p.take_timeline_entries().pop().unwrap();

    let mut error = String::new();
    sicompass_sdk::block_on(p.undo(&entry, &mut error));
    assert!(error.is_empty(), "undo should succeed: {error}");

    sicompass_sdk::block_on(p.redo(&entry, &mut error));
    assert!(error.is_empty(), "redo should succeed: {error}");

    // A command the guest does not recognise comes back as a domain error, not a
    // trap, and leaves the plugin usable.
    let bogus = TimelineEntry::ProviderOp {
        provider_idx: 0,
        command: "not-a-command".to_owned(),
        payload: FfonElement::new_str(""),
        label: String::new(),
    };
    sicompass_sdk::block_on(p.undo(&bogus, &mut error));
    assert!(!error.is_empty(), "the guest should have refused");
    assert!(!p.is_poisoned(), "a refusal is not a trap");
}

#[test]
fn poll_is_the_single_per_frame_crossing() {
    // `tick` performs the one `poll()` call and the other four read its cache, so
    // they must reflect what the guest reported without further crossings.
    let mut p = open_hello();
    p.tick();
    assert!(p.at_root());
    assert!(!p.is_busy());
    assert!(!p.needs_refresh());
    assert!(p.take_navigation_request().is_none());

    p.push_path("deep");
    p.tick();
    assert!(!p.at_root(), "guest should report leaving the root");
}

// ---------------------------------------------------------------------------
// The interactive dashboard
// ---------------------------------------------------------------------------

/// Read a row out of a frame as a string, for asserting on rendered text.
fn row_text(frame: &sicompass_sdk::DashboardFrame, row: u16) -> String {
    let start = row as usize * frame.cols as usize;
    frame.cells[start..start + frame.cols as usize]
        .iter()
        .map(|c| c.ch)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------
//
// A plugin ships its own files in `<plugin_dir>/assets/`. Two ways in, both
// confined to that directory: the guest reads bytes with the `read-asset` import,
// and the *host* resolves `asset:<plugin-name>/<file>` when it has to render or
// open something itself.

/// The fixture directory doubles as a plugin directory, so
/// `tests/fixtures/wasm/assets/hello-asset.txt` is the hello plugin's own asset.
fn hello_asset_bytes() -> Vec<u8> {
    std::fs::read(fixture_dir().join("assets/hello-asset.txt"))
        .expect("the fixture asset should be committed")
}

#[test]
fn a_plugin_directory_asset_is_reachable_as_an_asset_uri() {
    // Registered under a test-only name so this does not collide with the resolver
    // `WasmProvider::from_component` installs for `hello` elsewhere in this binary.
    wasm_host::register_plugin_assets("__uri_test", &fixture_dir());
    let bytes = sicompass_sdk::assets::resolve("asset:__uri_test/hello-asset.txt")
        .expect("the plugin's own asset should resolve");
    assert_eq!(bytes.as_ref(), hello_asset_bytes().as_slice());
}

#[test]
fn an_asset_uri_cannot_escape_the_plugin_directory() {
    // The end-to-end counterpart to the unit tests in `wasm_host`: a real directory
    // on disk, with real files just outside it.
    wasm_host::register_plugin_assets("__escape_test", &fixture_dir());
    for attempt in [
        "../../../Cargo.toml",
        "/etc/passwd",
        "a/../../hello.wasm",
        "../hello.wasm",
    ] {
        assert!(
            sicompass_sdk::assets::resolve(&format!("asset:__escape_test/{attempt}")).is_none(),
            "`{attempt}` should not have resolved"
        );
    }
}

#[test]
fn a_guest_reads_its_own_asset_through_the_host() {
    // The guest calls `read_asset("hello-asset.txt")` in `fetch` and reports the byte
    // count, so this exercises the import all the way into guest memory.
    let mut p = open_hello();
    let elems = p.fetch();
    let obj = elems[0].as_obj().unwrap();
    let line = obj
        .children
        .iter()
        .find_map(|c| c.as_str().filter(|s| s.starts_with("asset bytes: ")))
        .expect("no asset line — is the fixture rebuilt?");
    assert_eq!(
        line,
        format!("asset bytes: {}", hello_asset_bytes().len()),
        "the guest should have received the whole file"
    );
}

#[test]
fn a_guest_cannot_read_outside_its_own_asset_directory() {
    // The guest also asks for `../../Cargo.toml` and a file that does not exist. Both
    // must come back as `none`, and indistinguishably so: a guest that could tell
    // "refused" from "absent" would have a filesystem probe.
    let mut p = open_hello();
    let elems = p.fetch();
    let obj = elems[0].as_obj().unwrap();
    let rows: Vec<&str> = obj.children.iter().filter_map(|c| c.as_str()).collect();

    assert!(
        rows.contains(&"escape: refused"),
        "the guest read outside its asset dir: {rows:?}"
    );
    assert!(
        rows.contains(&"missing: refused"),
        "a missing asset did not come back as none: {rows:?}"
    );
    assert!(
        !rows.iter().any(|r| r.contains("LEAKED")),
        "confinement failed: {rows:?}"
    );
}

#[test]
fn the_fixture_opts_into_an_interactive_dashboard() {
    let p = open_hello();
    assert_eq!(
        p.dashboard_kind(),
        sicompass_sdk::DashboardKind::Interactive
    );
    assert!(
        p.manual_dashboard_entry_allowed(),
        "pressing `d` should work"
    );
    // Interactive and Image are mutually exclusive; no static image here.
    assert!(p.dashboard_image_path().is_none());
}

#[test]
fn dashboard_render_returns_a_frame_matching_the_requested_grid() {
    let mut p = open_hello();
    p.enter_dashboard();

    let frame = p.dashboard_render(60, 10);
    // The size the host asked for, not whatever the guest felt like — a mismatch
    // would panic the renderer, so it is replaced with blanks instead.
    assert_eq!(frame.cols, 60);
    assert_eq!(frame.rows, 10);
    assert_eq!(frame.cells.len(), 600);

    assert!(
        row_text(&frame, 0).contains("hello, from inside the sandbox"),
        "row 0 was {:?}",
        row_text(&frame, 0)
    );
    assert!(
        row_text(&frame, 1).contains("60x10"),
        "guest should see the grid size"
    );
    assert!(
        p.take_error().is_none(),
        "a well-behaved frame should raise nothing"
    );
}

#[test]
fn the_guest_sees_frames_advance_and_the_cursor_is_carried_across() {
    let mut p = open_hello();
    p.enter_dashboard();

    let first = p.dashboard_render(40, 8);
    assert!(
        row_text(&first, 2).contains("frames rendered: 1"),
        "{:?}",
        row_text(&first, 2)
    );

    let second = p.dashboard_render(40, 8);
    assert!(
        row_text(&second, 2).contains("frames rendered: 2"),
        "{:?}",
        row_text(&second, 2)
    );

    // The guest parks the cursor where typed text lands, so focus is somewhere
    // deliberate rather than wherever the renderer defaults to.
    assert_eq!(second.cursor, Some((0, 5)));
}

#[test]
fn typed_text_reaches_the_guest_and_shows_up_in_the_next_frame() {
    let mut p = open_hello();
    p.enter_dashboard();

    p.dashboard_text("abc");
    let frame = p.dashboard_render(60, 8);
    assert!(
        row_text(&frame, 5).contains("last input: abc"),
        "{:?}",
        row_text(&frame, 5)
    );

    // Backspace is a non-printable key, so it arrives through `dashboard_key`.
    let consumed = p.dashboard_key(sicompass_sdk::DashboardKey {
        keysym: sicompass_sdk::DashboardKeysym::Backspace,
        ctrl: false,
        shift: false,
        alt: false,
    });
    assert!(consumed, "the guest should consume Backspace");

    let frame = p.dashboard_render(60, 8);
    assert!(
        row_text(&frame, 5).contains("last input: ab"),
        "{:?}",
        row_text(&frame, 5)
    );
}

#[test]
fn a_key_that_changes_nothing_asks_for_no_redraw() {
    // The bool is a *redraw request*, not "I consumed this". While an interactive
    // dashboard is open the host forwards every key to the provider regardless and
    // uses the answer only to decide whether to repaint — see `shortcuts.rs`, where
    // the interactive branch returns unconditionally. That is why leaving is the
    // host's double-Ctrl+C and not a key a plugin can claim: a terminal emulator has
    // to be able to receive Escape.
    let mut p = open_hello();
    p.enter_dashboard();
    let redraw = p.dashboard_key(sicompass_sdk::DashboardKey {
        keysym: sicompass_sdk::DashboardKeysym::Escape,
        ctrl: false,
        shift: false,
        alt: false,
    });
    assert!(!redraw, "Escape changes nothing on screen for this plugin");
}

#[test]
fn a_paste_is_delivered_distinctly_from_typed_text() {
    let mut p = open_hello();
    p.enter_dashboard();
    p.dashboard_paste("pasted");
    let frame = p.dashboard_render(60, 8);
    assert!(
        row_text(&frame, 5).contains("pasted"),
        "{:?}",
        row_text(&frame, 5)
    );
}

#[test]
fn resize_is_forwarded_and_the_next_frame_uses_the_new_size() {
    let mut p = open_hello();
    p.enter_dashboard();
    p.dashboard_resize(12, 30);

    let frame = p.dashboard_render(30, 12);
    assert_eq!((frame.cols, frame.rows), (30, 12));
    assert!(
        row_text(&frame, 1).contains("30x12"),
        "{:?}",
        row_text(&frame, 1)
    );
}

#[test]
fn entering_the_dashboard_resets_the_guests_frame_counter() {
    let mut p = open_hello();
    p.enter_dashboard();
    p.dashboard_render(20, 6);
    p.dashboard_render(20, 6);
    p.leave_dashboard();

    p.enter_dashboard();
    let frame = p.dashboard_render(20, 6);
    assert!(
        row_text(&frame, 2).contains("frames rendered: 1"),
        "enter should have reset the counter, got {:?}",
        row_text(&frame, 2)
    );
}

#[test]
fn a_poisoned_plugin_renders_blanks_rather_than_panicking() {
    let mut p = open_hello();
    p.execute_command("explode", "");
    assert!(p.is_poisoned());
    let _ = p.take_error();

    // The app calls `dashboard_render` every frame; after a trap it must keep
    // returning a well-formed frame at the requested size or the renderer indexes
    // off the end of a short buffer.
    let frame = p.dashboard_render(20, 5);
    assert_eq!((frame.cols, frame.rows), (20, 5));
    assert_eq!(frame.cells.len(), 100);
    assert!(frame.cells.iter().all(|c| c.ch == ' '));
}

/// Where startup time goes when a WASM plugin is installed.
///
/// Ignored by default; run deliberately:
///
/// ```text
/// cargo test -p sicompass --test wasm_plugin -- --ignored --nocapture startup
/// ```
#[test]
#[ignore = "manual profiling aid; prints timings instead of asserting"]
fn profile_startup_cost() {
    let bytes = std::fs::read(hello_wasm()).unwrap();

    // Engine construction is one-off and lazy; charge it explicitly rather than
    // letting it hide inside whichever measurement runs first.
    let t = std::time::Instant::now();
    let _ = wasm_host::engine();
    println!("\n  engine init            {:>10.2?}", t.elapsed());

    // Compiling the component is the expensive part: Cranelift lowers the whole
    // module to machine code.
    let t = std::time::Instant::now();
    let component = wasm_host::load_component(&hello_wasm()).expect("fixture compiles");
    let compile = t.elapsed();
    println!(
        "  load_component         {:>10.2?}   ({} KiB of wasm)",
        compile,
        bytes.len() / 1024
    );

    // Instantiating an already-compiled component, plus init + describe.
    let mut instantiate = std::time::Duration::ZERO;
    const N: u32 = 5;
    for _ in 0..N {
        let t = std::time::Instant::now();
        let p =
            WasmProvider::from_component(&component, "hello", "hello", &fixture_dir(), Vec::new())
                .expect("instantiates");
        instantiate += t.elapsed();
        std::hint::black_box(&p);
    }
    println!("  instantiate (avg of {N}) {:>9.2?}", instantiate / N);

    // What the app actually does today, end to end, per provider.
    let t = std::time::Instant::now();
    let _ = WasmProvider::open(&hello_wasm(), "hello", "hello", &fixture_dir(), Vec::new());
    println!("  open() = compile+inst  {:>10.2?}", t.elapsed());

    println!(
        "\n  Two caches sit behind these numbers. `load_component` keeps compiled\n  \
         components in-process, so the app's per-tab provider sets do not each pay\n  \
         to compile the same bytes; and wasmtime's on-disk cache carries the\n  \
         compile across runs. With a cold on-disk cache expect `load_component`\n  \
         near 1.2s for this fixture rather than the few ms shown here — delete\n  \
         ~/.cache/wasmtime to see it.\n"
    );
}

/// Rough per-frame cost of crossing the boundary, printed rather than asserted.
///
/// Ignored by default: timing assertions are flaky under load and in CI. Run it
/// deliberately, on both backends, to decide whether the dashboard is affordable:
///
/// ```text
/// cargo test -p sicompass --test wasm_plugin -- --ignored --nocapture profile
/// cargo test -p sicompass --no-default-features --features no-jit-wasm \
///     --test wasm_plugin -- --ignored --nocapture profile
/// ```
#[test]
#[ignore = "manual profiling aid; prints timings instead of asserting"]
fn profile_dashboard_render_cost() {
    let mut p = open_hello();
    p.enter_dashboard();

    const FRAMES: u32 = 600; // ~10s of wall clock at 60fps

    let backend = if wasm_host::uses_jit() {
        "cranelift (jit)"
    } else {
        "pulley (no-jit)"
    };
    println!("\ndashboard_render on {backend}\n");

    // Sweep grid sizes. If cost tracks cell count the expense is per-cell — guest
    // construction plus Canonical-ABI lifting of 1920 records — and a flatter wire
    // representation would help. If it is mostly fixed, the call itself is the cost
    // and only calling less often helps.
    for (cols, rows) in [(20u16, 6u16), (40, 12), (80, 24), (160, 48)] {
        let cells = cols as usize * rows as usize;

        // Warm up: the first calls pay for lazy init inside the guest.
        for _ in 0..20 {
            p.dashboard_render(cols, rows);
        }

        let started = std::time::Instant::now();
        for _ in 0..FRAMES {
            std::hint::black_box(&p.dashboard_render(cols, rows));
        }
        let per_frame = started.elapsed() / FRAMES;

        println!(
            "  {cols:>3}x{rows:<3} ({cells:>5} cells)  {per_frame:>12.3?}/frame  \
             {:>8.3}µs/cell  {:>7.1}% of a 60fps frame",
            per_frame.as_secs_f64() * 1e6 / cells as f64,
            per_frame.as_secs_f64() / 0.016_667 * 100.0
        );
    }

    // How much of that is the crossing itself rather than the payload? `poll`
    // returns a small fixed record, so it isolates per-call overhead.
    for _ in 0..20 {
        p.tick();
    }
    let started = std::time::Instant::now();
    for _ in 0..FRAMES {
        std::hint::black_box(p.tick());
    }
    println!(
        "\n  poll() (small payload)     {:>12.3?}/call",
        started.elapsed() / FRAMES
    );
    println!(
        "\n  60fps budget is 16.67ms per frame for *everything* — Vulkan, text \
         shaping and AccessKit included.\n"
    );
}

// ---------------------------------------------------------------------------
// Drift guards
// ---------------------------------------------------------------------------

#[test]
fn wit_vendor_matches_host_tables() {
    // `src/sicompass/wit/` is a vendored copy of the canonical file in the
    // sicompass-plugin-sdk repo. The host's HOST_IMPORTS/NET_IMPORTS tables drive
    // the install-time capability audit, so they must not drift from it.
    let wit = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/sicompass-plugin.wit"),
    )
    .expect("vendored WIT should exist");

    let mut resolve = wit_parser::Resolve::default();
    let pkg = resolve
        .push_str("sicompass-plugin.wit", &wit)
        .expect("vendored WIT must parse");
    let world = resolve
        .select_world(&[pkg], Some("plugin"))
        .expect("vendored WIT must define the `plugin` world");

    let mut found: Vec<(String, String)> = Vec::new();
    for (_, item) in &resolve.worlds[world].imports {
        if let wit_parser::WorldItem::Interface { id, .. } = *item {
            let iface = resolve.id_of(id).unwrap_or_default();
            // Strip the @version suffix; the tables are version-agnostic.
            let iface = iface.split('@').next().unwrap_or(&iface).to_owned();
            for name in resolve.interfaces[id].functions.keys() {
                found.push((iface.clone(), name.clone()));
            }
        }
    }
    found.sort();

    let mut expected: Vec<(String, String)> = wasm_host::HOST_IMPORTS
        .iter()
        .chain(wasm_host::NET_IMPORTS.iter())
        .map(|(i, f)| (i.to_string(), f.to_string()))
        .collect();
    expected.sort();

    assert_eq!(
        found, expected,
        "the vendored WIT and the host's import tables disagree — one of them was \
         edited without the other"
    );
}

// ---------------------------------------------------------------------------
// The capability audit, against a plugin that really does use the network
// ---------------------------------------------------------------------------
//
// `net.wasm` is `examples/net-plugin`: it calls `net::fetch`, so LTO keeps the
// import. Regenerate it the same way as `hello.wasm`.

fn net_wasm() -> PathBuf {
    fixture_dir().join("net.wasm")
}

#[test]
fn a_network_using_plugin_is_refused_when_it_declares_no_allowed_hosts() {
    // The central security check. A plugin that reaches the network without saying
    // so in its manifest gives the user no chance to see the capability before
    // enabling it, so it must not load at all.
    // `WasmProvider` is not `Debug` (it owns a wasmtime `Store`), so match rather
    // than `expect_err`.
    let err = match WasmProvider::open(
        &net_wasm(),
        "net-demo",
        "net demo",
        &fixture_dir(),
        Vec::new(),
    ) {
        Err(e) => e,
        Ok(_) => panic!("a net-importing plugin with no allowedHosts must be refused"),
    };

    assert!(
        err.contains("allowedHosts"),
        "the error should say what is missing: {err}"
    );
    assert!(err.contains("network"), "{err}");
}

#[test]
fn the_same_plugin_loads_once_it_declares_its_hosts() {
    // The refusal above must be about the missing declaration, not about the plugin
    // being broken — otherwise the test above would pass for the wrong reason.
    let p = WasmProvider::open(
        &net_wasm(),
        "net-demo",
        "net demo",
        &fixture_dir(),
        vec!["example.com".to_owned()],
    );
    assert!(
        p.is_ok(),
        "should load once allowedHosts is declared: {:?}",
        p.err()
    );
}

#[test]
fn the_two_fixtures_carry_different_capabilities() {
    // Proves the import list actually tracks what a plugin does, rather than every
    // component carrying the same boilerplate: `hello` never calls the network and
    // `net-plugin` never logs, and their imports differ accordingly. That difference
    // is what the audit reads.
    let hello = std::fs::read(hello_wasm()).unwrap();
    let net = std::fs::read(net_wasm()).unwrap();

    let interfaces = |bytes: &[u8]| -> Vec<String> {
        let decoded = wit_component::decode(bytes).expect("component");
        let (resolve, world) = match &decoded {
            wit_component::DecodedWasm::Component(r, w) => (r, *w),
            _ => panic!("not a component"),
        };
        let mut out: Vec<String> = resolve.worlds[world]
            .imports
            .values()
            .filter_map(|item| match *item {
                wit_parser::WorldItem::Interface { id, .. } => resolve
                    .id_of(id)
                    .map(|n| n.split('@').next().unwrap_or(&n).to_owned()),
                _ => None,
            })
            .collect();
        out.sort();
        out
    };

    let hello_ifaces = interfaces(&hello);
    let net_ifaces = interfaces(&net);

    assert!(
        !hello_ifaces.iter().any(|i| i == "sicompass:plugin/net"),
        "hello should carry no network capability, got {hello_ifaces:?}"
    );
    assert!(
        net_ifaces.iter().any(|i| i == "sicompass:plugin/net"),
        "net-plugin should carry the network capability, got {net_ifaces:?}"
    );
}

#[test]
fn network_functions_live_in_their_own_interface() {
    // wasmtime links host functions an interface at a time, so `fetch` sharing an
    // interface with `log` would make "link logging" and "link networking" the same
    // decision, silently destroying the conditional-capability property.
    assert!(
        wasm_host::HOST_IMPORTS.iter().all(|(_, f)| *f != "fetch"),
        "`fetch` must not be in the always-linked interface"
    );
    assert!(wasm_host::NET_IMPORTS.iter().any(|(_, f)| *f == "fetch"));
}
