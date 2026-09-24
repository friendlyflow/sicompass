//! End-to-end tests for the WASM plugin host, against a real component.
//!
//! The unit tests in `wasm_host` cover pure logic. These instantiate an actual
//! guest and drive it through the `Provider` trait, which is the only way to check
//! the parts that matter: that the capability model holds, and that a misbehaving
//! guest cannot take the host down.
//!
//! # The fixture
//!
//! `tests/fixtures/wasm/hello.wasm` and `net.wasm` are `examples/hello-plugin` and
//! `examples/net-plugin` from the sicompass-plugin-sdk repo, built for
//! `wasm32-wasip2` (ABI 0.2), which emits a component directly. Inside *that* repo's dev shell (this one's rustc has no
//! wasip2 `std`):
//!
//! ```text
//! cd ../sicompass-plugin-sdk
//! nix develop -c ./scripts/verify-guest.sh   # builds and audits both
//! cp examples/hello-plugin/plugin.wasm <this repo>/src/sicompass/tests/fixtures/wasm/hello.wasm
//! cp examples/net-plugin/plugin.wasm   <this repo>/src/sicompass/tests/fixtures/wasm/net.wasm
//! ```
//!
//! `hello-abi-0.1.wasm` is the last ABI 0.1 build of hello, kept only to prove an
//! old plugin is refused with a readable reason. Never rebuild it.
//!
//! They are committed rather than built here on purpose: building them would need a
//! guest toolchain for every `cargo test` run, including CI jobs that have no
//! business compiling guests. `wit_vendor_matches_host_tables`
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

        // Since ABI 0.2 guests are wasm32-wasip2 and their `std` imports WASI. The
        // capability model holds as long as every WASI import is in the inert
        // baseline (no preopens, empty environment, stdio to the log): sockets,
        // http or anything else from WASI must fail here.
        if iface.starts_with("wasi:") {
            let bare = iface.split('@').next().unwrap_or_default();
            assert!(
                wasm_host::wasi::is_baseline(bare),
                "component imports {iface}, which is not in the inert WASI baseline"
            );
            continue;
        }
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
        // ABI 0.2 (4.4): the desktop interface.
        .chain(wasm_host::DESKTOP_IMPORTS.iter())
        // ABI 0.2 (4.5): background tasks.
        .chain(wasm_host::TASK_IMPORTS.iter())
        // ABI 0.2 (4.6): processes.
        .chain(wasm_host::PROCESS_IMPORTS.iter())
        // ABI 0.2 (4.7): socket name resolution.
        .chain(wasm_host::SOCKET_IMPORTS.iter())
        // ABI 0.2 (4.9): whether the user holds a tier.
        .chain(wasm_host::LICENSE_IMPORTS.iter())
        .map(|(i, f)| (i.to_string(), f.to_string()))
        .collect();
    expected.sort();

    assert_eq!(
        found, expected,
        "the vendored WIT and the host's import tables disagree — one of them was \
         edited without the other"
    );
}

