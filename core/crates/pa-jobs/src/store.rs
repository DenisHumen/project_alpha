// SPDX-License-Identifier: MPL-2.0
//! The synchronous core of the queue: the `jobs` table and every state
//! change. [`JobQueue`](crate::JobQueue) wraps it with worker threads; tests
//! drive it directly with explicit timestamps.

use std::collections::{HashMap, HashSet};

use pa_library::rusqlite::{
    self, Connection, OptionalExtension, Row, TransactionBehavior, params,
};
use serde_json::Value;

use crate::{
    Admission, Backoff, Conditions, Error, Job, JobEvent, JobId, JobState, Limits, NewJob,
    PauseReason, ResourceGate, kinds, priority,
};

/// What a handler run ended with.
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    /// Finished; the optional result goes to `result_json`.
    Done(Option<Value>),
    /// A transient error (network, busy server): retried with backoff until
    /// `max_attempts` runs failed.
    Retryable(String),
    /// A permanent error (VOD deleted, no access): the job fails now.
    Fatal(String),
    /// The handler stopped because it was asked to (pause, cancel, shutdown).
    Interrupted,
}

/// Input of one scheduling pass.
pub struct ScheduleInput<'a> {
    pub conditions: Conditions,
    pub limits: &'a Limits,
    pub gate: Option<&'a dyn ResourceGate>,
    /// True if a handler for the type is registered. Jobs of other types
    /// stay where they are.
    pub can_run: &'a dyn Fn(&str) -> bool,
    /// Jobs whose previous run has not returned yet; never started twice.
    pub busy: &'a HashSet<JobId>,
}

/// Result of a scheduling pass.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Schedule {
    /// Jobs moved to `running`; the caller starts their handlers.
    pub started: Vec<Job>,
    /// Running jobs asked to pause (conditions changed); the caller signals
    /// their handlers.
    pub interrupt: Vec<(JobId, PauseReason)>,
}

const COLUMNS: &str = "id, type, state, priority, params_json, checkpoint_json, result_json, error, \
     progress, bytes_done, bytes_total, attempts, max_attempts, next_run_at_ms, network, \
     allow_on_battery, pause_reason, wait_reason, owner_profile_id, target_storage, \
     created_by_device, created_at_ms, updated_at_ms";

fn parse_json(text: Option<String>) -> rusqlite::Result<Option<Value>> {
    text.map(|t| {
        serde_json::from_str(&t)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    })
    .transpose()
}

fn bad_text(e: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        e.to_string().into(),
    )
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    let state: String = row.get(2)?;
    let network: String = row.get(14)?;
    let pause: Option<String> = row.get(16)?;
    let attempts: i64 = row.get(11)?;
    let max_attempts: i64 = row.get(12)?;
    Ok(Job {
        id: row.get(0)?,
        kind: row.get(1)?,
        state: state.parse().map_err(bad_text)?,
        priority: row.get(3)?,
        params: parse_json(row.get(4)?)?.unwrap_or(Value::Null),
        checkpoint: parse_json(row.get(5)?)?,
        result: parse_json(row.get(6)?)?,
        error: row.get(7)?,
        progress: row.get(8)?,
        bytes_done: to_u64(row.get(9)?),
        bytes_total: row.get::<_, Option<i64>>(10)?.map(to_u64),
        attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
        max_attempts: u32::try_from(max_attempts).unwrap_or(u32::MAX),
        next_run_at_ms: row.get(13)?,
        network: network.parse().map_err(bad_text)?,
        allow_on_battery: row.get(15)?,
        pause_reason: pause.map(|p| p.parse()).transpose().map_err(bad_text)?,
        wait_reason: row.get(17)?,
        owner_profile_id: row.get(18)?,
        target_storage: row.get(19)?,
        created_by_device: row.get(20)?,
        created_at_ms: row.get(21)?,
        updated_at_ms: row.get(22)?,
    })
}

fn load(conn: &Connection, id: JobId) -> Result<Job, Error> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"),
        [id],
        job_from_row,
    )
    .optional()?
    .ok_or(Error::NotFound(id))
}

/// Moves `job` to `to`, checking the transition table and that nobody
/// changed the row meanwhile. Records the event.
fn transition(
    conn: &Connection,
    events: &mut Vec<JobEvent>,
    job: &mut Job,
    to: JobState,
    now_ms: i64,
) -> Result<(), Error> {
    if !job.state.can_transition_to(to) {
        return Err(Error::InvalidTransition {
            id: job.id,
            from: job.state,
            to,
        });
    }
    let changed = conn.execute(
        "UPDATE jobs SET state = ?1, updated_at_ms = ?2 WHERE id = ?3 AND state = ?4",
        params![to.as_str(), now_ms, job.id, job.state.as_str()],
    )?;
    if changed != 1 {
        return Err(Error::Conflict(job.id));
    }
    events.push(JobEvent::StateChanged {
        id: job.id,
        kind: job.kind.clone(),
        from: job.state,
        to,
    });
    job.state = to;
    job.updated_at_ms = now_ms;
    Ok(())
}

