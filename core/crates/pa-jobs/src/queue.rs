// SPDX-License-Identifier: MPL-2.0
//! [`JobQueue`]: the [`JobStore`] with handlers, worker threads, events and
//! a background scheduler.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::store::{RunOutcome, ScheduleInput};
use crate::{
    Backoff, Conditions, Error, Job, JobEvent, JobId, JobState, JobStore, Limits, NewJob,
    PauseReason, ResourceGate,
};

/// Runs jobs of one type on a worker thread.
///
/// A handler must be resumable: it reads [`JobContext::checkpoint`] and
/// continues from there (HTTP Range from `bytes_done`, the next HLS
/// segment), saves progress with [`JobContext::save_checkpoint`], writes
/// into a temporary file and moves it into place atomically at the end. It
/// checks [`JobContext::stop_requested`] often (at least once per chunk)
/// and returns [`RunOutcome::Interrupted`] when asked to stop.
pub trait JobHandler: Send + Sync + 'static {
    fn run(&self, ctx: &JobContext) -> RunOutcome;
}

impl<F> JobHandler for F
where
    F: Fn(&JobContext) -> RunOutcome + Send + Sync + 'static,
{
    fn run(&self, ctx: &JobContext) -> RunOutcome {
        self(ctx)
    }
}

/// Wall clock in milliseconds since the Unix epoch.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// Current time in milliseconds since the Unix epoch.
pub fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Settings of a [`JobQueue`].
#[derive(Clone)]
pub struct QueueConfig {
    pub limits: Limits,
    pub backoff: Backoff,
    /// Minimal interval between persisted progress reports of one job
    /// (checkpoints are always written).
    pub progress_interval: Duration,
    /// How often the background scheduler wakes up without a reason (due
    /// retries, scheduled jobs).
    pub tick_interval: Duration,
    pub clock: Clock,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            backoff: Backoff::default(),
            progress_interval: Duration::from_millis(500),
            tick_interval: Duration::from_secs(1),
            clock: Arc::new(system_now_ms),
        }
    }
}

impl fmt::Debug for QueueConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueConfig")
            .field("limits", &self.limits)
            .field("backoff", &self.backoff)
            .field("progress_interval", &self.progress_interval)
            .field("tick_interval", &self.tick_interval)
            .finish_non_exhaustive()
    }
}

const STOP_NONE: u8 = 0;
const STOP_PAUSE: u8 = 1;
const STOP_CANCEL: u8 = 2;
const STOP_SHUTDOWN: u8 = 3;

/// Stop request of one run.
#[derive(Debug, Default)]
struct Control {
    stop: AtomicU8,
}

impl Control {
    fn request(&self, what: u8) {
        // A stronger request wins: cancel over pause.
        self.stop.fetch_max(what, Ordering::SeqCst);
    }
}

struct Inner {
    store: Mutex<JobStore>,
    handlers: RwLock<HashMap<String, Arc<dyn JobHandler>>>,
    limits: RwLock<Limits>,
    conditions: Mutex<Conditions>,
    gate: RwLock<Option<Arc<dyn ResourceGate>>>,
    active: Mutex<HashMap<JobId, Arc<Control>>>,
    active_changed: Condvar,
    subscribers: Mutex<Vec<Sender<JobEvent>>>,
    wake: Mutex<bool>,
    wake_cv: Condvar,
    shutting_down: AtomicBool,
    backoff: Backoff,
    progress_interval: Duration,
    tick_interval: Duration,
    clock: Clock,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// Sends the store's pending events to subscribers. Called with the
    /// store locked, so events keep their order.
    fn publish(&self, store: &mut JobStore) {
        let events = store.take_events();
        if events.is_empty() {
            return;
        }
        let mut subs = lock(&self.subscribers);
        subs.retain(|tx| events.iter().all(|e| tx.send(e.clone()).is_ok()));
    }

    fn with_store<T>(&self, f: impl FnOnce(&mut JobStore) -> Result<T, Error>) -> Result<T, Error> {
        let mut store = lock(&self.store);
        let result = f(&mut store);
        self.publish(&mut store);
        result
    }

    fn signal(&self, id: JobId, what: u8) {
        if let Some(control) = lock(&self.active).get(&id) {
            control.request(what);
        }
    }

    fn wake(&self) {
        *lock(&self.wake) = true;
        self.wake_cv.notify_all();
    }
}

