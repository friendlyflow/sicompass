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
//!
//! The UI instance can also talk to a task: `tasks.send` puts a message in its
//! [`Inbox`] and `tasks.receive` takes it out, so a task can live as long as
//! the plugin, holding an IMAP session or a browser and doing what the UI asks,
//! with no deadline on the waiting.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use sicompass_sdk::plugin_abi::MAX_CONCURRENT_TASKS;
use wasmtime::component::Component;
use wasmtime::{Store, UpdateDeadline};

use super::sicompass::plugin as wit;
use super::wit_types::TaskEvent;
use super::{Grants, HostState, Plugin};

/// Epoch ticks (100 ms each) a cancelled task gets to stop by itself.
pub const CANCEL_GRACE_TICKS: u32 = 5;

/// Messages waiting in one task's inbox before `send` refuses more: a task
/// that does not read cannot make the host hold everything the UI sends it.
pub const INBOX_LIMIT: usize = 256;

/// The longest one `receive` waits.
const RECEIVE_MAX: Duration = Duration::from_secs(60);

/// How often a waiting `receive` looks at the cancel flag.
const RECEIVE_SLICE: Duration = Duration::from_millis(100);

/// What the UI instance sent one task and it has not received yet.
#[derive(Default)]
pub struct Inbox {
    queue: Mutex<VecDeque<Vec<u8>>>,
    ready: Condvar,
}

impl Inbox {
    fn push(&self, message: Vec<u8>) -> Result<(), String> {
        let mut q = self.queue.lock().map_err(|e| e.to_string())?;
        if q.len() >= INBOX_LIMIT {
            return Err("the task is not keeping up with what it is sent".to_owned());
        }
        q.push_back(message);
        self.ready.notify_one();
        Ok(())
    }

    /// The next message, waiting up to `timeout`; `None` in time or on cancel.
    fn pop(&self, timeout: Duration, cancel: &AtomicBool) -> Option<Vec<u8>> {
        let deadline = Instant::now() + timeout.min(RECEIVE_MAX);
        let mut q = self.queue.lock().ok()?;
        loop {
            if let Some(m) = q.pop_front() {
                return Some(m);
            }
            let now = Instant::now();
            if cancel.load(Ordering::Acquire) || now >= deadline {
                return None;
            }
            let wait = (deadline - now).min(RECEIVE_SLICE);
            q = self.ready.wait_timeout(q, wait).ok()?.0;
        }
    }
}

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
        inbox: Arc<Inbox>,
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
    inboxes: HashMap<u64, Arc<Inbox>>,
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
        st.inboxes.insert(id, Arc::new(Inbox::default()));
        let job = Queued { id, name, input };
        if st.running < MAX_CONCURRENT_TASKS {
            st.running += 1;
            let cancel = st.cancels[&id].clone();
            let inbox = st.inboxes[&id].clone();
            drop(st);
            self.start(job, cancel, inbox);
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
            st.inboxes.remove(&id);
            let _ = self.tx.send((id, TaskEvent::Done(Err("cancelled".to_owned()))));
        } else if let Some(flag) = st.cancels.get(&id) {
            flag.store(true, Ordering::Release);
            // A task waiting in `receive` notices at once.
            if let Some(inbox) = st.inboxes.get(&id) {
                inbox.ready.notify_all();
            }
        }
    }

    /// Put `message` in task `id`'s inbox (see `tasks.send`).
    pub fn send(&self, id: u64, message: Vec<u8>) -> Result<(), String> {
        let inbox = {
            let st = self.state.lock().map_err(|e| e.to_string())?;
            st.inboxes.get(&id).cloned()
        };
        inbox
            .ok_or_else(|| format!("task {id} has ended"))?
            .push(message)
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
        for inbox in st.inboxes.values() {
            inbox.ready.notify_all();
        }
    }

    /// Events waiting for the UI instance, in the order they happened.
    pub fn drain(&self) -> Vec<(u64, TaskEvent)> {
        self.rx
            .lock()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default()
    }

    fn start(self: &Arc<Self>, job: Queued, cancel: Arc<AtomicBool>, inbox: Arc<Inbox>) {
        let me = self.clone();
        let id = job.id;
        let spawned = std::thread::Builder::new()
            // Short on purpose: Linux keeps 15 bytes of a thread name, and this
            // is what shows in `top -H` and what a test watches.
            .name(format!("task:{}", self.spec.plugin_name))
            .spawn(move || {
                let result = run(&me.spec, &job, &cancel, &me.tx, inbox);
                let _ = me.tx.send((job.id, TaskEvent::Done(result)));
                me.finished(job.id);
            });
        if let Err(e) = spawned {
            let _ = self.tx.send((id, TaskEvent::Done(Err(format!("no worker thread: {e}")))));
            self.finished(id);
        }
    }

    fn finished(self: &Arc<Self>, id: u64) {
        let next = {
            let Ok(mut st) = self.state.lock() else { return };
            st.cancels.remove(&id);
            st.inboxes.remove(&id);
            st.running -= 1;
            if st.closed {
                None
            } else {
                st.queue.pop_front().map(|job| {
                    st.running += 1;
                    let cancel = st.cancels[&job.id].clone();
                    let inbox = st.inboxes[&job.id].clone();
                    (job, cancel, inbox)
                })
            }
        };
        if let Some((job, cancel, inbox)) = next {
            self.start(job, cancel, inbox);
        }
    }
}