/// The `descriptor` record is the guest's whole self-declaration, and the host
/// builds one field-by-field in `default_descriptor`. A record is structural in
/// the component model, so a field added on one side and not the other does not
/// fail to compile and does not fail the import guard above — it fails when a
/// real plugin is instantiated, on a user's machine. Pin the field list.
#[test]
fn wit_vendor_descriptor_fields_match_the_host() {
    let wit = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/sicompass-plugin.wit"),
    )
    .expect("vendored WIT should exist");

    let mut resolve = wit_parser::Resolve::default();
    resolve
        .push_str("sicompass-plugin.wit", &wit)
        .expect("vendored WIT must parse");

    let fields = resolve
        .types
        .iter()
        .find_map(|(_, ty)| match (&ty.name, &ty.kind) {
            (Some(n), wit_parser::TypeDefKind::Record(r)) if n == "descriptor" => Some(
                r.fields
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<Vec<String>>(),
            ),
            _ => None,
        })
        .expect("vendored WIT must define `record descriptor`");

    assert_eq!(
        fields,
        vec![
            "name",
            "display-name",
            "version",
            "supports-config-files",
            "no-cache",
            "path-is-filesystem",
            "stable-root-key",
            "has-editor-semantics",
            "supports-structural-edit",
            "manual-dashboard-entry-allowed",
            "dashboard-kind",
            // ABI 0.2.
            "dashboard-uses-app-undo",
        ],
        "the vendored WIT's `descriptor` drifted from the host. Re-copy \
         sicompass-plugin.wit from the SDK repo, update `default_descriptor` and \
         the `Provider` forwarder in src/wasm_host/provider.rs, and rebuild the \
         .wasm fixtures in tests/fixtures/wasm/"
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

// ---------------------------------------------------------------------------
// ABI 0.2: the WASI baseline, plugin translations, the version gate
// ---------------------------------------------------------------------------

/// Every string in a fetched tree, depth first.
fn all_text(elements: &[FfonElement]) -> Vec<String> {
    let mut out = Vec::new();
    for e in elements {
        match e {
            FfonElement::Str(s) => out.push(s.clone()),
            FfonElement::Obj(o) => {
                out.push(o.key.clone());
                out.extend(all_text(&o.children));
            }
        }
    }
    out
}

/// The guest's `std::fs` compiles and links (baseline `wasi:filesystem`), but with
/// no `storage`/`filesystem` grant there is no preopened directory, so nothing on
/// the host is reachable. The fixture tries `/` and `/etc/passwd` and reports.
#[test]
fn std_fs_reaches_nothing_without_a_grant() {
    let mut p = open_hello();
    let text = all_text(&p.fetch());
    assert!(
        text.iter().any(|t| t == "fs: refused"),
        "the guest's std::fs reached the host filesystem: {text:?}"
    );
    assert!(!text.iter().any(|t| t.contains("LEAKED")), "{text:?}");
}

/// The baseline `wasi:clocks` makes `SystemTime::now()` work inside a guest.
#[test]
fn std_clock_works_in_a_guest() {
    let mut p = open_hello();
    let text = all_text(&p.fetch());
    assert!(text.iter().any(|t| t == "std clock: ok"), "{text:?}");
}

/// A plugin ships `locales/<lang>.ftl`, the host registers it, and the guest's
/// `translate-args` resolves against it with its arguments.
#[test]
fn a_plugin_translates_with_its_own_locale_file_and_arguments() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(hello_wasm(), dir.path().join("plugin.wasm")).unwrap();
    std::fs::create_dir(dir.path().join("locales")).unwrap();
    std::fs::write(
        dir.path().join("locales/en-US.ftl"),
        "hello-plugin-name = hello\nhello-plugin-greetings = greeted { $count } times\n",
    )
    .unwrap();
    sicompass_sdk::localize::set_locale("en-US");

    let mut p = WasmProvider::open(
        &dir.path().join("plugin.wasm"),
        "hello",
        "hello",
        dir.path(),
        Vec::new(),
    )
    .expect("hello loads from a temp plugin directory");
    let text = all_text(&p.fetch());
    assert!(
        text.iter().any(|t| t == "greeted 0 times"),
        "translate-args did not resolve the plugin's own message: {text:?}"
    );
}

/// A locale file with a message id outside the plugin's own prefix is refused
/// whole: the bundles are shared, and the first definition of an id wins.
#[test]
fn a_locale_file_with_a_foreign_message_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("locales")).unwrap();
    std::fs::write(
        dir.path().join("locales/nl-BE.ftl"),
        "spoof-ok = fine\nsettings-section-available-programs = Gestolen\n",
    )
    .unwrap();
    let refusals = wasm_host::register_plugin_locales("spoof", dir.path());
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].contains("settings-section-available-programs"),
        "{refusals:?}"
    );
}

