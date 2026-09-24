//! `sicompass:plugin/tasks`: a plugin's background work.
//!
//! Every call into a provider runs on the UI thread under a 10-second deadline,
//! so anything slower runs as a task: a **fresh instance of the same component**
//! on a worker thread, with the same grants and its own store and memory. No
//! state is shared: the task gets its `input`, reports through `emit` and its
//! result, and the host delivers those to the UI instance's `on-task-event`
//! just before the next `poll` ([`TaskManager::drain`]).
//!
//! A task has no deadline and unlimited fuel, but a cancel flag checked at every
//! epoch tick (100 ms): after [`TaskManager::cancel`], or closing the provider
//! ([`TaskManager::close`]), a task that has not stopped by itself within
//! [`CANCEL_GRACE_TICKS`] ticks is stopped the next time it runs WebAssembly. A
//! task blocked inside a host call (a network request, a sleep) stops when that
//! call returns. `tasks.cancelled()` lets a loop stop cleanly, and the grace is
//! what makes sure it gets the chance to, even on a loaded machine. At most
//! [`MAX_CONCURRENT_TASKS`] run per plugin; further ones wait in order.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use sicompass_sdk::plugin_abi::MAX_CONCURRENT_TASKS;
use wasmtime::component::Component;
use wasmtime::{Store, UpdateDeadline};

use super::sicompass::plugin as wit;
use super::wit_types::TaskEvent;
use super::{Grants, HostState, Plugin};

/// Epoch ticks (100 ms each) a cancelled task gets to stop by itself.
pub const CANCEL_GRACE_TICKS: u32 = 5;

/// Which side of the task boundary a `HostState` is on.
#[derive(Default)]
pub enum TaskRole {
    /// The UI instance, and its manager once the provider has one. `None` for a
    /// state built outside a provider (unit tests), where `spawn` is refused.
    #[default]
    Unmanaged,
    Ui(Arc<TaskManager>),
    /// A worker instance running task `id`.
    Worker {
        id: u64,
        cancel: Arc<AtomicBool>,
        events: mpsc::Sender<(u64, TaskEvent)>,
    },
}

/// Everything needed to build another instance of a plugin.
#[derive(Clone)]
pub struct TaskSpec {
    pub component: Component,
    pub plugin_name: String,
    pub settings_section: String,
    pub plugin_dir: PathBuf,
    pub grants: Grants,
}

struct Queued {
    id: u64,
    name: String,
    input: Vec<u8>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    running: usize,
    queue: VecDeque<Queued>,
    cancels: HashMap<u64, Arc<AtomicBool>>,
    closed: bool,
}

/// One plugin's tasks: the worker pool, the queue and the event channel.
pub struct TaskManager {
    spec: TaskSpec,
    state: Mutex<State>,
    tx: mpsc::Sender<(u64, TaskEvent)>,
    rx: Mutex<mpsc::Receiver<(u64, TaskEvent)>>,
}

impl TaskManager {
    pub fn new(spec: TaskSpec) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        Arc::new(TaskManager {
            spec,
            state: Mutex::new(State::default()),
            tx,
            rx: Mutex::new(rx),
        })
    }

    /// Start (or queue) a task. Returns its id.
    pub fn spawn(self: &Arc<Self>, name: String, input: Vec<u8>) -> Result<u64, String> {
        let mut st = self.state.lock().map_err(|e| e.to_string())?;
        if st.closed {
            return Err("this plugin is shutting down".to_owned());
        }
        st.next_id += 1;
        let id = st.next_id;
        st.cancels.insert(id, Arc::new(AtomicBool::new(false)));
        let job = Queued { id, name, input };
        if st.running < MAX_CONCURRENT_TASKS {
            st.running += 1;
            let cancel = st.cancels[&id].clone();
            drop(st);
            self.start(job, cancel);
        } else {
            st.queue.push_back(job);
        }
        Ok(id)
    }

    /// Ask task `id` to stop. A queued task is dropped with a `done` error.
    pub fn cancel(&self, id: u64) {
        let Ok(mut st) = self.state.lock() else { return };
        if let Some(pos) = st.queue.iter().position(|q| q.id == id) {
            st.queue.remove(pos);
            st.cancels.remove(&id);
            let _ = self.tx.send((id, TaskEvent::Done(Err("cancelled".to_owned()))));
        } else if let Some(flag) = st.cancels.get(&id) {
            flag.store(true, Ordering::Release);
        }
    }

    /// Stop everything: cancel running tasks, drop queued ones. Called when the
    /// provider goes away.
    pub fn close(&self) {
        let Ok(mut st) = self.state.lock() else { return };
        st.closed = true;
        st.queue.clear();
        for flag in st.cancels.values() {
            flag.store(true, Ordering::Release);
        }
    }

    /// Events waiting for the UI instance, in the order they happened.
    pub fn drain(&self) -> Vec<(u64, TaskEvent)> {
        self.rx
            .lock()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default()
    }

    fn start(self: &Arc<Self>, job: Queued, cancel: Arc<AtomicBool>) {
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            // Short on purpose: Linux keeps 15 bytes of a thread name, and this
            // is what shows in `top -H` and what a test watches.
            .name(format!("task:{}", self.spec.plugin_name))
            .spawn(move || {
                let result = run(&me.spec, job.id, &job.name, &job.input, &cancel, &me.tx);
                let _ = me.tx.send((job.id, TaskEvent::Done(result)));
                me.finished(job.id);
            });
        if let Err(e) = spawned {
            let _ = self.tx.send((job.id, TaskEvent::Done(Err(format!("no worker thread: {e}")))));
            self.finished(job.id);
        }
    }

    fn finished(self: &Arc<Self>, id: u64) {
        let next = {
            let Ok(mut st) = self.state.lock() else { return };
            st.cancels.remove(&id);
            st.running -= 1;
            if st.closed {
                None
            } else {
                st.queue.pop_front().map(|job| {
                    st.running += 1;
                    let cancel = st.cancels[&job.id].clone();
                    (job, cancel)
                })
            }
        };
        if let Some((job, cancel)) = next {
            self.start(job, cancel);
        }
    }
}

