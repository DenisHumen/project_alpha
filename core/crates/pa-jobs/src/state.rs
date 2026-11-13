// SPDX-License-Identifier: MPL-2.0
//! Job states and the allowed transitions (docs/dev/architecture.md §6,
//! docs/research/04 §1.10).

use std::fmt;
use std::str::FromStr;

/// State of a job. Stored as text in `jobs.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JobState {
    /// Waiting for the scheduler.
    Queued,
    /// Not started: no network, Wi-Fi only on a metered network, on battery,
    /// the concurrency limit of its type is reached or the resource gate
    /// (storage, CORE-012) said "wait". The reason is in
    /// [`Job::wait_reason`](crate::Job::wait_reason).
    WaitingResources,
    /// A handler runs it.
    Running,
    /// Stopped by the user or by the scheduler (network, battery,
    /// resources); see [`PauseReason`](crate::PauseReason).
    Paused,
    /// Failed with a retryable error; runs again at `next_run_at_ms`.
    Retrying,
    /// Finished successfully. Terminal.
    Done,
    /// Retries exhausted or a fatal error. Terminal, but the user may
    /// [`retry`](crate::JobQueue::retry) it.
    Failed,
    /// Cancelled by the user. Terminal.
    Cancelled,
}

impl JobState {
    /// All states, in declaration order.
    pub const ALL: [JobState; 8] = [
        JobState::Queued,
        JobState::WaitingResources,
        JobState::Running,
        JobState::Paused,
        JobState::Retrying,
        JobState::Done,
        JobState::Failed,
        JobState::Cancelled,
    ];

    /// Every allowed transition `(from, to)`.
    ///
    /// The edges of the diagram in docs/dev/architecture.md plus a few the
    /// diagram leaves implicit (ADR-NNNN):
    ///
    /// - `running → queued`: the process died while the job ran; on the next
    ///   start it is queued again and resumes from its checkpoint;
    /// - `retrying → queued`: a due retry whose resources are missing goes
    ///   back through `queued` to `waiting_resources`;
    /// - `queued | waiting_resources | retrying → paused`: the user may pause
    ///   a job that is not running yet;
    /// - `waiting_resources | paused | retrying → cancelled`: cancel works in
    ///   every non-terminal state;
    /// - `failed → queued`: the user retries a failed job.
    pub const TRANSITIONS: [(JobState, JobState); 20] = {
        use JobState::*;
        [
            (Queued, WaitingResources),
            (Queued, Running),
            (Queued, Paused),
            (Queued, Cancelled),
            (WaitingResources, Queued),
            (WaitingResources, Paused),
            (WaitingResources, Cancelled),
            (Running, Paused),
            (Running, Retrying),
            (Running, Done),
            (Running, Failed),
            (Running, Cancelled),
            (Running, Queued),
            (Paused, Queued),
            (Paused, Cancelled),
            (Retrying, Running),
            (Retrying, Queued),
            (Retrying, Paused),
            (Retrying, Cancelled),
            (Failed, Queued),
        ]
    };

    /// Name stored in the database.
    pub const fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::WaitingResources => "waiting_resources",
            JobState::Running => "running",
            JobState::Paused => "paused",
            JobState::Retrying => "retrying",
            JobState::Done => "done",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    /// `done`, `failed` and `cancelled`: the scheduler never touches them.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Done | JobState::Failed | JobState::Cancelled
        )
    }

    /// True if `self → to` is an allowed transition. A state never
    /// "transitions" to itself.
    pub fn can_transition_to(self, to: JobState) -> bool {
        self != to
            && Self::TRANSITIONS
                .iter()
                .any(|&(from, next)| from == self && next == to)
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error of [`JobState::from_str`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown job state {0:?}")]
pub struct UnknownState(pub String);

impl FromStr for JobState {
    type Err = UnknownState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| UnknownState(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for state in JobState::ALL {
            assert_eq!(state.as_str().parse::<JobState>(), Ok(state));
        }
        assert!("stuck".parse::<JobState>().is_err());
    }

    #[test]
    fn diagram_edges_are_allowed() {
        use JobState::*;
        // docs/dev/architecture.md, stateDiagram-v2 of pa-jobs.
        for (from, to) in [
            (Queued, WaitingResources),
            (WaitingResources, Queued),
            (Queued, Running),
            (Running, Paused),
            (Paused, Queued),
            (Running, Retrying),
            (Retrying, Running),
            (Running, Done),
            (Running, Failed),
            (Queued, Cancelled),
            (Running, Cancelled),
        ] {
            assert!(from.can_transition_to(to), "{from} -> {to}");
        }
    }

    #[test]
    fn done_and_cancelled_are_final() {
        for to in JobState::ALL {
            assert!(!JobState::Done.can_transition_to(to));
            assert!(!JobState::Cancelled.can_transition_to(to));
        }
        assert!(JobState::Failed.can_transition_to(JobState::Queued));
        assert!(!JobState::Failed.can_transition_to(JobState::Running));
    }

    #[test]
    fn every_live_state_can_be_cancelled() {
        for from in JobState::ALL {
            assert_eq!(
                from.can_transition_to(JobState::Cancelled),
                !from.is_terminal()
            );
        }
    }
}
