// SPDX-License-Identifier: MPL-2.0
//! Behaviour of `JobQueue` with real worker threads: priorities, limits,
//! retries, pause, cancel, conditions, the resource gate and events.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pa_jobs::serde_json::{Value, json};
use pa_jobs::{
    Admission, Backoff, Conditions, Job, JobContext, JobEvent, JobQueue, JobState, JobStore,
    Network, NetworkPolicy, NewJob, PauseReason, QueueConfig, ResourceGate, RunOutcome, priority,
};

const WAIT: Duration = Duration::from_secs(20);

fn queue() -> JobQueue {
    let config = QueueConfig {
        backoff: Backoff {
            base: Duration::ZERO,
            max: Duration::ZERO,
        },
        progress_interval: Duration::ZERO,
        tick_interval: Duration::from_millis(10),
        ..QueueConfig::default()
    };
    JobQueue::new(JobStore::open_in_memory().unwrap(), config).unwrap()
}

fn job(kind: &str) -> NewJob {
    NewJob::new(kind, json!({}))
}

fn wait_for(queue: &JobQueue, id: i64, what: impl Fn(&Job) -> bool) -> Job {
    let deadline = Instant::now() + WAIT;
    loop {
        let job = queue.get(id).unwrap();
        if what(&job) {
            return job;
        }
        assert!(Instant::now() < deadline, "timeout: {job:?}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A handler that runs until it is asked to stop, counting its runs.
fn until_stopped(runs: Arc<AtomicU32>) -> impl Fn(&JobContext) -> RunOutcome + Send + Sync {
    move |ctx: &JobContext| {
        runs.fetch_add(1, Ordering::SeqCst);
        while !ctx.stop_requested() {
            thread::sleep(Duration::from_millis(2));
        }
        RunOutcome::Interrupted
    }
}

#[test]
fn runs_by_priority_within_the_limit() {
    let q = queue();
    q.set_limit("work", 1);
    let order = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&order);
    let low = q.enqueue(&job("work").priority(priority::BACKGROUND)).unwrap();
    let high = q.enqueue(&job("work").priority(priority::USER_WAITING)).unwrap();
    let low2 = q.enqueue(&job("work").priority(priority::BACKGROUND)).unwrap();
    q.register("work", move |ctx: &JobContext| {
        seen.lock().unwrap().push(ctx.job().id);
        thread::sleep(Duration::from_millis(5));
        RunOutcome::Done(Some(json!({ "ok": true })))
    });
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(*order.lock().unwrap(), vec![high, low, low2]);
    let done = q.get(low).unwrap();
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.result, Some(json!({ "ok": true })));
    assert_eq!(done.progress, Some(1.0));
}

#[test]
fn limit_holds_jobs_in_waiting_resources() {
    let q = queue();
    q.set_limit("scan", 1);
    let runs = Arc::new(AtomicU32::new(0));
    q.register("scan", until_stopped(Arc::clone(&runs)));
    let a = q.enqueue(&job("scan")).unwrap();
    let b = q.enqueue(&job("scan")).unwrap();
    q.tick().unwrap();
    assert_eq!(q.get(a).unwrap().state, JobState::Running);
    let waiting = q.get(b).unwrap();
    assert_eq!(waiting.state, JobState::WaitingResources);
    assert_eq!(waiting.wait_reason.as_deref(), Some("limit"));
    q.cancel(a).unwrap();
    wait_for(&q, b, |j| {
        q.tick().unwrap();
        j.state == JobState::Running
    });
    q.cancel(b).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[test]
fn live_recording_ignores_the_limit() {
    let q = queue();
    q.set_limit("record", 0);
    let runs = Arc::new(AtomicU32::new(0));
    q.register("record", until_stopped(Arc::clone(&runs)));
    let background = q.enqueue(&job("record")).unwrap();
    let live = q.enqueue(&job("record").priority(priority::LIVE)).unwrap();
    q.tick().unwrap();
    assert_eq!(q.get(live).unwrap().state, JobState::Running);
    assert_eq!(q.get(background).unwrap().state, JobState::WaitingResources);
    q.cancel(live).unwrap();
    q.cancel(background).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
}

#[test]
fn retries_then_succeeds() {
    let q = queue();
    let calls = Arc::new(AtomicU32::new(0));
    let c = Arc::clone(&calls);
    q.register("fetch", move |_: &JobContext| {
        if c.fetch_add(1, Ordering::SeqCst) < 2 {
            RunOutcome::Retryable("timeout".into())
        } else {
            RunOutcome::Done(None)
        }
    });
    let id = q.enqueue(&job("fetch").max_attempts(3)).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    let done = q.get(id).unwrap();
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.attempts, 3);
    assert_eq!(done.error.as_deref(), Some("timeout"));
}

#[test]
fn retries_are_exhausted_then_user_retries() {
    let q = queue();
    let fail = Arc::new(Mutex::new(true));
    let f = Arc::clone(&fail);
    q.register("fetch", move |_: &JobContext| {
        if *f.lock().unwrap() {
            RunOutcome::Retryable("503".into())
        } else {
            RunOutcome::Done(None)
        }
    });
    let id = q.enqueue(&job("fetch").max_attempts(2)).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    let failed = q.get(id).unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.attempts, 2);

    *fail.lock().unwrap() = false;
    q.retry(id).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.get(id).unwrap().state, JobState::Done);
}

