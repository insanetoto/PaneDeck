use std::{
    collections::{BTreeMap, VecDeque},
    error::Error,
    fmt,
    num::{NonZeroU64, NonZeroUsize},
};

use panedeck_fs::OperationPlan;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JobId(pub(crate) NonZeroU64);

impl JobId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionReason {
    Conflict,
    ExternalChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobFailureCode {
    Validation,
    Execution,
    DecisionRejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobState {
    Queued,
    Validating,
    Running,
    AwaitingDecision {
        reason: DecisionReason,
    },
    Completed,
    PartiallyFailed {
        completed_items: u64,
        failed_items: u64,
    },
    Failed {
        code: JobFailureCode,
    },
    Cancelled,
}

impl JobState {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::PartiallyFailed { .. } | Self::Failed { .. } | Self::Cancelled
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobProgress {
    completed_units: u64,
    total_units: u64,
    failed_items: u64,
}

impl JobProgress {
    pub fn new(
        completed_units: u64,
        total_units: u64,
        failed_items: u64,
    ) -> Result<Self, JobQueueError> {
        if completed_units > total_units {
            return Err(JobQueueError::InvalidProgress);
        }
        Ok(Self {
            completed_units,
            total_units,
            failed_items,
        })
    }

    #[must_use]
    pub const fn completed_units(self) -> u64 {
        self.completed_units
    }

    #[must_use]
    pub const fn total_units(self) -> u64 {
        self.total_units
    }

    #[must_use]
    pub const fn failed_items(self) -> u64 {
        self.failed_items
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobEventKind {
    StateChanged(JobState),
    Progress(JobProgress),
    CancellationRequested,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobEvent {
    pub sequence: u64,
    pub job_id: JobId,
    pub kind: JobEventKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationCheckpoint {
    Continue,
    Stop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobQueueError {
    UnknownJob,
    InvalidTransition {
        from: JobState,
        action: &'static str,
    },
    InvalidProgress,
    IdentifierExhausted,
}

impl fmt::Display for JobQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownJob => formatter.write_str("unknown job"),
            Self::InvalidTransition { from, action } => {
                write!(formatter, "cannot {action} while job is in {from:?}")
            }
            Self::InvalidProgress => {
                formatter.write_str("completed progress exceeds total progress")
            }
            Self::IdentifierExhausted => formatter.write_str("job identifier space exhausted"),
        }
    }
}

impl Error for JobQueueError {}

struct JobRecord {
    plan: OperationPlan,
    state: JobState,
    cancellation_requested: bool,
}

/// A deterministic queue that grants at most `max_concurrency` execution slots.
/// Executors call `cancellation_checkpoint` only after leaving the filesystem
/// in a consistent state.
pub struct JobQueue {
    max_concurrency: NonZeroUsize,
    active_jobs: usize,
    next_id: u64,
    next_sequence: u64,
    pending: VecDeque<JobId>,
    jobs: BTreeMap<JobId, JobRecord>,
    events: VecDeque<JobEvent>,
}

impl JobQueue {
    #[must_use]
    pub fn new(max_concurrency: NonZeroUsize) -> Self {
        Self {
            max_concurrency,
            active_jobs: 0,
            next_id: 1,
            next_sequence: 1,
            pending: VecDeque::new(),
            jobs: BTreeMap::new(),
            events: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, plan: OperationPlan) -> Result<JobId, JobQueueError> {
        let id = NonZeroU64::new(self.next_id)
            .map(JobId)
            .ok_or(JobQueueError::IdentifierExhausted)?;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(JobQueueError::IdentifierExhausted)?;
        self.jobs.insert(
            id,
            JobRecord {
                plan,
                state: JobState::Queued,
                cancellation_requested: false,
            },
        );
        self.pending.push_back(id);
        self.emit(id, JobEventKind::StateChanged(JobState::Queued));
        Ok(id)
    }

    /// Claims the next queued plan for validation when a concurrency slot exists.
    pub fn claim_next(&mut self) -> Option<JobId> {
        if self.active_jobs >= self.max_concurrency.get() {
            return None;
        }
        while let Some(id) = self.pending.pop_front() {
            let Some(record) = self.jobs.get_mut(&id) else {
                continue;
            };
            if record.state != JobState::Queued {
                continue;
            }
            record.state = JobState::Validating;
            self.active_jobs += 1;
            self.emit(id, JobEventKind::StateChanged(JobState::Validating));
            return Some(id);
        }
        None
    }

    pub fn plan(&self, id: JobId) -> Result<&OperationPlan, JobQueueError> {
        self.jobs
            .get(&id)
            .map(|record| &record.plan)
            .ok_or(JobQueueError::UnknownJob)
    }

    pub fn state(&self, id: JobId) -> Result<&JobState, JobQueueError> {
        self.jobs
            .get(&id)
            .map(|record| &record.state)
            .ok_or(JobQueueError::UnknownJob)
    }

    #[must_use]
    pub const fn active_jobs(&self) -> usize {
        self.active_jobs
    }

    pub fn mark_validation_succeeded(&mut self, id: JobId) -> Result<(), JobQueueError> {
        self.transition(
            id,
            &[JobStateTag::Validating],
            JobState::Running,
            "start execution",
        )
    }

    pub fn mark_validation_failed(&mut self, id: JobId) -> Result<(), JobQueueError> {
        self.finish_active(
            id,
            &[JobStateTag::Validating],
            JobState::Failed {
                code: JobFailureCode::Validation,
            },
            "fail validation",
        )
    }

    pub fn report_progress(
        &mut self,
        id: JobId,
        progress: JobProgress,
    ) -> Result<(), JobQueueError> {
        let state = self.state(id)?.clone();
        if state != JobState::Running {
            return Err(JobQueueError::InvalidTransition {
                from: state,
                action: "report progress",
            });
        }
        self.emit(id, JobEventKind::Progress(progress));
        Ok(())
    }

    pub fn await_decision(
        &mut self,
        id: JobId,
        reason: DecisionReason,
    ) -> Result<(), JobQueueError> {
        self.transition(
            id,
            &[JobStateTag::Running],
            JobState::AwaitingDecision { reason },
            "wait for a decision",
        )
    }

    pub fn resume_after_decision(&mut self, id: JobId) -> Result<(), JobQueueError> {
        self.transition(
            id,
            &[JobStateTag::AwaitingDecision],
            JobState::Running,
            "resume after a decision",
        )
    }

    pub fn mark_completed(&mut self, id: JobId) -> Result<(), JobQueueError> {
        self.finish_active(id, &[JobStateTag::Running], JobState::Completed, "complete")
    }

    pub fn mark_partially_failed(
        &mut self,
        id: JobId,
        completed_items: u64,
        failed_items: u64,
    ) -> Result<(), JobQueueError> {
        if failed_items == 0 {
            return Err(JobQueueError::InvalidProgress);
        }
        self.finish_active(
            id,
            &[JobStateTag::Running],
            JobState::PartiallyFailed {
                completed_items,
                failed_items,
            },
            "finish with partial failure",
        )
    }

    pub fn mark_failed(&mut self, id: JobId, code: JobFailureCode) -> Result<(), JobQueueError> {
        self.finish_active(
            id,
            &[JobStateTag::Running, JobStateTag::AwaitingDecision],
            JobState::Failed { code },
            "fail",
        )
    }

    /// Queued jobs cancel immediately. Active jobs only record intent; the
    /// executor must reach `cancellation_checkpoint` before cancellation occurs.
    pub fn request_cancel(&mut self, id: JobId) -> Result<(), JobQueueError> {
        let state = self.state(id)?.clone();
        if state.is_terminal() {
            return Err(JobQueueError::InvalidTransition {
                from: state,
                action: "request cancellation",
            });
        }
        if state == JobState::Queued {
            self.jobs.get_mut(&id).expect("job was checked above").state = JobState::Cancelled;
            self.emit(id, JobEventKind::StateChanged(JobState::Cancelled));
        } else {
            let record = self.jobs.get_mut(&id).expect("job was checked above");
            if !record.cancellation_requested {
                record.cancellation_requested = true;
                self.emit(id, JobEventKind::CancellationRequested);
            }
        }
        Ok(())
    }

    pub fn cancellation_requested(&self, id: JobId) -> Result<bool, JobQueueError> {
        self.jobs
            .get(&id)
            .map(|record| record.cancellation_requested)
            .ok_or(JobQueueError::UnknownJob)
    }

    pub fn cancellation_checkpoint(
        &mut self,
        id: JobId,
    ) -> Result<CancellationCheckpoint, JobQueueError> {
        let record = self.jobs.get(&id).ok_or(JobQueueError::UnknownJob)?;
        let state = record.state.clone();
        if !matches!(
            state,
            JobState::Validating | JobState::Running | JobState::AwaitingDecision { .. }
        ) {
            return Err(JobQueueError::InvalidTransition {
                from: state,
                action: "check cancellation",
            });
        }
        if !record.cancellation_requested {
            return Ok(CancellationCheckpoint::Continue);
        }
        self.finish_active(
            id,
            &[
                JobStateTag::Validating,
                JobStateTag::Running,
                JobStateTag::AwaitingDecision,
            ],
            JobState::Cancelled,
            "cancel",
        )?;
        Ok(CancellationCheckpoint::Stop)
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = JobEvent> + '_ {
        self.events.drain(..)
    }

    fn transition(
        &mut self,
        id: JobId,
        allowed: &[JobStateTag],
        next: JobState,
        action: &'static str,
    ) -> Result<(), JobQueueError> {
        let record = self.jobs.get_mut(&id).ok_or(JobQueueError::UnknownJob)?;
        if !allowed.contains(&JobStateTag::of(&record.state)) {
            return Err(JobQueueError::InvalidTransition {
                from: record.state.clone(),
                action,
            });
        }
        record.state = next.clone();
        self.emit(id, JobEventKind::StateChanged(next));
        Ok(())
    }

    fn finish_active(
        &mut self,
        id: JobId,
        allowed: &[JobStateTag],
        next: JobState,
        action: &'static str,
    ) -> Result<(), JobQueueError> {
        self.transition(id, allowed, next, action)?;
        self.active_jobs = self.active_jobs.saturating_sub(1);
        Ok(())
    }

    fn emit(&mut self, job_id: JobId, kind: JobEventKind) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.events.push_back(JobEvent {
            sequence,
            job_id,
            kind,
        });
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum JobStateTag {
    Queued,
    Validating,
    Running,
    AwaitingDecision,
    Terminal,
}

impl JobStateTag {
    const fn of(state: &JobState) -> Self {
        match state {
            JobState::Queued => Self::Queued,
            JobState::Validating => Self::Validating,
            JobState::Running => Self::Running,
            JobState::AwaitingDecision { .. } => Self::AwaitingDecision,
            JobState::Completed
            | JobState::PartiallyFailed { .. }
            | JobState::Failed { .. }
            | JobState::Cancelled => Self::Terminal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use panedeck_domain::{EntryKind, NativePath};
    use panedeck_fs::{
        FileIdentity, OperationPlanner, OperationRequest, PathPreflightState, PreflightProbe,
        ProbeError,
    };
    use std::{collections::HashMap, path::Path};

    struct FakeProbe(HashMap<std::path::PathBuf, PathPreflightState>);
    impl PreflightProbe for FakeProbe {
        fn inspect(&self, path: &Path) -> Result<Option<PathPreflightState>, ProbeError> {
            Ok(self.0.get(path).copied())
        }
    }

    fn plan(seed: u128) -> OperationPlan {
        let parent = std::path::PathBuf::from(format!("/target-{seed}"));
        let state = PathPreflightState {
            identity: Some(FileIdentity {
                volume_id: 1,
                file_id: seed,
            }),
            kind: EntryKind::Directory,
            byte_len: 0,
            is_readable: true,
            is_writable: true,
        };
        OperationPlanner
            .plan(
                OperationRequest::CreateDirectory {
                    parent: NativePath::new(&parent),
                    name: "new-folder".into(),
                },
                &FakeProbe(HashMap::from([(parent, state)])),
            )
            .expect("fixture plan should be valid")
    }

    #[test]
    fn enforces_bounded_concurrency_and_fifo_claiming() {
        let mut queue = JobQueue::new(NonZeroUsize::new(2).expect("non-zero"));
        let first = queue.enqueue(plan(1)).expect("enqueue");
        let second = queue.enqueue(plan(2)).expect("enqueue");
        let third = queue.enqueue(plan(3)).expect("enqueue");
        assert_eq!(queue.claim_next(), Some(first));
        assert_eq!(queue.claim_next(), Some(second));
        assert_eq!(queue.claim_next(), None);
        queue.mark_validation_succeeded(first).expect("validate");
        queue.mark_completed(first).expect("complete");
        assert_eq!(queue.claim_next(), Some(third));
        assert_eq!(queue.active_jobs(), 2);
    }

    #[test]
    fn supports_explainable_terminal_states() {
        let mut queue = JobQueue::new(NonZeroUsize::new(3).expect("non-zero"));
        let completed = queue.enqueue(plan(1)).expect("enqueue");
        let partial = queue.enqueue(plan(2)).expect("enqueue");
        let failed = queue.enqueue(plan(3)).expect("enqueue");
        for expected in [completed, partial, failed] {
            assert_eq!(queue.claim_next(), Some(expected));
            queue.mark_validation_succeeded(expected).expect("validate");
        }
        queue.mark_completed(completed).expect("complete");
        queue.mark_partially_failed(partial, 4, 1).expect("partial");
        queue
            .mark_failed(failed, JobFailureCode::Execution)
            .expect("fail");
        assert_eq!(queue.state(completed), Ok(&JobState::Completed));
        assert_eq!(
            queue.state(partial),
            Ok(&JobState::PartiallyFailed {
                completed_items: 4,
                failed_items: 1
            })
        );
        assert_eq!(
            queue.state(failed),
            Ok(&JobState::Failed {
                code: JobFailureCode::Execution
            })
        );
    }

    #[test]
    fn models_decision_wait_and_rejects_invalid_transitions() {
        let mut queue = JobQueue::new(NonZeroUsize::new(1).expect("non-zero"));
        let id = queue.enqueue(plan(1)).expect("enqueue");
        assert!(matches!(
            queue.mark_completed(id),
            Err(JobQueueError::InvalidTransition { .. })
        ));
        queue.claim_next();
        queue.mark_validation_succeeded(id).expect("validate");
        queue
            .await_decision(id, DecisionReason::Conflict)
            .expect("wait");
        assert_eq!(
            queue.state(id),
            Ok(&JobState::AwaitingDecision {
                reason: DecisionReason::Conflict
            })
        );
        queue.resume_after_decision(id).expect("resume");
        assert_eq!(queue.state(id), Ok(&JobState::Running));
    }

    #[test]
    fn active_cancellation_only_applies_at_a_safe_checkpoint() {
        let mut queue = JobQueue::new(NonZeroUsize::new(1).expect("non-zero"));
        let id = queue.enqueue(plan(1)).expect("enqueue");
        queue.claim_next();
        queue.mark_validation_succeeded(id).expect("validate");
        queue.request_cancel(id).expect("request cancellation");
        assert_eq!(queue.state(id), Ok(&JobState::Running));
        assert!(queue.cancellation_requested(id).expect("known job"));
        assert_eq!(
            queue.cancellation_checkpoint(id).expect("checkpoint"),
            CancellationCheckpoint::Stop
        );
        assert_eq!(queue.state(id), Ok(&JobState::Cancelled));
        assert_eq!(queue.active_jobs(), 0);
    }

    #[test]
    fn queued_cancellation_never_consumes_a_slot() {
        let mut queue = JobQueue::new(NonZeroUsize::new(1).expect("non-zero"));
        let cancelled = queue.enqueue(plan(1)).expect("enqueue");
        let next = queue.enqueue(plan(2)).expect("enqueue");
        queue.request_cancel(cancelled).expect("cancel");
        assert_eq!(queue.claim_next(), Some(next));
        assert_eq!(queue.state(cancelled), Ok(&JobState::Cancelled));
    }

    #[test]
    fn validation_failure_releases_the_slot_for_the_next_job() {
        let mut queue = JobQueue::new(NonZeroUsize::new(1).expect("non-zero"));
        let failed = queue.enqueue(plan(1)).expect("enqueue");
        let next = queue.enqueue(plan(2)).expect("enqueue");
        assert_eq!(queue.claim_next(), Some(failed));
        queue
            .mark_validation_failed(failed)
            .expect("validation failure");

        assert_eq!(
            queue.state(failed),
            Ok(&JobState::Failed {
                code: JobFailureCode::Validation
            })
        );
        assert_eq!(queue.claim_next(), Some(next));
    }

    #[test]
    fn emits_and_validates_progress() {
        let mut queue = JobQueue::new(NonZeroUsize::new(1).expect("non-zero"));
        let id = queue.enqueue(plan(1)).expect("enqueue");
        queue.claim_next();
        queue.mark_validation_succeeded(id).expect("validate");
        let progress = JobProgress::new(3, 10, 1).expect("valid progress");
        queue.report_progress(id, progress).expect("report");
        assert_eq!(
            JobProgress::new(11, 10, 0),
            Err(JobQueueError::InvalidProgress)
        );
        assert!(queue
            .drain_events()
            .any(|event| event.kind == JobEventKind::Progress(progress)));
    }
}
