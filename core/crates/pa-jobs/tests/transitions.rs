// SPDX-License-Identifier: MPL-2.0
//! CORE-011 "done when": property test of state transitions.
//!
//! Random sequences of operations (enqueue, scheduling passes, handler
//! outcomes, pause, resume, cancel, retry, network and battery changes,
//! gate changes, restarts) run against `JobStore`. After every step:
//!
//! - every recorded transition is an allowed edge and starts from the state
//!   the job really had;
//! - the stored state of every job equals the state the events lead to;
//! - `done` and `cancelled` never change;
//! - `attempts <= max_attempts`; a failed job has an error;
//! - running non-live jobs of a type never exceed its limit;
//! - a running job whose requirements are not met has a pending pause;
//! - after a restart nothing is running and checkpoints are kept.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use pa_jobs::serde_json::json;
use pa_jobs::{
    Admission, Backoff, Conditions, Error, Job, JobEvent, JobId, JobState, JobStore, Limits,
    Network, NetworkPolicy, NewJob, PauseReason, RunOutcome, ScheduleInput, priority,
};
use proptest::prelude::*;

const KINDS: [&str; 3] = ["scan", "copy", "record"];

#[derive(Debug, Clone)]
enum Op {
    Enqueue {
        kind: usize,
        priority: usize,
        network: usize,
        allow_on_battery: bool,
        max_attempts: u32,
    },
    Tick,
    Finish {
        pick: usize,
        outcome: usize,
    },
    Checkpoint {
        pick: usize,
        bytes: u64,
    },
    Pause {
        pick: usize,
    },
    Resume {
        pick: usize,
    },
    Cancel {
        pick: usize,
    },
    Retry {
        pick: usize,
    },
    Conditions {
        network: usize,
        on_battery: bool,
    },
    Gate {
        admit: bool,
    },
    Restart,
    Wait {
        ms: i64,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..3usize, 0..3usize, 0..3usize, any::<bool>(), 1..4u32).prop_map(
            |(kind, priority, network, allow_on_battery, max_attempts)| Op::Enqueue {
                kind,
                priority,
                network,
                allow_on_battery,
                max_attempts,
            }
        ),
        4 => Just(Op::Tick),
        4 => (any::<usize>(), 0..4usize).prop_map(|(pick, outcome)| Op::Finish { pick, outcome }),
        1 => (any::<usize>(), 0..1000u64).prop_map(|(pick, bytes)| Op::Checkpoint { pick, bytes }),
        1 => any::<usize>().prop_map(|pick| Op::Pause { pick }),
        1 => any::<usize>().prop_map(|pick| Op::Resume { pick }),
        1 => any::<usize>().prop_map(|pick| Op::Cancel { pick }),
        1 => any::<usize>().prop_map(|pick| Op::Retry { pick }),
        1 => (0..3usize, any::<bool>())
            .prop_map(|(network, on_battery)| Op::Conditions { network, on_battery }),
        1 => any::<bool>().prop_map(|admit| Op::Gate { admit }),
        1 => Just(Op::Restart),
        1 => (0..20_000i64).prop_map(|ms| Op::Wait { ms }),
    ]
}

struct World {
    store: JobStore,
    now: i64,
    conditions: Conditions,
    admit: bool,
    limits: Limits,
    backoff: Backoff,
    /// State of each job according to the events.
    seen: HashMap<JobId, JobState>,
    finals: HashSet<JobId>,
    checkpoints: HashMap<JobId, u64>,
}

fn pick(ids: &[JobId], n: usize) -> Option<JobId> {
    (!ids.is_empty()).then(|| ids[n % ids.len()])
}