/// The persistent queue over a library database connection.
///
/// Every method that changes a job records [`JobEvent`]s; collect them with
/// [`take_events`](Self::take_events). One process owns the queue of a
/// database: two schedulers on the same file would both start a job.
#[derive(Debug)]
pub struct JobStore {
    conn: Connection,
    events: Vec<JobEvent>,
}

impl JobStore {
    /// Wraps a connection to a migrated library database
    /// ([`pa_library::open`]).
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            events: Vec::new(),
        }
    }

    /// Opens (creating and migrating if needed) a library database.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        Ok(Self::new(pa_library::open(path)?))
    }

    /// A store over a migrated in-memory database (tests).
    pub fn open_in_memory() -> Result<Self, Error> {
        Ok(Self::new(pa_library::open_in_memory()?))
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Events recorded since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<JobEvent> {
        std::mem::take(&mut self.events)
    }

    /// Runs `f` in a transaction; its events are kept only on commit.
    fn write<T>(
        &mut self,
        f: impl FnOnce(&Connection, &mut Vec<JobEvent>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        // Immediate: take the write lock up front, so a concurrent writer
        // makes us wait (busy timeout) instead of failing on upgrade.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut events = Vec::new();
        let value = f(&tx, &mut events)?;
        tx.commit()?;
        self.events.append(&mut events);
        Ok(value)
    }

    /// Adds a job in state `queued`.
    pub fn enqueue(&mut self, job: &NewJob, now_ms: i64) -> Result<JobId, Error> {
        if !kinds::is_valid(&job.kind) {
            return Err(Error::InvalidKind(job.kind.clone()));
        }
        if !job.params.is_object() {
            return Err(Error::Invalid("params must be a JSON object"));
        }
        if job.max_attempts == 0 {
            return Err(Error::Invalid("max_attempts must be at least 1"));
        }
        let params = serde_json::to_string(&job.params)?;
        self.write(|conn, events| {
            conn.execute(
                "INSERT INTO jobs (type, owner_profile_id, target_storage, priority, state, \
                 bytes_total, max_attempts, next_run_at_ms, params_json, network, \
                 allow_on_battery, created_by_device, created_at_ms, updated_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                params![
                    job.kind,
                    job.owner_profile_id,
                    job.target_storage,
                    job.priority,
                    job.bytes_total.map(to_i64),
                    i64::from(job.max_attempts),
                    job.not_before_ms,
                    params,
                    job.network.as_str(),
                    job.allow_on_battery,
                    job.created_by_device,
                    now_ms,
                ],
            )?;
            let id = conn.last_insert_rowid();
            events.push(JobEvent::Added {
                id,
                kind: job.kind.clone(),
            });
            Ok(id)
        })
    }

    pub fn get(&self, id: JobId) -> Result<Job, Error> {
        load(&self.conn, id)
    }

    /// Jobs in any of `states` (all jobs if empty), in scheduling order:
    /// priority, then age.
    pub fn list(&self, states: &[JobState]) -> Result<Vec<Job>, Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM jobs ORDER BY priority DESC, created_at_ms, id"
        ))?;
        let jobs = stmt
            .query_map([], job_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs
            .into_iter()
            .filter(|j| states.is_empty() || states.contains(&j.state))
            .collect())
    }

    /// Recovery after a restart: jobs left `running` by a dead process go
    /// back to `queued` (or to `paused` if a pause was requested) and keep
    /// their checkpoint and `bytes_done`, so their handler resumes. The
    /// interrupted run does not count as a failed attempt. Returns how many
    /// jobs were recovered.
    pub fn recover(&mut self, now_ms: i64) -> Result<usize, Error> {
        let running = self.list(&[JobState::Running])?;
        self.write(|conn, events| {
            for mut job in running.iter().cloned() {
                let to = if job.pause_reason.is_some() {
                    JobState::Paused
                } else {
                    JobState::Queued
                };
                transition(conn, events, &mut job, to, now_ms)?;
                conn.execute(
                    "UPDATE jobs SET attempts = max(attempts - 1, 0) WHERE id = ?1",
                    [job.id],
                )?;
            }
            Ok(running.len())
        })
    }

    /// Pauses a job. A job that is not running pauses now; for a running job
    /// the pause is recorded (and survives a restart) and takes effect when
    /// its handler returns: the caller signals the handler. Returns the job
    /// after the call.
    pub fn pause(&mut self, id: JobId, reason: PauseReason, now_ms: i64) -> Result<Job, Error> {
        self.write(|conn, events| {
            let mut job = load(conn, id)?;
            if job.state == JobState::Paused {
                return Ok(job);
            }
            if job.state != JobState::Running {
                transition(conn, events, &mut job, JobState::Paused, now_ms)?;
            }
            conn.execute(
                "UPDATE jobs SET pause_reason = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![reason.as_str(), now_ms, id],
            )?;
            job.pause_reason = Some(reason);
            Ok(job)
        })
    }

    /// Resumes a paused job (`paused → queued`). On a running job it
    /// withdraws a pending pause.
    pub fn resume(&mut self, id: JobId, now_ms: i64) -> Result<Job, Error> {
        self.write(|conn, events| {
            let mut job = load(conn, id)?;
            match job.state {
                JobState::Paused => {
                    transition(conn, events, &mut job, JobState::Queued, now_ms)?;
                }
                JobState::Running => {}
                from => {
                    return Err(Error::InvalidTransition {
                        id,
                        from,
                        to: JobState::Queued,
                    });
                }
            }
            conn.execute(
                "UPDATE jobs SET pause_reason = NULL WHERE id = ?1",
                [id],
            )?;
            job.pause_reason = None;
            Ok(job)
        })
    }

    /// Cancels a job in any non-terminal state. A running handler is told
    /// to stop by the caller; whatever it returns is ignored.
    pub fn cancel(&mut self, id: JobId, now_ms: i64) -> Result<Job, Error> {
        self.write(|conn, events| {
            let mut job = load(conn, id)?;
            transition(conn, events, &mut job, JobState::Cancelled, now_ms)?;
            conn.execute(
                "UPDATE jobs SET pause_reason = NULL, wait_reason = NULL WHERE id = ?1",
                [id],
            )?;
            Ok(job)
        })
    }

    /// Queues a failed job again with a fresh attempt budget. The
    /// checkpoint is kept.
    pub fn retry(&mut self, id: JobId, now_ms: i64) -> Result<Job, Error> {
        self.write(|conn, events| {
            let mut job = load(conn, id)?;
            if job.state != JobState::Failed {
                return Err(Error::InvalidTransition {
                    id,
                    from: job.state,
                    to: JobState::Queued,
                });
            }
            transition(conn, events, &mut job, JobState::Queued, now_ms)?;
            conn.execute(
                "UPDATE jobs SET attempts = 0, next_run_at_ms = NULL WHERE id = ?1",
                [id],
            )?;
            job.attempts = 0;
            job.next_run_at_ms = None;
            Ok(job)
        })
    }

    pub fn set_priority(&mut self, id: JobId, priority: i64, now_ms: i64) -> Result<(), Error> {
        let changed = self.conn.execute(
            "UPDATE jobs SET priority = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![priority, now_ms, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    /// Deletes a finished (`done`, `failed`, `cancelled`) job.
    pub fn remove(&mut self, id: JobId) -> Result<(), Error> {
        self.write(|conn, events| {
            let job = load(conn, id)?;
            if !job.state.is_terminal() {
                return Err(Error::NotFinished(id));
            }
            conn.execute("DELETE FROM jobs WHERE id = ?1", [id])?;
            events.push(JobEvent::Removed { id });
            Ok(())
        })
    }

    /// Deletes `done` and `cancelled` jobs last changed before `before_ms`.
    /// Failed jobs stay until the user removes or retries them.
    pub fn purge_finished(&mut self, before_ms: i64) -> Result<usize, Error> {
        self.write(|conn, events| {
            let mut stmt = conn.prepare(
                "DELETE FROM jobs WHERE state IN ('done', 'cancelled') AND updated_at_ms < ?1 \
                 RETURNING id",
            )?;
            let ids = stmt
                .query_map([before_ms], |r| r.get::<_, JobId>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            events.extend(ids.iter().map(|&id| JobEvent::Removed { id }));
            Ok(ids.len())
        })
    }

    /// One scheduling pass at `now_ms`:
    ///
    /// 1. running jobs whose network or power requirement is no longer met
    ///    get a pending pause ([`Schedule::interrupt`]);
    /// 2. jobs paused by the scheduler (network, battery, resources) whose
    ///    requirement is met again go back to `queued`;
    /// 3. `queued`, `waiting_resources` and due `retrying` jobs, by priority
    ///    and age, either start (attempts + 1) or wait in
    ///    `waiting_resources` with a reason: requirements, the concurrency
    ///    limit of their type (not for [`priority::LIVE`]) or the gate.
    pub fn schedule(&mut self, input: &ScheduleInput<'_>, now_ms: i64) -> Result<Schedule, Error> {
        let jobs = self.list(&[
            JobState::Running,
            JobState::Paused,
            JobState::Queued,
            JobState::WaitingResources,
            JobState::Retrying,
        ])?;
        self.write(|conn, events| {
            let mut out = Schedule::default();
            let mut running: HashMap<String, usize> = HashMap::new();
            for job in jobs.iter().filter(|j| j.state == JobState::Running) {
                *running.entry(job.kind.clone()).or_default() += 1;
                if job.pause_reason.is_none()
                    && let Some(reason) = job.blocked_by(&input.conditions)
                {
                    conn.execute(
                        "UPDATE jobs SET pause_reason = ?1, updated_at_ms = ?2 WHERE id = ?3",
                        params![reason.as_str(), now_ms, job.id],
                    )?;
                    out.interrupt.push((job.id, reason));
                }
            }
            for job in jobs.iter().filter(|j| j.state == JobState::Paused) {
                let auto = matches!(
                    job.pause_reason,
                    Some(PauseReason::Network | PauseReason::Battery | PauseReason::Resources)
                );
                if auto && job.blocked_by(&input.conditions).is_none() {
                    let mut job = job.clone();
                    transition(conn, events, &mut job, JobState::Queued, now_ms)?;
                    conn.execute("UPDATE jobs SET pause_reason = NULL WHERE id = ?1", [job.id])?;
                }
            }
            // Reload: step 2 may have queued more jobs.
            let mut candidates = Vec::new();
            {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLUMNS} FROM jobs \
                     WHERE state IN ('queued', 'waiting_resources', 'retrying') \
                       AND (next_run_at_ms IS NULL OR next_run_at_ms <= ?1) \
                     ORDER BY priority DESC, created_at_ms, id"
                ))?;
                for job in stmt.query_map([now_ms], job_from_row)? {
                    candidates.push(job?);
                }
            }
            for mut job in candidates {
                if !(input.can_run)(&job.kind) || input.busy.contains(&job.id) {
                    continue;
                }
                let wait = if let Some(reason) = job.blocked_by(&input.conditions) {
                    Some(reason.as_str().to_owned())
                } else if job.priority < priority::LIVE
                    && running.get(&job.kind).copied().unwrap_or(0) >= input.limits.get(&job.kind)
                {
                    Some("limit".to_owned())
                } else if let Some(Admission::Wait(reason)) =
                    input.gate.map(|gate| gate.admit(&job))
                {
                    Some(reason)
                } else {
                    None
                };
                match wait {
                    Some(reason) => {
                        if job.state == JobState::Retrying {
                            transition(conn, events, &mut job, JobState::Queued, now_ms)?;
                        }
                        if job.state != JobState::WaitingResources {
                            transition(conn, events, &mut job, JobState::WaitingResources, now_ms)?;
                        }
                        if job.wait_reason.as_deref() != Some(reason.as_str()) {
                            conn.execute(
                                "UPDATE jobs SET wait_reason = ?1 WHERE id = ?2",
                                params![reason, job.id],
                            )?;
                        }
                    }
                    None => {
                        if job.state == JobState::WaitingResources {
                            transition(conn, events, &mut job, JobState::Queued, now_ms)?;
                        }
                        transition(conn, events, &mut job, JobState::Running, now_ms)?;
                        conn.execute(
                            "UPDATE jobs SET attempts = attempts + 1, wait_reason = NULL, \
                             next_run_at_ms = NULL WHERE id = ?1",
                            [job.id],
                        )?;
                        *running.entry(job.kind.clone()).or_default() += 1;
                        out.started.push(load(conn, job.id)?);
                    }
                }
            }
            Ok(out)
        })
    }

    /// Progress of a running job. `progress` defaults to
    /// `bytes_done / bytes_total`. Returns false if the job is no longer
    /// running (cancelled meanwhile).
    pub fn report_progress(
        &mut self,
        id: JobId,
        progress: Option<f64>,
        bytes_done: u64,
        bytes_total: Option<u64>,
        now_ms: i64,
    ) -> Result<bool, Error> {
        self.write(|conn, events| progress_update(conn, events, id, progress, bytes_done, bytes_total, now_ms))
    }

    /// Saves the handler state for resuming, with `bytes_done`. Written
    /// immediately: after a crash the handler continues from the last
    /// checkpoint. Returns false if the job is no longer running.
    pub fn save_checkpoint(
        &mut self,
        id: JobId,
        checkpoint: &Value,
        bytes_done: u64,
        now_ms: i64,
    ) -> Result<bool, Error> {
        let text = serde_json::to_string(checkpoint)?;
        self.write(|conn, events| {
            let job = load(conn, id)?;
            if !progress_update(conn, events, id, None, bytes_done, job.bytes_total, now_ms)? {
                return Ok(false);
            }
            conn.execute(
                "UPDATE jobs SET checkpoint_json = ?1 WHERE id = ?2",
                params![text, id],
            )?;
            Ok(true)
        })
    }

    /// Records how a run ended. Ignored (returns `None`) if the job is no
    /// longer running: it was cancelled meanwhile. Otherwise returns the new
    /// state:
    ///
    /// - pending pause and the handler stopped (or failed retryably) →
    ///   `paused`, the run is not counted;
    /// - [`RunOutcome::Done`] → `done`, progress 1;
    /// - [`RunOutcome::Retryable`] → `retrying` after [`Backoff`], or
    ///   `failed` when `max_attempts` runs failed;
    /// - [`RunOutcome::Fatal`] → `failed`;
    /// - [`RunOutcome::Interrupted`] without a pending pause → `queued`, not
    ///   counted.
    pub fn finish(
        &mut self,
        id: JobId,
        outcome: &RunOutcome,
        backoff: &Backoff,
        now_ms: i64,
    ) -> Result<Option<JobState>, Error> {
        self.write(|conn, events| {
            let mut job = load(conn, id)?;
            if job.state != JobState::Running {
                return Ok(None);
            }
            let paused = job.pause_reason.is_some();
            let to = match outcome {
                RunOutcome::Done(result) => {
                    let result = result.as_ref().map(serde_json::to_string).transpose()?;
                    conn.execute(
                        "UPDATE jobs SET progress = 1.0, result_json = ?1, pause_reason = NULL, \
                         bytes_done = coalesce(bytes_total, bytes_done) WHERE id = ?2",
                        params![result, id],
                    )?;
                    JobState::Done
                }
                RunOutcome::Interrupted | RunOutcome::Retryable(_) if paused => {
                    if let RunOutcome::Retryable(msg) = outcome {
                        conn.execute("UPDATE jobs SET error = ?1 WHERE id = ?2", params![msg, id])?;
                    }
                    conn.execute(
                        "UPDATE jobs SET attempts = max(attempts - 1, 0) WHERE id = ?1",
                        [id],
                    )?;
                    JobState::Paused
                }
                RunOutcome::Interrupted => {
                    conn.execute(
                        "UPDATE jobs SET attempts = max(attempts - 1, 0) WHERE id = ?1",
                        [id],
                    )?;
                    JobState::Queued
                }
                RunOutcome::Retryable(msg) if job.attempts < job.max_attempts => {
                    let delay = i64::try_from(backoff.delay(job.attempts).as_millis())
                        .unwrap_or(i64::MAX);
                    conn.execute(
                        "UPDATE jobs SET error = ?1, next_run_at_ms = ?2 WHERE id = ?3",
                        params![msg, now_ms.saturating_add(delay), id],
                    )?;
                    JobState::Retrying
                }
                RunOutcome::Retryable(msg) | RunOutcome::Fatal(msg) => {
                    conn.execute(
                        "UPDATE jobs SET error = ?1, pause_reason = NULL WHERE id = ?2",
                        params![msg, id],
                    )?;
                    JobState::Failed
                }
            };
            transition(conn, events, &mut job, to, now_ms)?;
            Ok(Some(to))
        })
    }
}

fn progress_update(
    conn: &Connection,
    events: &mut Vec<JobEvent>,
    id: JobId,
    progress: Option<f64>,
    bytes_done: u64,
    bytes_total: Option<u64>,
    now_ms: i64,
) -> Result<bool, Error> {
    let progress = progress
        .or_else(|| {
            bytes_total
                .filter(|&t| t > 0)
                .map(|t| bytes_done as f64 / t as f64)
        })
        .map(|p| if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 });
    let changed = conn.execute(
        "UPDATE jobs SET progress = ?1, bytes_done = ?2, bytes_total = coalesce(?3, bytes_total), \
         updated_at_ms = ?4 WHERE id = ?5 AND state = 'running'",
        params![progress, to_i64(bytes_done), bytes_total.map(to_i64), now_ms, id],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    events.push(JobEvent::Progress {
        id,
        progress,
        bytes_done,
        bytes_total,
    });
    Ok(true)
}