/// The persistent job queue: one per library database and process.
///
/// ```
/// use pa_jobs::{JobQueue, JobState, JobStore, NewJob, QueueConfig, RunOutcome, serde_json::json};
/// use std::time::Duration;
///
/// let queue = JobQueue::new(JobStore::open_in_memory().unwrap(), QueueConfig::default()).unwrap();
/// queue.register("cleanup", |_ctx: &pa_jobs::JobContext| RunOutcome::Done(None));
/// let id = queue.enqueue(&NewJob::new("cleanup", json!({}))).unwrap();
/// assert!(queue.run_until_idle(Duration::from_secs(5)).unwrap());
/// assert_eq!(queue.get(id).unwrap().state, JobState::Done);
/// ```
#[derive(Clone)]
pub struct JobQueue {
    inner: Arc<Inner>,
}

impl fmt::Debug for JobQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobQueue").finish_non_exhaustive()
    }
}

impl JobQueue {
    /// Opens the library database at `path` and recovers jobs a previous
    /// process left running.
    pub fn open(path: impl AsRef<Path>, config: QueueConfig) -> Result<Self, Error> {
        Self::new(JobStore::open(path)?, config)
    }

    /// Wraps a store and recovers jobs a previous process left running
    /// ([`JobStore::recover`]).
    pub fn new(mut store: JobStore, config: QueueConfig) -> Result<Self, Error> {
        store.recover((config.clock)())?;
        // Recovery events have no subscribers yet.
        store.take_events();
        Ok(Self {
            inner: Arc::new(Inner {
                store: Mutex::new(store),
                handlers: RwLock::new(HashMap::new()),
                limits: RwLock::new(config.limits),
                conditions: Mutex::new(Conditions::default()),
                gate: RwLock::new(None),
                active: Mutex::new(HashMap::new()),
                active_changed: Condvar::new(),
                subscribers: Mutex::new(Vec::new()),
                wake: Mutex::new(false),
                wake_cv: Condvar::new(),
                shutting_down: AtomicBool::new(false),
                backoff: config.backoff,
                progress_interval: config.progress_interval,
                tick_interval: config.tick_interval,
                clock: config.clock,
            }),
        })
    }

    /// Registers the handler of a job type. Jobs of types without a handler
    /// stay queued (a store build has no Twitch handlers, ADR-0007).
    pub fn register(&self, kind: impl Into<String>, handler: impl JobHandler) {
        self.inner
            .handlers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(kind.into(), Arc::new(handler));
        self.inner.wake();
    }

    /// Sets the concurrency limit of a type (0 disables it).
    pub fn set_limit(&self, kind: impl Into<String>, limit: usize) {
        self.inner
            .limits
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .set(kind, limit);
        self.inner.wake();
    }

    /// Reports the device state (network, battery). Running jobs whose
    /// requirements are no longer met are paused on the next tick and
    /// resumed when the condition is back.
    pub fn set_conditions(&self, conditions: Conditions) {
        *lock(&self.inner.conditions) = conditions;
        self.inner.wake();
    }

    pub fn conditions(&self) -> Conditions {
        *lock(&self.inner.conditions)
    }

    /// Sets the admission check (storage quotas, CORE-012).
    pub fn set_gate(&self, gate: Option<Arc<dyn ResourceGate>>) {
        *self.inner.gate.write().unwrap_or_else(|e| e.into_inner()) = gate;
        self.inner.wake();
    }

    /// A channel of all events from now on. Dropping the receiver
    /// unsubscribes.
    pub fn subscribe(&self) -> Receiver<JobEvent> {
        let (tx, rx) = mpsc::channel();
        lock(&self.inner.subscribers).push(tx);
        rx
    }

    pub fn enqueue(&self, job: &NewJob) -> Result<JobId, Error> {
        let now = self.inner.now();
        let id = self.inner.with_store(|s| s.enqueue(job, now))?;
        self.inner.wake();
        Ok(id)
    }

    pub fn get(&self, id: JobId) -> Result<Job, Error> {
        lock(&self.inner.store).get(id)
    }

    /// Jobs in any of `states` (all if empty), in scheduling order.
    pub fn list(&self, states: &[JobState]) -> Result<Vec<Job>, Error> {
        lock(&self.inner.store).list(states)
    }

    /// Pauses a job on behalf of the user. A running handler is asked to
    /// stop; the job becomes `paused` when it returns.
    pub fn pause(&self, id: JobId) -> Result<Job, Error> {
        self.pause_with(id, PauseReason::User)
    }

    /// Pauses a job with a reason; `pa-storage` uses
    /// [`PauseReason::Resources`] when free space crosses the reserve.
    pub fn pause_with(&self, id: JobId, reason: PauseReason) -> Result<Job, Error> {
        let now = self.inner.now();
        let job = self.inner.with_store(|s| s.pause(id, reason, now))?;
        if job.state == JobState::Running {
            self.inner.signal(id, STOP_PAUSE);
        }
        Ok(job)
    }