/// A plugin built for ABI 0.1 (wasm32-unknown-unknown, no WASI) is refused before
/// instantiation, with a reason that says what to do.
#[test]
fn a_plugin_built_for_the_previous_abi_is_refused_readably() {
    let old = fixture_dir().join("hello-abi-0.1.wasm");
    let err = match WasmProvider::open(&old, "hello", "hello", &fixture_dir(), Vec::new()) {
        Ok(_) => panic!("an ABI 0.1 plugin must not load on an ABI 0.2 host"),
        Err(e) => e,
    };
    assert!(
        err.contains("ABI 0.1.0") && err.contains("rebuilt"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// 4.4: storage, granted folders, the desktop interface
// ---------------------------------------------------------------------------

fn fs_wasm() -> PathBuf {
    fixture_dir().join("fs.wasm")
}

fn open_fs(grants: wasm_host::Grants) -> WasmProvider {
    wasm_host::desktop::_set_test_mode(true);
    WasmProvider::open_with_grants(&fs_wasm(), "fs", "fs", &fixture_dir(), grants)
        .expect("the fs fixture loads")
}

/// Run one of the fs fixture's commands and return its one-line answer.
fn fs_cmd(p: &mut WasmProvider, cmd: &str, arg: &str) -> String {
    let mut error = String::new();
    match p.handle_command(cmd, arg, 0, &mut error) {
        Some(FfonElement::Str(s)) => s,
        other => panic!("{cmd} {arg}: unexpected answer {other:?} (error: {error})"),
    }
}

#[test]
fn storage_is_the_plugins_own_folder_and_only_when_granted() {
    let none = all_text(&open_fs(wasm_host::Grants::default()).fetch());
    assert!(none.contains(&"storage: absent".to_owned()), "{none:?}");

    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("fs-storage");
    let mut p = open_fs(wasm_host::Grants {
        storage_dir: Some(storage.clone()),
        ..Default::default()
    });
    assert!(all_text(&p.fetch()).contains(&"storage: present".to_owned()));
    assert_eq!(fs_cmd(&mut p, "write", "/storage/note.txt"), "ok");
    assert_eq!(
        std::fs::read_to_string(storage.join("note.txt")).unwrap(),
        "hello from fs-plugin"
    );
    assert_eq!(
        fs_cmd(&mut p, "read", "/storage/note.txt"),
        "hello from fs-plugin"
    );
    assert_eq!(fs_cmd(&mut p, "list", "/storage"), "note.txt");
    // Nothing outside it.
    assert!(fs_cmd(&mut p, "read", "/etc/passwd").starts_with("err"));
    assert!(fs_cmd(&mut p, "read", "/storage/../../etc/passwd").starts_with("err"));
}

#[test]
fn an_approved_folder_is_reachable_at_its_own_path_and_nothing_beside_it() {
    let granted = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let granted_path = granted.path().canonicalize().unwrap();
    let mut p = open_fs(wasm_host::Grants {
        filesystem: vec![granted_path.clone()],
        ..Default::default()
    });
    let inside = granted_path.join("x.txt");
    assert_eq!(fs_cmd(&mut p, "write", inside.to_str().unwrap()), "ok");
    assert!(inside.is_file());
    let outside = other.path().join("y.txt");
    assert!(fs_cmd(&mut p, "write", outside.to_str().unwrap()).starts_with("err"));
    assert!(!outside.exists());
}

#[test]
fn desktop_trash_and_restore_stay_inside_the_grant() {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().join("s");
    let mut p = open_fs(wasm_host::Grants {
        storage_dir: Some(storage.clone()),
        ..Default::default()
    });
    assert_eq!(fs_cmd(&mut p, "write", "/storage/t.txt"), "ok");
    assert_eq!(fs_cmd(&mut p, "trash", "/storage/t.txt"), "ok");
    assert!(!storage.join("t.txt").exists());
    assert_eq!(fs_cmd(&mut p, "restore", "/storage/t.txt"), "ok");
    assert!(storage.join("t.txt").is_file());

    assert!(fs_cmd(&mut p, "trash", "/etc/hostname").starts_with("err"));
    assert!(fs_cmd(&mut p, "open-path", "/etc/passwd").starts_with("err"));
    wasm_host::desktop::_take_recorded();
    assert_eq!(fs_cmd(&mut p, "open-path", "/storage/t.txt"), "ok");
    assert_eq!(fs_cmd(&mut p, "open-url", "https://example.com/x"), "ok");
    assert!(fs_cmd(&mut p, "open-url", "file:///etc/passwd").starts_with("err"));
    let recorded = wasm_host::desktop::_take_recorded();
    assert!(
        recorded
            .iter()
            .any(|r| r.starts_with("open-path:") && r.ends_with("t.txt")),
        "{recorded:?}"
    );
    assert!(
        recorded.contains(&"open-url:https://example.com/x".to_owned()),
        "{recorded:?}"
    );
}

#[test]
fn a_filesystem_grant_needs_the_users_approval_of_exactly_this_manifest() {
    use sicompass::plugin_manifest::{grants_for, parse_manifest};
    let m = parse_manifest(
        r#"{ "name": "fb", "displayName": "fb", "entry": "plugin.wasm",
             "permissions": { "filesystem": ["~/Documents"], "storage": true } }"#,
    )
    .unwrap();
    let mut approvals = std::collections::HashMap::new();
    let e = grants_for(&m, &approvals).unwrap_err();
    assert!(e.contains("approve"), "{e}");

    approvals.insert(
        "fb".to_owned(),
        sicompass_sdk::plugin_abi::approval_fingerprint(&m),
    );
    let g = grants_for(&m, &approvals).unwrap();
    assert!(g.filesystem[0].ends_with("Documents") && !g.filesystem[0].starts_with("~"));
    assert!(g.storage_dir.unwrap().ends_with("fb"));

    // An update asking for more is held back until approved again.
    let more = parse_manifest(
        r#"{ "name": "fb", "displayName": "fb", "entry": "plugin.wasm",
             "permissions": { "filesystem": ["~/Documents", "~/Pictures"] } }"#,
    )
    .unwrap();
    assert!(grants_for(&more, &approvals).is_err());
}

// ---------------------------------------------------------------------------
// 4.5: background tasks
// ---------------------------------------------------------------------------

fn open_task() -> WasmProvider {
    WasmProvider::open(
        &fixture_dir().join("task.wasm"),
        "task",
        "task",
        &fixture_dir(),
        Vec::new(),
    )
    .expect("the task fixture loads")
}

/// Start a task through the fixture's command and return its id.
fn start(p: &mut WasmProvider, cmd: &str, arg: &str) -> u64 {
    let mut error = String::new();
    match p.handle_command(cmd, arg, 0, &mut error) {
        Some(FfonElement::Str(id)) => id.parse().unwrap(),
        other => panic!("{cmd}: {other:?} ({error})"),
    }
}

/// Tick (which delivers task events) until `pred` holds on the event log, or
/// panic after `secs`.
fn wait_for(p: &mut WasmProvider, secs: u64, pred: impl Fn(&[String]) -> bool) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        p.tick();
        let log = all_text(&p.fetch());
        if pred(&log) {
            return log;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out after {secs}s; events so far: {log:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_task_reports_progress_in_order_then_its_result() {
    let mut p = open_task();
    let id = start(&mut p, "count", "3");
    let log = wait_for(&mut p, 10, |l| l.iter().any(|x| x.contains("done")));
    assert_eq!(
        log,
        vec![
            format!("started {id}"),
            format!("{id} progress 0"),
            format!("{id} progress 1"),
            format!("{id} progress 2"),
            format!("{id} done ok counted 3"),
        ]
    );
}

/// The whole point of a task: it is not bound by the 10-second call deadline.
/// Deliberately slow (about 11 seconds).
#[test]
fn a_task_outlives_the_call_deadline() {
    let mut p = open_task();
    let id = start(&mut p, "sleep", "11");
    let log = wait_for(&mut p, 30, |l| l.iter().any(|x| x.contains("done")));
    assert!(log.contains(&format!("{id} done ok slept")), "{log:?}");
}

#[test]
fn a_cooperative_task_stops_when_asked() {
    let mut p = open_task();
    let id = start(&mut p, "spin", "");
    wait_for(&mut p, 10, |l| {
        l.contains(&format!("{id} progress running"))
    });
    let mut error = String::new();
    p.handle_command("cancel", &id.to_string(), 0, &mut error);
    let log = wait_for(&mut p, 10, |l| l.iter().any(|x| x.contains("done")));
    assert!(log.contains(&format!("{id} done ok stopped")), "{log:?}");
}

#[test]
fn a_task_that_ignores_cancel_is_stopped_by_the_host() {
    let mut p = open_task();
    let id = start(&mut p, "busy", "");
    wait_for(&mut p, 10, |l| {
        l.contains(&format!("{id} progress running"))
    });
    let mut error = String::new();
    p.handle_command("cancel", &id.to_string(), 0, &mut error);
    let log = wait_for(&mut p, 10, |l| l.iter().any(|x| x.contains("done")));
    assert!(log.contains(&format!("{id} done err cancelled")), "{log:?}");
}

#[test]
fn at_most_four_tasks_run_at_once_and_the_next_starts_when_one_ends() {
    let mut p = open_task();
    let ids: Vec<u64> = (0..5).map(|_| start(&mut p, "spin", "")).collect();
    let running = |l: &[String]| l.iter().filter(|x| x.ends_with("progress running")).count();
    wait_for(&mut p, 10, |l| running(l) == 4);
    // Give a fifth a moment it must not use.
    std::thread::sleep(std::time::Duration::from_millis(400));
    p.tick();
    assert_eq!(running(&all_text(&p.fetch())), 4);

    let mut error = String::new();
    p.handle_command("cancel", &ids[0].to_string(), 0, &mut error);
    wait_for(&mut p, 10, |l| running(l) == 5);
    for id in &ids[1..] {
        p.handle_command("cancel", &id.to_string(), 0, &mut error);
    }
    wait_for(&mut p, 10, |l| {
        l.iter().filter(|x| x.contains("done")).count() == 5
    });
}

#[test]
fn a_task_cannot_start_a_task() {
    let mut p = open_task();
    let id = start(&mut p, "nested", "");
    let log = wait_for(&mut p, 10, |l| l.iter().any(|x| x.contains("done")));
    assert!(
        log.contains(&format!("{id} done err a task cannot start tasks")),
        "{log:?}"
    );
}

/// Threads of this process whose name is `name` (Linux: `/proc/self/task/*/comm`).
#[cfg(target_os = "linux")]
fn threads_named(name: &str) -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .flatten()
        .filter(|t| {
            std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim_end() == name)
        })
        .count()
}

/// A provider dropped without `cleanup` must not leave a task spinning a core.
/// A plugin name no other test uses, so the worker thread (`task:<name>`) is
/// this test's alone.
#[cfg(target_os = "linux")]
#[test]
fn dropping_the_provider_stops_its_tasks() {
    let mut p = WasmProvider::open(
        &fixture_dir().join("task.wasm"),
        "dropme",
        "dropme",
        &fixture_dir(),
        Vec::new(),
    )
    .unwrap();
    let id = start(&mut p, "busy", "");
    wait_for(&mut p, 10, |l| {
        l.contains(&format!("{id} progress running"))
    });
    assert_eq!(threads_named("task:dropme"), 1);
    drop(p);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while threads_named("task:dropme") > 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the task outlived its provider"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------------------
// 4.6: processes
// ---------------------------------------------------------------------------

fn open_process(grants: wasm_host::Grants) -> Result<WasmProvider, String> {
    WasmProvider::open_with_grants(
        &fixture_dir().join("process.wasm"),
        "process",
        "process",
        &fixture_dir(),
        grants,
    )
}

fn process_grants() -> wasm_host::Grants {
    wasm_host::Grants {
        process: vec!["echo".into(), "cat".into(), "sh".into()],
        ..Default::default()
    }
}

#[test]
fn a_plugin_that_starts_programs_needs_them_granted() {
    let e = match open_process(wasm_host::Grants::default()) {
        Ok(_) => panic!("process must not be linked without a grant"),
        Err(e) => e,
    };
    assert!(e.contains("permissions.process"), "{e}");
}

#[test]
fn a_listed_program_runs_on_pipes() {
    let mut p = open_process(process_grants()).unwrap();
    assert_eq!(
        fs_cmd(&mut p, "run", "echo hello there"),
        "exit 0: hello there\n"
    );
}

#[test]
fn a_program_not_listed_is_refused() {
    let mut p = open_process(process_grants()).unwrap();
    let a = fs_cmd(&mut p, "run", "rm -rf /tmp/sicompass-never");
    assert!(a.starts_with("err:") && a.contains("not among"), "{a}");
}

#[test]
fn a_program_runs_on_a_pty() {
    let mut p = open_process(process_grants()).unwrap();
    let a = fs_cmd(&mut p, "pty", "");
    assert!(a.starts_with("exit 3:") && a.contains("pty-42"), "{a}");
}

#[test]
fn stdin_reaches_the_program() {
    let mut p = open_process(process_grants()).unwrap();
    assert_eq!(fs_cmd(&mut p, "stdin", ""), "piped through\n");
}

#[test]
fn the_plugin_sets_the_programs_environment() {
    let mut p = open_process(process_grants()).unwrap();
    assert_eq!(fs_cmd(&mut p, "env", ""), "exit 0: from-the-plugin\n");
}

/// A program inherits sicompass's environment, and the plugin can remove an
/// inherited variable (the git client removes `GIT_DIR` and its kind, so a
/// stray one can never point it at another repository). `HOME` is always
/// inherited, which makes it a reliable one to remove here.
#[test]
fn the_plugin_can_unset_an_inherited_variable() {
    let mut p = open_process(process_grants()).unwrap();
    assert_eq!(fs_cmd(&mut p, "unset", "HOME"), "exit 0: unset\n");
}

#[test]
fn the_working_directory_must_be_granted() {
    let dir = tempfile::tempdir().unwrap();
    let granted = dir.path().canonicalize().unwrap();
    let mut grants = process_grants();
    grants.filesystem = vec![granted.clone()];
    let mut p = open_process(grants).unwrap();
    let sub = granted.join("work");
    std::fs::create_dir(&sub).unwrap();
    assert_eq!(
        fs_cmd(&mut p, "cwd", sub.to_str().unwrap()),
        format!("exit 0: {}\n", sub.display())
    );
    let outside = fs_cmd(&mut p, "cwd", "/etc");
    assert!(outside.starts_with("err:"), "{outside}");
}

// ---------------------------------------------------------------------------
// 4.7: sockets
// ---------------------------------------------------------------------------

/// A line-echo server on 127.0.0.1, answering `pong` to every line, one thread
/// per connection. Returns its port.
fn echo_server() -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut w = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let _ = writeln!(w, "pong to {line}");
                }
            });
        }
    });
    port
}