/// Run one task in a fresh instance, on the current (worker) thread.
fn run(
    spec: &TaskSpec,
    id: u64,
    name: &str,
    input: &[u8],
    cancel: &Arc<AtomicBool>,
    events: &mpsc::Sender<(u64, TaskEvent)>,
) -> Result<Vec<u8>, String> {
    let mut state = HostState::with_grants(
        spec.plugin_name.clone(),
        spec.settings_section.clone(),
        spec.plugin_dir.clone(),
        spec.grants.clone(),
    )?;
    // A task holds up no frame, so its requests may wait out a long poll.
    state.fetch_policy.for_task();
    state.tasks = TaskRole::Worker {
        id,
        cancel: cancel.clone(),
        events: events.clone(),
    };
    let linker = super::linker_for(&state)?;
    let mut store = Store::new(super::engine(), state);
    store.limiter(|s: &mut HostState| &mut s.limits);
    // No deadline and no fuel budget: a task is allowed to take long. What stops
    // it is the cancel flag, checked at every epoch tick.
    store
        .set_fuel(u64::MAX)
        .map_err(|e| format!("could not grant fuel: {e}"))?;
    let flag = cancel.clone();
    let mut ticks_since_cancel = 0u32;
    store.epoch_deadline_callback(move |_| {
        if !flag.load(Ordering::Acquire) {
            return Ok(UpdateDeadline::Continue(1));
        }
        ticks_since_cancel += 1;
        if ticks_since_cancel > CANCEL_GRACE_TICKS {
            Err(wasmtime::Error::msg("cancelled"))
        } else {
            Ok(UpdateDeadline::Continue(1))
        }
    });
    store.set_epoch_deadline(1);

    let instance = Plugin::instantiate(&mut store, &spec.component, &linker)
        .map_err(|e| format!("task {name}: {e}"))?;
    match instance
        .sicompass_plugin_provider()
        .call_run_task(&mut store, name, input)
    {
        Ok(result) => result,
        Err(_) if cancel.load(Ordering::Acquire) => Err("cancelled".to_owned()),
        Err(e) => Err(format!("task {name} stopped: {e}")),
    }
}

impl wit::tasks::Host for HostState {
    fn spawn(&mut self, name: String, input: Vec<u8>) -> Result<u64, String> {
        match &self.tasks {
            TaskRole::Ui(manager) => manager.spawn(name, input),
            TaskRole::Worker { .. } => Err("a task cannot start tasks".to_owned()),
            TaskRole::Unmanaged => Err("tasks are not available here".to_owned()),
        }
    }

    fn cancel(&mut self, id: u64) {
        if let TaskRole::Ui(manager) = &self.tasks {
            manager.cancel(id);
        }
    }

    fn emit(&mut self, event: Vec<u8>) {
        if let TaskRole::Worker { id, events, .. } = &self.tasks {
            let _ = events.send((*id, TaskEvent::Progress(event)));
        }
    }

    fn cancelled(&mut self) -> bool {
        match &self.tasks {
            TaskRole::Worker { cancel, .. } => cancel.load(Ordering::Acquire),
            _ => false,
        }
    }
}