    pub fn resume(&self, id: JobId) -> Result<Job, Error> {
        let now = self.inner.now();
        let job = self.inner.with_store(|s| s.resume(id, now))?;
        self.inner.wake();
        Ok(job)
    }

    /// Cancels a job; a running handler is asked to stop.
    pub fn cancel(&self, id: JobId) -> Result<Job, Error> {
        let now = self.inner.now();
        let job = self.inner.with_store(|s| s.cancel(id, now))?;
        self.inner.signal(id, STOP_CANCEL);
        self.inner.wake();
        Ok(job)
    }

    /// Queues a failed job again.
    pub fn retry(&self, id: JobId) -> Result<Job, Error> {
        let now = self.inner.now();
        let job = self.inner.with_store(|s| s.retry(id, now))?;
        self.inner.wake();
        Ok(job)
    }

    pub fn set_priority(&self, id: JobId, priority: i64) -> Result<(), Error> {
        let now = self.inner.now();
        self.inner
            .with_store(|s| s.set_priority(id, priority, now))?;
        self.inner.wake();
        Ok(())
    }

    /// Deletes a finished job.
    pub fn remove(&self, id: JobId) -> Result<(), Error> {
        self.inner.with_store(|s| s.remove(id))
    }

    /// Deletes `done` and `cancelled` jobs older than `age`.
    pub fn purge_finished(&self, age: Duration) -> Result<usize, Error> {
        let before = self
            .inner
            .now()
            .saturating_sub(i64::try_from(age.as_millis()).unwrap_or(i64::MAX));
        self.inner.with_store(|s| s.purge_finished(before))
    }

    /// Number of handlers running now.
    pub fn active_count(&self) -> usize {
        lock(&self.inner.active).len()
    }

    /// One scheduling pass: pauses and resumes jobs by conditions and
    /// starts what may start. Returns how many handlers were started.
    pub fn tick(&self) -> Result<usize, Error> {
        if self.inner.shutting_down.load(Ordering::SeqCst) {
            return Ok(0);
        }
        let inner = &self.inner;
        let handlers = inner
            .handlers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let limits = inner
            .limits
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let gate = inner.gate.read().unwrap_or_else(|e| e.into_inner()).clone();
        let conditions = *lock(&inner.conditions);
        let mut store = lock(&inner.store);
        let mut active = lock(&inner.active);
        let busy: HashSet<JobId> = active.keys().copied().collect();
        let can_run = |kind: &str| handlers.contains_key(kind);
        let input = ScheduleInput {
            conditions,
            limits: &limits,
            gate: gate.as_deref(),
            can_run: &can_run,
            busy: &busy,
        };
        let schedule = store.schedule(&input, inner.now());
        inner.publish(&mut store);
        let schedule = schedule?;
        for (id, _) in &schedule.interrupt {
            if let Some(control) = active.get(id) {
                control.request(STOP_PAUSE);
            }
        }
        let started = schedule.started.len();
        for job in schedule.started {
            let Some(handler) = handlers.get(&job.kind).cloned() else {
                continue;
            };
            let control = Arc::new(Control::default());
            active.insert(job.id, Arc::clone(&control));
            let worker_inner = Arc::clone(inner);
            let id = job.id;
            let spawned = thread::Builder::new()
                .name(format!("pa-jobs-{id}"))
                .spawn(move || run_job(worker_inner, handler, control, job));
            if spawned.is_err() {
                active.remove(&id);
                let outcome = RunOutcome::Retryable("cannot start a worker thread".into());
                let _ = store.finish(id, &outcome, &inner.backoff, inner.now());
                inner.publish(&mut store);
            }
        }
        Ok(started)
    }

    /// Ticks until no handler runs and nothing more can start, or until
    /// `timeout`. Returns true if the queue became idle. Retries due later
    /// do not keep it busy.
    pub fn run_until_idle(&self, timeout: Duration) -> Result<bool, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            let started = self.tick()?;
            let active = lock(&self.inner.active);
            if active.is_empty() && started == 0 {
                return Ok(true);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            if !active.is_empty() {
                let wait = (deadline - now).min(Duration::from_millis(50));
                let _ = self
                    .inner
                    .active_changed
                    .wait_timeout(active, wait)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    /// Starts the background scheduler thread. It ticks on every change and
    /// every [`QueueConfig::tick_interval`]. Dropping the driver stops it
    /// (running handlers continue; see [`shutdown`](Self::shutdown)).
    pub fn start(&self) -> QueueDriver {
        let stop = Arc::new(AtomicBool::new(false));
        let queue = self.clone();
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("pa-jobs-scheduler".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    let _ = queue.tick();
                    let inner = &queue.inner;
                    let guard = lock(&inner.wake);
                    let (mut guard, _) = inner
                        .wake_cv
                        .wait_timeout_while(guard, inner.tick_interval, |woken| {
                            !*woken && !thread_stop.load(Ordering::SeqCst)
                        })
                        .unwrap_or_else(|e| e.into_inner());
                    *guard = false;
                }
            })
            .ok();
        QueueDriver {
            stop,
            inner: Arc::clone(&self.inner),
            thread,
        }
    }