#[test]
fn backoff_delays_the_next_run() {
    let now = Arc::new(Mutex::new(1_000_000i64));
    let clock_now = Arc::clone(&now);
    let config = QueueConfig {
        backoff: Backoff {
            base: Duration::from_secs(10),
            max: Duration::from_secs(60),
        },
        clock: Arc::new(move || *clock_now.lock().unwrap()),
        ..QueueConfig::default()
    };
    let q = JobQueue::new(JobStore::open_in_memory().unwrap(), config).unwrap();
    q.register("fetch", |_: &JobContext| RunOutcome::Retryable("down".into()));
    let id = q.enqueue(&job("fetch")).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    let job = q.get(id).unwrap();
    assert_eq!(job.state, JobState::Retrying);
    assert_eq!(job.next_run_at_ms, Some(1_010_000));
    // Not due yet.
    assert_eq!(q.tick().unwrap(), 0);
    *now.lock().unwrap() += 10_000;
    assert!(q.run_until_idle(WAIT).unwrap());
    let job = q.get(id).unwrap();
    assert_eq!(job.attempts, 2);
    assert_eq!(job.next_run_at_ms, Some(1_030_000), "second delay doubles");
}

#[test]
fn fatal_error_and_panic_fail_at_once() {
    let q = queue();
    q.register("gone", |_: &JobContext| RunOutcome::Fatal("VOD deleted".into()));
    q.register("bug", |_: &JobContext| -> RunOutcome { panic!("boom") });
    let gone = q.enqueue(&job("gone")).unwrap();
    let bug = q.enqueue(&job("bug")).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    let gone = q.get(gone).unwrap();
    assert_eq!(gone.state, JobState::Failed);
    assert_eq!(gone.attempts, 1);
    assert_eq!(gone.error.as_deref(), Some("VOD deleted"));
    assert_eq!(q.get(bug).unwrap().state, JobState::Failed);
}

#[test]
fn cancel_stops_a_running_handler() {
    let q = queue();
    let runs = Arc::new(AtomicU32::new(0));
    q.register("copy", until_stopped(Arc::clone(&runs)));
    let id = q.enqueue(&job("copy")).unwrap();
    q.tick().unwrap();
    assert_eq!(q.get(id).unwrap().state, JobState::Running);
    q.cancel(id).unwrap();
    assert_eq!(q.get(id).unwrap().state, JobState::Cancelled);
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.get(id).unwrap().state, JobState::Cancelled);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(q.cancel(id).is_err(), "cancelled is final");
}