fn open_socket(endpoints: Vec<String>) -> Result<WasmProvider, String> {
    WasmProvider::open_with_grants(
        &fixture_dir().join("socket.wasm"),
        "socket",
        "socket",
        &fixture_dir(),
        wasm_host::Grants {
            sockets: endpoints,
            ..Default::default()
        },
    )
}

#[test]
fn a_granted_endpoint_connects_by_name() {
    let port = echo_server();
    let mut p = open_socket(vec![format!("localhost:{port}")]).unwrap();
    assert_eq!(
        fs_cmd(&mut p, "echo", &format!("localhost:{port}")),
        "pong to ping"
    );
}

#[test]
fn a_port_that_was_not_granted_is_refused_even_by_address() {
    let granted = echo_server();
    let other = echo_server();
    let mut p = open_socket(vec![format!("localhost:{granted}")]).unwrap();
    // The same address as the granted one, another port: the host's socket
    // check refuses it (not "unsupported", which would prove nothing).
    let a = fs_cmd(&mut p, "raw", &format!("127.0.0.1:{other}"));
    assert!(a.starts_with("err:") && !a.contains("not supported"), "{a}");
    // And the granted one, by address, works.
    assert_eq!(
        fs_cmd(&mut p, "raw", &format!("127.0.0.1:{granted}")),
        "pong to ping"
    );
    let a = fs_cmd(&mut p, "echo", &format!("localhost:{other}"));
    assert!(a.starts_with("err:") && a.contains("not among"), "{a}");
}

#[test]
fn a_name_that_was_not_granted_does_not_resolve() {
    let port = echo_server();
    let mut p = open_socket(vec![format!("127.0.0.1:{port}")]).unwrap();
    let a = fs_cmd(&mut p, "echo", &format!("localhost:{port}"));
    assert!(a.starts_with("err:") && a.contains("not among"), "{a}");
    // The same address, approved as an IP, works.
    assert_eq!(
        fs_cmd(&mut p, "echo", &format!("127.0.0.1:{port}")),
        "pong to ping"
    );
}

#[test]
fn a_plugin_that_uses_sockets_needs_them_granted() {
    let e = match open_socket(Vec::new()) {
        Ok(_) => panic!("sockets must not be linked without a grant"),
        Err(e) => e,
    };
    assert!(e.contains("permissions.sockets"), "{e}");
}