/// Errors a user operation may legitimately return.
fn expected(result: Result<Job, Error>) {
    match result {
        Ok(_) | Err(Error::InvalidTransition { .. }) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

impl World {
    fn new() -> Self {
        let mut limits = Limits::default();
        limits.set("scan", 1);
        limits.set("copy", 2);
        limits.set("record", 0);
        Self {
            store: JobStore::open_in_memory().unwrap(),
            now: 1_000_000,
            conditions: Conditions::default(),
            admit: true,
            limits,
            backoff: Backoff {
                base: Duration::from_secs(1),
                max: Duration::from_secs(8),
            },
            seen: HashMap::new(),
            finals: HashSet::new(),
            checkpoints: HashMap::new(),
        }
    }

    fn ids(&self, states: &[JobState]) -> Vec<JobId> {
        self.store
            .list(states)
            .unwrap()
            .into_iter()
            .map(|j| j.id)
            .collect()
    }

    fn schedule(&mut self) {
        let admit = self.admit;
        let gate = move |_: &Job| {
            if admit {
                Admission::Admit
            } else {
                Admission::Wait("space".into())
            }
        };
        let busy = HashSet::new();
        let can_run = |_: &str| true;
        let input = ScheduleInput {
            conditions: self.conditions,
            limits: &self.limits,
            gate: Some(&gate),
            can_run: &can_run,
            busy: &busy,
        };
        let schedule = self.store.schedule(&input, self.now).unwrap();
        for job in &schedule.started {
            assert_eq!(job.state, JobState::Running);
            assert!(job.attempts >= 1);
        }
        for (id, reason) in &schedule.interrupt {
            let job = self.store.get(*id).unwrap();
            assert_eq!(job.state, JobState::Running);
            assert_eq!(job.pause_reason, Some(*reason));
        }
    }

    fn apply(&mut self, op: &Op) {
        self.now += 1;
        match *op {
            Op::Enqueue {
                kind,
                priority: p,
                network,
                allow_on_battery,
                max_attempts,
            } => {
                let job = NewJob::new(KINDS[kind], json!({ "n": kind }))
                    .priority([priority::BACKGROUND, priority::USER_WAITING, priority::LIVE][p])
                    .network(
                        [
                            NetworkPolicy::None,
                            NetworkPolicy::Any,
                            NetworkPolicy::Unmetered,
                        ][network],
                    )
                    .allow_on_battery(allow_on_battery)
                    .max_attempts(max_attempts);
                self.store.enqueue(&job, self.now).unwrap();
            }
            Op::Tick => self.schedule(),
            Op::Finish { pick: n, outcome } => {
                if let Some(id) = pick(&self.ids(&[JobState::Running]), n) {
                    let outcome = match outcome {
                        0 => RunOutcome::Done(Some(json!({ "ok": true }))),
                        1 => RunOutcome::Retryable("net".into()),
                        2 => RunOutcome::Fatal("gone".into()),
                        _ => RunOutcome::Interrupted,
                    };
                    let to = self
                        .store
                        .finish(id, &outcome, &self.backoff, self.now)
                        .unwrap();
                    assert!(to.is_some(), "a running job takes the outcome");
                }
            }
            Op::Checkpoint { pick: n, bytes } => {
                if let Some(id) = pick(&self.ids(&[JobState::Running]), n) {
                    assert!(
                        self.store
                            .save_checkpoint(id, &json!({ "offset": bytes }), bytes, self.now)
                            .unwrap()
                    );
                    self.checkpoints.insert(id, bytes);
                }
            }
            Op::Pause { pick: n } => {
                if let Some(id) = pick(&self.ids(&[]), n) {
                    expected(self.store.pause(id, PauseReason::User, self.now));
                }
            }
            Op::Resume { pick: n } => {
                if let Some(id) = pick(&self.ids(&[]), n) {
                    expected(self.store.resume(id, self.now));
                }
            }
            Op::Cancel { pick: n } => {
                if let Some(id) = pick(&self.ids(&[]), n) {
                    expected(self.store.cancel(id, self.now));
                }
            }
            Op::Retry { pick: n } => {
                if let Some(id) = pick(&self.ids(&[]), n) {
                    expected(self.store.retry(id, self.now));
                }
            }
            Op::Conditions {
                network,
                on_battery,
            } => {
                self.conditions = Conditions {
                    network: [Network::Offline, Network::Metered, Network::Unmetered][network],
                    on_battery,
                };
            }
            Op::Gate { admit } => self.admit = admit,
            Op::Restart => {
                let running_before = self.ids(&[JobState::Running]).len();
                let recovered = self.store.recover(self.now).unwrap();
                assert_eq!(recovered, running_before);
                assert!(self.ids(&[JobState::Running]).is_empty());
            }
            Op::Wait { ms } => self.now += ms,
        }
        self.check();
    }

    fn check(&mut self) {
        for event in self.store.take_events() {
            match event {
                JobEvent::Added { id, .. } => {
                    assert!(self.seen.insert(id, JobState::Queued).is_none());
                }
                JobEvent::StateChanged { id, from, to, .. } => {
                    assert!(from.can_transition_to(to), "{from} -> {to}");
                    assert_eq!(self.seen.get(&id), Some(&from), "job {id}");
                    self.seen.insert(id, to);
                }
                JobEvent::Progress { id, .. } => {
                    assert_eq!(self.seen.get(&id), Some(&JobState::Running));
                }
                JobEvent::Removed { id } => {
                    self.seen.remove(&id);
                }
            }
        }
        let jobs = self.store.list(&[]).unwrap();
        assert_eq!(jobs.len(), self.seen.len());
        let mut running: HashMap<&str, usize> = HashMap::new();
        for job in &jobs {
            assert_eq!(self.seen.get(&job.id), Some(&job.state), "{job:?}");
            if self.finals.contains(&job.id) {
                assert!(
                    matches!(job.state, JobState::Done | JobState::Cancelled),
                    "{job:?}"
                );
            }
            if matches!(job.state, JobState::Done | JobState::Cancelled) {
                self.finals.insert(job.id);
            }
            assert!(job.attempts <= job.max_attempts, "{job:?}");
            match job.state {
                JobState::Failed => assert!(job.error.is_some(), "{job:?}"),
                JobState::Done => assert_eq!(job.progress, Some(1.0)),
                JobState::Paused => assert!(job.pause_reason.is_some(), "{job:?}"),
                JobState::Running if job.priority < priority::LIVE => {
                    *running.entry(job.kind.as_str()).or_default() += 1;
                }
                JobState::WaitingResources => assert!(job.wait_reason.is_some()),
                _ => {}
            }
            if !job.state.is_terminal()
                && let Some(&bytes) = self.checkpoints.get(&job.id)
            {
                assert_eq!(job.checkpoint, Some(json!({ "offset": bytes })));
            }
        }
        for (kind, n) in running {
            assert!(n <= self.limits.get(kind), "{kind}: {n} running");
        }
    }

    /// After the last step: running jobs with unmet requirements are asked
    /// to pause by the next scheduling pass.
    fn check_pending_pauses(&mut self) {
        self.schedule();
        self.check();
        for job in self.store.list(&[JobState::Running]).unwrap() {
            if job.blocked_by(&self.conditions).is_some() {
                assert!(job.pause_reason.is_some(), "{job:?}");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn transitions_stay_valid(ops in prop::collection::vec(op(), 1..80)) {
        let mut world = World::new();
        for op in &ops {
            world.apply(op);
        }
        world.check_pending_pauses();
    }
}

#[test]
fn every_state_is_reachable() {
    // Smoke check that the generator covers the machine: a fixed sequence
    // visiting every state.
    let mut w = World::new();
    let steps = [
        Op::Enqueue {
            kind: 1,
            priority: 0,
            network: 2,
            allow_on_battery: true,
            max_attempts: 2,
        },
        Op::Conditions {
            network: 1,
            on_battery: false,
        },
        Op::Tick, // waiting_resources (Wi-Fi only)
        Op::Conditions {
            network: 2,
            on_battery: false,
        },
        Op::Tick, // running
        Op::Finish {
            pick: 0,
            outcome: 1,
        }, // retrying
        Op::Wait { ms: 5_000 },
        Op::Tick, // running again
        Op::Pause { pick: 0 },
        Op::Finish {
            pick: 0,
            outcome: 3,
        }, // paused
        Op::Resume { pick: 0 }, // queued
        Op::Tick,
        Op::Finish {
            pick: 0,
            outcome: 2,
        }, // failed
        Op::Retry { pick: 0 },
        Op::Tick,
        Op::Finish {
            pick: 0,
            outcome: 0,
        }, // done
        Op::Enqueue {
            kind: 0,
            priority: 0,
            network: 0,
            allow_on_battery: true,
            max_attempts: 1,
        },
        Op::Cancel { pick: 1 }, // cancelled
    ];
    let mut visited = HashSet::new();
    for step in &steps {
        w.apply(step);
        visited.extend(w.seen.values().copied());
    }
    for state in JobState::ALL {
        assert!(visited.contains(&state), "{state} not visited");
    }
}