    /// Stops scheduling, asks running handlers to stop and waits up to
    /// `timeout` for them. Interrupted jobs go back to `queued` and resume
    /// on the next start. Returns true if all handlers returned.
    pub fn shutdown(&self, timeout: Duration) -> bool {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        self.inner.wake();
        let deadline = Instant::now() + timeout;
        let mut active = lock(&self.inner.active);
        for control in active.values() {
            control.request(STOP_SHUTDOWN);
        }
        while !active.is_empty() {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            active = self
                .inner
                .active_changed
                .wait_timeout(active, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

fn run_job(inner: Arc<Inner>, handler: Arc<dyn JobHandler>, control: Arc<Control>, job: Job) {
    let id = job.id;
    let ctx = JobContext {
        job,
        inner: Arc::clone(&inner),
        control,
        last_report: Mutex::new(None),
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| handler.run(&ctx)))
        .unwrap_or_else(|_| RunOutcome::Fatal("handler panicked".into()));
    {
        let mut store = lock(&inner.store);
        let _ = store.finish(id, &outcome, &inner.backoff, inner.now());
        inner.publish(&mut store);
        lock(&inner.active).remove(&id);
    }
    inner.active_changed.notify_all();
    inner.wake();
}

/// Background scheduler of a [`JobQueue`]; stops when dropped.
pub struct QueueDriver {
    stop: Arc<AtomicBool>,
    inner: Arc<Inner>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for QueueDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueDriver").finish_non_exhaustive()
    }
}

impl Drop for QueueDriver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.inner.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What a handler sees of its job and how it reports back.
pub struct JobContext {
    job: Job,
    inner: Arc<Inner>,
    control: Arc<Control>,
    last_report: Mutex<Option<Instant>>,
}

impl fmt::Debug for JobContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobContext")
            .field("job", &self.job)
            .finish_non_exhaustive()
    }
}

impl JobContext {
    /// The job as it was when this run started (`attempts` includes this
    /// run).
    pub fn job(&self) -> &Job {
        &self.job
    }

    pub fn params(&self) -> &Value {
        &self.job.params
    }

    /// Checkpoint saved by an earlier run, if any.
    pub fn checkpoint(&self) -> Option<&Value> {
        self.job.checkpoint.as_ref()
    }

    /// True once the job was paused, cancelled or the queue shuts down: the
    /// handler should save a checkpoint and return
    /// [`RunOutcome::Interrupted`].
    pub fn stop_requested(&self) -> bool {
        self.control.stop.load(Ordering::SeqCst) != STOP_NONE
    }

    /// True if the job was cancelled: there is no point in a checkpoint.
    pub fn is_cancelled(&self) -> bool {
        self.control.stop.load(Ordering::SeqCst) == STOP_CANCEL
    }

    fn due(&self, force: bool) -> bool {
        let mut last = lock(&self.last_report);
        let now = Instant::now();
        let due =
            force || last.is_none_or(|t| now.duration_since(t) >= self.inner.progress_interval);
        if due {
            *last = Some(now);
        }
        due
    }

    /// Reports bytes done (and the total, once known). Throttled to
    /// [`QueueConfig::progress_interval`]; the final report of a byte job
    /// (`done == total`) always goes through.
    pub fn report_bytes(&self, done: u64, total: Option<u64>) {
        if self.due(total == Some(done)) {
            let now = self.inner.now();
            let _ = self
                .inner
                .with_store(|s| s.report_progress(self.job.id, None, done, total, now));
        }
    }

    /// Reports progress as a fraction 0.0..=1.0 (jobs without bytes:
    /// scans, metadata). Throttled like [`report_bytes`](Self::report_bytes).
    pub fn report_progress(&self, fraction: f64) {
        if self.due(fraction >= 1.0) {
            let now = self.inner.now();
            let _ = self.inner.with_store(|s| {
                let done = s.get(self.job.id)?.bytes_done;
                s.report_progress(self.job.id, Some(fraction), done, None, now)
            });
        }
    }

    /// Saves the state to resume from and `bytes_done`, immediately. Returns
    /// false if the job is no longer running (cancelled): stop.
    pub fn save_checkpoint(&self, checkpoint: &Value, bytes_done: u64) -> Result<bool, Error> {
        let now = self.inner.now();
        self.inner
            .with_store(|s| s.save_checkpoint(self.job.id, checkpoint, bytes_done, now))
    }
}