#[test]
fn pause_and_resume_continue_from_the_checkpoint() {
    let q = queue();
    let starts = Arc::new(Mutex::new(Vec::new()));
    let s = Arc::clone(&starts);
    q.register("copy", move |ctx: &JobContext| {
        let mut offset = ctx
            .checkpoint()
            .and_then(|c| c["offset"].as_u64())
            .unwrap_or(0);
        s.lock().unwrap().push(offset);
        while offset < 100 {
            if ctx.stop_requested() {
                return RunOutcome::Interrupted;
            }
            offset += 1;
            ctx.save_checkpoint(&json!({ "offset": offset }), offset)
                .unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        RunOutcome::Done(None)
    });
    let id = q.enqueue(&job("copy").bytes_total(100)).unwrap();
    let driver = q.start();
    wait_for(&q, id, |j| j.bytes_done >= 10);
    let paused = q.pause(id).unwrap();
    assert_eq!(paused.pause_reason, Some(PauseReason::User));
    let paused = wait_for(&q, id, |j| j.state == JobState::Paused);
    assert_eq!(paused.attempts, 0, "a paused run is not an attempt");
    thread::sleep(Duration::from_millis(50));
    assert_eq!(q.get(id).unwrap().state, JobState::Paused, "stays paused");

    q.resume(id).unwrap();
    let done = wait_for(&q, id, |j| j.state == JobState::Done);
    drop(driver);
    let starts = starts.lock().unwrap();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0], 0);
    assert!(starts[1] >= 10);
    assert_eq!(done.bytes_done, 100);
}

#[test]
fn wifi_only_waits_and_pauses_with_the_network() {
    let q = queue();
    let runs = Arc::new(AtomicU32::new(0));
    q.register("vod", until_stopped(Arc::clone(&runs)));
    q.set_conditions(Conditions {
        network: Network::Metered,
        on_battery: false,
    });
    let id = q
        .enqueue(&job("vod").network(NetworkPolicy::Unmetered))
        .unwrap();
    q.tick().unwrap();
    let waiting = q.get(id).unwrap();
    assert_eq!(waiting.state, JobState::WaitingResources);
    assert_eq!(waiting.wait_reason.as_deref(), Some("network"));

    let driver = q.start();
    q.set_conditions(Conditions::default());
    wait_for(&q, id, |j| j.state == JobState::Running);

    // Wi-Fi lost while running: paused by the scheduler, not the user.
    q.set_conditions(Conditions {
        network: Network::Offline,
        on_battery: false,
    });
    let paused = wait_for(&q, id, |j| j.state == JobState::Paused);
    assert_eq!(paused.pause_reason, Some(PauseReason::Network));

    // Back on Wi-Fi: resumed automatically.
    q.set_conditions(Conditions::default());
    wait_for(&q, id, |j| {
        j.state == JobState::Running && runs.load(Ordering::SeqCst) == 2
    });
    q.cancel(id).unwrap();
    drop(driver);
    assert!(q.run_until_idle(WAIT).unwrap());
}

#[test]
fn battery_holds_jobs_that_need_power() {
    let q = queue();
    q.register("transcode", |_: &JobContext| RunOutcome::Done(None));
    q.set_conditions(Conditions {
        network: Network::Unmetered,
        on_battery: true,
    });
    let heavy = q
        .enqueue(&job("transcode").allow_on_battery(false))
        .unwrap();
    let light = q.enqueue(&job("transcode")).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.get(light).unwrap().state, JobState::Done);
    let held = q.get(heavy).unwrap();
    assert_eq!(held.state, JobState::WaitingResources);
    assert_eq!(held.wait_reason.as_deref(), Some("battery"));
    q.set_conditions(Conditions::default());
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.get(heavy).unwrap().state, JobState::Done);
}

struct SpaceGate(Mutex<bool>);

impl ResourceGate for SpaceGate {
    fn admit(&self, _: &Job) -> Admission {
        if *self.0.lock().unwrap() {
            Admission::Admit
        } else {
            Admission::Wait("storage_reserve".into())
        }
    }
}

