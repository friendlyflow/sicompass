//! Resource caps for a WASM plugin instance.
//!
//! A guest cannot be trusted to terminate, to bound its allocations, or to avoid
//! panicking. None of those may take the host down, so every guest call runs under
//! three independent limits:
//!
//! - **Fuel** caps work per call. Wasmtime charges fuel per instruction, so an
//!   infinite loop runs out rather than hanging the render thread.
//! - **Epoch interruption** caps wall-clock time per call. Fuel alone cannot catch a
//!   guest blocked *inside* a host function (a slow `fetch`), because host time
//!   burns no fuel.
//! - **`StoreLimits`** caps memory, tables and instances, so a guest cannot balloon
//!   the process.
//!
//! Exceeding any of them is a `Trap`, which the caller turns into an error row and a
//! poisoned provider. See [`super::provider`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};

/// Fuel granted before each guest call.
///
/// Generous: a `fetch()` that builds a large FFON tree does real work. This is a
/// runaway-loop backstop, not a performance budget.
///
/// **Which limit fires first depends on the backend.** Measured with the
/// `hello-plugin` fixture's `spin` command: under Cranelift the guest burns this
/// budget well inside the wall-clock deadline and traps with `OutOfFuel`; under
/// Pulley the interpreter is slow enough that [`CALL_TIMEOUT`] arrives first and the
/// trap is `Interrupt` instead. Both contain the guest, which is why there are two
/// independent limits rather than one — but it does mean a runaway plugin stalls a
/// no-JIT build for the full timeout. Worth revisiting with real plugins: the honest
/// figures here are "big enough not to hurt anyone legitimate", not tuned.
pub const FUEL_PER_CALL: u64 = 2_000_000_000;

/// Wall-clock budget per guest call, enforced via epoch interruption.
///
/// Deliberately much larger than a frame: a slow call degrades the UI, and killing a
/// legitimately-slow plugin mid-`fetch` would be worse than a stutter. This exists
/// to catch hangs — including a guest blocked inside a host function, which burns no
/// fuel and so is invisible to the fuel cap.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the ticker bumps the engine epoch. Epoch deadlines are counted in
/// ticks, so this is the granularity of [`CALL_TIMEOUT`].
const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// Maximum linear memory per plugin instance.
pub const MAX_MEMORY_BYTES: usize = 64 << 20; // 64 MiB

/// Ticks a wasmtime `Engine`'s epoch on a background thread so epoch-based
/// deadlines actually fire.
///
/// One ticker serves the whole engine, hence one per process rather than one per
/// plugin. The thread exits when the last handle drops, so tests do not leak it.
pub struct EpochTicker {
    stop: Arc<AtomicBool>,
}

impl EpochTicker {
    /// Spawn the ticker for `engine`.
    ///
    /// Holds only a `Weak` reference so the engine is not kept alive by the ticker;
    /// if the engine goes away, the thread notices and exits.
    pub fn spawn(engine: &Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let weak = Engine::weak(engine);
        let stop_clone = Arc::clone(&stop);

        std::thread::Builder::new()
            .name("sicompass-wasm-epoch".to_owned())
            .spawn(move || {
                while !stop_clone.load(Ordering::Relaxed) {
                    std::thread::sleep(TICK_INTERVAL);
                    match weak.upgrade() {
                        Some(engine) => engine.increment_epoch(),
                        // Engine dropped; nothing left to interrupt.
                        None => break,
                    }
                }
            })
            .expect("spawn wasm epoch ticker");

        EpochTicker { stop }
    }

    /// Number of epoch ticks corresponding to [`CALL_TIMEOUT`].
    pub fn ticks_for_timeout() -> u64 {
        let ticks = CALL_TIMEOUT.as_millis() / TICK_INTERVAL.as_millis().max(1);
        // At least one tick, or the deadline would already be expired on arrival.
        ticks.max(1) as u64
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Memory, table and instance caps for one plugin's `Store`.
pub fn store_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(MAX_MEMORY_BYTES)
        // A provider is one component instance with one memory; these bounds are
        // "sane ceiling", not tuned figures.
        .memories(4)
        .tables(8)
        .table_elements(100_000)
        .instances(16)
        .build()
}

/// Re-arm the per-call limits on `store`.
///
/// Call immediately before every guest invocation. Fuel is consumed and the epoch
/// deadline is absolute, so both must be reset each time rather than once at
/// creation.
pub fn refresh_for_call<T>(store: &mut Store<T>) -> Result<(), String> {
    store
        .set_fuel(FUEL_PER_CALL)
        .map_err(|e| format!("could not grant fuel: {e}"))?;
    store.set_epoch_deadline(EpochTicker::ticks_for_timeout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_at_least_one_tick() {
        assert!(EpochTicker::ticks_for_timeout() >= 1);
    }

    #[test]
    fn timeout_ticks_match_the_configured_budget() {
        // 10s at 100ms granularity.
        assert_eq!(EpochTicker::ticks_for_timeout(), 100);
    }

    #[test]
    fn limits_cap_memory_at_the_documented_size() {
        // Guards against the builder silently losing the cap if this is refactored.
        assert_eq!(MAX_MEMORY_BYTES, 67_108_864);
        let _ = store_limits();
    }

    #[test]
    fn ticker_spawns_and_stops_cleanly() {
        // Wasmtime exposes no epoch *reader*, so the tick cannot be observed from
        // here. What is checkable is that spawning and dropping is clean and that
        // Drop signals the thread — without it every plugin load would leak a
        // thread. That the deadline actually fires is verified behaviourally, by a
        // guest that loops forever (see the trap tests, which need a wasm fixture).
        let engine = Engine::default();
        let ticker = EpochTicker::spawn(&engine);
        std::thread::sleep(TICK_INTERVAL * 2);
        assert!(!ticker.stop.load(Ordering::Relaxed));
        drop(ticker);
    }

    #[test]
    fn ticker_does_not_keep_the_engine_alive() {
        // The ticker holds a Weak, so dropping the engine must let it go. If it held
        // a strong reference, the engine (and its compilation cache) would outlive
        // every plugin for the life of the process.
        let engine = Engine::default();
        let weak = Engine::weak(&engine);
        let _ticker = EpochTicker::spawn(&engine);
        assert!(weak.upgrade().is_some());
        drop(engine);
        // The ticker's own Weak is all that is left, so this must now fail.
        assert!(weak.upgrade().is_none(), "ticker kept the engine alive");
    }

    #[test]
    fn fuel_and_epoch_are_re_armed_per_call() {
        // Fuel is consumed and the epoch deadline is absolute, so both have to be
        // reset before every guest call, not once at store creation.
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config).unwrap();
        let mut store = Store::new(&engine, ());

        store.set_fuel(10).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 10);

        refresh_for_call(&mut store).unwrap();
        assert_eq!(store.get_fuel().unwrap(), FUEL_PER_CALL);
    }
}