/// Run one task in a fresh instance, on the current (worker) thread.
fn run(
    spec: &TaskSpec,
    job: &Queued,
    cancel: &Arc<AtomicBool>,
    events: &mpsc::Sender<(u64, TaskEvent)>,
    inbox: Arc<Inbox>,
) -> Result<Vec<u8>, String> {
    let (id, name, input) = (job.id, job.name.as_str(), job.input.as_slice());
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
        inbox,
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

    fn send(&mut self, id: u64, message: Vec<u8>) -> Result<(), String> {
        match &self.tasks {
            TaskRole::Ui(manager) => manager.send(id, message),
            TaskRole::Worker { .. } => Err("a task cannot send to tasks".to_owned()),
            TaskRole::Unmanaged => Err("tasks are not available here".to_owned()),
        }
    }

    fn receive(&mut self, timeout_ms: u32) -> Option<Vec<u8>> {
        match &self.tasks {
            TaskRole::Worker { cancel, inbox, .. } => {
                inbox.pop(Duration::from_millis(timeout_ms.into()), cancel)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inbox_hands_messages_over_in_order() {
        let inbox = Inbox::default();
        let cancel = AtomicBool::new(false);
        inbox.push(b"a".to_vec()).unwrap();
        inbox.push(b"b".to_vec()).unwrap();
        assert_eq!(inbox.pop(Duration::ZERO, &cancel), Some(b"a".to_vec()));
        assert_eq!(inbox.pop(Duration::ZERO, &cancel), Some(b"b".to_vec()));
        assert_eq!(inbox.pop(Duration::from_millis(30), &cancel), None);
    }

    #[test]
    fn an_inbox_refuses_a_backlog_the_task_is_not_reading() {
        let inbox = Inbox::default();
        for _ in 0..INBOX_LIMIT {
            inbox.push(Vec::new()).unwrap();
        }
        assert!(inbox.push(Vec::new()).is_err());
    }

    #[test]
    fn a_wait_ends_with_a_message_sent_meanwhile() {
        let inbox = Arc::new(Inbox::default());
        let other = inbox.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            other.push(b"late".to_vec()).unwrap();
        });
        let cancel = AtomicBool::new(false);
        assert_eq!(inbox.pop(Duration::from_secs(5), &cancel), Some(b"late".to_vec()));
        sender.join().unwrap();
    }
}