#[test]
fn resource_gate_holds_and_pauses() {
    let q = queue();
    let gate = Arc::new(SpaceGate(Mutex::new(false)));
    q.set_gate(Some(Arc::clone(&gate) as Arc<dyn ResourceGate>));
    let runs = Arc::new(AtomicU32::new(0));
    q.register("rec", until_stopped(Arc::clone(&runs)));
    let id = q.enqueue(&job("rec")).unwrap();
    q.tick().unwrap();
    let held = q.get(id).unwrap();
    assert_eq!(held.state, JobState::WaitingResources);
    assert_eq!(held.wait_reason.as_deref(), Some("storage_reserve"));

    *gate.0.lock().unwrap() = true;
    q.tick().unwrap();
    assert_eq!(q.get(id).unwrap().state, JobState::Running);

    // The reserve is crossed while running (pa-storage): pause, then
    // resume when the gate admits again.
    *gate.0.lock().unwrap() = false;
    q.pause_with(id, PauseReason::Resources).unwrap();
    wait_for(&q, id, |j| {
        q.tick().unwrap();
        j.state == JobState::WaitingResources
    });
    *gate.0.lock().unwrap() = true;
    wait_for(&q, id, |j| {
        q.tick().unwrap();
        j.state == JobState::Running
    });
    q.cancel(id).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
}

#[test]
fn jobs_without_a_handler_stay_queued() {
    let q = queue();
    let id = q.enqueue(&job("twitch_vod_download")).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.get(id).unwrap().state, JobState::Queued);
}

#[test]
fn events_follow_the_job() {
    let q = queue();
    let events = q.subscribe();
    q.register("copy", |ctx: &JobContext| {
        ctx.report_bytes(50, Some(100));
        ctx.report_bytes(100, Some(100));
        RunOutcome::Done(None)
    });
    let id = q.enqueue(&job("copy")).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    let got: Vec<JobEvent> = events.try_iter().collect();
    let kind = "copy".to_owned();
    assert_eq!(
        got,
        vec![
            JobEvent::Added {
                id,
                kind: kind.clone()
            },
            JobEvent::StateChanged {
                id,
                kind: kind.clone(),
                from: JobState::Queued,
                to: JobState::Running
            },
            JobEvent::Progress {
                id,
                progress: Some(0.5),
                bytes_done: 50,
                bytes_total: Some(100)
            },
            JobEvent::Progress {
                id,
                progress: Some(1.0),
                bytes_done: 100,
                bytes_total: Some(100)
            },
            JobEvent::StateChanged {
                id,
                kind,
                from: JobState::Running,
                to: JobState::Done
            },
        ]
    );
    q.remove(id).unwrap();
    assert_eq!(events.try_recv().unwrap(), JobEvent::Removed { id });
}

#[test]
fn rejects_bad_input() {
    let q = queue();
    assert!(q.enqueue(&job("Bad-Kind")).is_err());
    assert!(q.enqueue(&NewJob::new("scan", Value::Null)).is_err());
    assert!(q.enqueue(&job("scan").max_attempts(0)).is_err());
    let id = q.enqueue(&job("scan")).unwrap();
    assert!(q.remove(id).is_err(), "only finished jobs are removed");
    assert!(q.resume(id).is_err(), "not paused");
    assert!(q.get(id + 100).is_err());
}

#[test]
fn scheduled_job_waits_for_its_time() {
    let q = queue();
    q.register("cleanup", |_: &JobContext| RunOutcome::Done(None));
    let later = q
        .enqueue(&job("cleanup").not_before_ms(pa_jobs::system_now_ms() + 3_600_000))
        .unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.get(later).unwrap().state, JobState::Queued);
}

#[test]
fn purge_removes_old_finished_jobs() {
    let q = queue();
    q.register("cleanup", |_: &JobContext| RunOutcome::Done(None));
    let id = q.enqueue(&job("cleanup")).unwrap();
    assert!(q.run_until_idle(WAIT).unwrap());
    assert_eq!(q.purge_finished(Duration::from_secs(3600)).unwrap(), 0);
    thread::sleep(Duration::from_millis(5));
    assert_eq!(q.purge_finished(Duration::ZERO).unwrap(), 1);
    assert!(q.get(id).is_err());
}
