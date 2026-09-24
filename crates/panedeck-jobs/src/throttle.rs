use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

use crate::{JobEvent, JobEventKind, JobId};

/// Coalesces frequent progress updates while forwarding state changes at once.
/// A terminal state flushes the newest pending progress first, so the frontend
/// never observes a terminal state without the final available progress sample.
pub struct FrontendEventThrottle {
    minimum_interval: Duration,
    last_progress_at: BTreeMap<JobId, Instant>,
    pending_progress: BTreeMap<JobId, JobEvent>,
    ready: VecDeque<JobEvent>,
}

impl FrontendEventThrottle {
    #[must_use]
    pub fn new(minimum_interval: Duration) -> Self {
        Self {
            minimum_interval,
            last_progress_at: BTreeMap::new(),
            pending_progress: BTreeMap::new(),
            ready: VecDeque::new(),
        }
    }

    pub fn push(&mut self, event: JobEvent, now: Instant) {
        match &event.kind {
            JobEventKind::Progress(_) => {
                let may_emit = self.last_progress_at.get(&event.job_id).is_none_or(|last| {
                    now.saturating_duration_since(*last) >= self.minimum_interval
                });
                if may_emit {
                    self.last_progress_at.insert(event.job_id, now);
                    self.ready.push_back(event);
                } else {
                    self.pending_progress.insert(event.job_id, event);
                }
            }
            JobEventKind::StateChanged(state) if state.is_terminal() => {
                if let Some(progress) = self.pending_progress.remove(&event.job_id) {
                    self.last_progress_at.insert(event.job_id, now);
                    self.ready.push_back(progress);
                }
                self.ready.push_back(event);
            }
            JobEventKind::StateChanged(_) | JobEventKind::CancellationRequested => {
                self.ready.push_back(event);
            }
        }
    }

    /// Makes progress samples ready once their interval has elapsed.
    pub fn flush_due(&mut self, now: Instant) {
        let due: Vec<_> = self
            .pending_progress
            .keys()
            .copied()
            .filter(|job_id| {
                self.last_progress_at.get(job_id).is_none_or(|last| {
                    now.saturating_duration_since(*last) >= self.minimum_interval
                })
            })
            .collect();
        let mut events: Vec<_> = due
            .into_iter()
            .filter_map(|job_id| {
                self.last_progress_at.insert(job_id, now);
                self.pending_progress.remove(&job_id)
            })
            .collect();
        events.sort_by_key(|event| event.sequence);
        self.ready.extend(events);
    }

    pub fn drain_ready(&mut self) -> impl Iterator<Item = JobEvent> + '_ {
        self.ready.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use crate::{JobProgress, JobState};

    use super::*;

    fn id(value: u64) -> JobId {
        JobId(NonZeroU64::new(value).expect("non-zero"))
    }

    fn progress(sequence: u64, completed: u64) -> JobEvent {
        JobEvent {
            sequence,
            job_id: id(1),
            kind: JobEventKind::Progress(
                JobProgress::new(completed, 10, 0).expect("valid progress"),
            ),
        }
    }

    #[test]
    fn coalesces_progress_until_the_interval_elapses() {
        let start = Instant::now();
        let mut throttle = FrontendEventThrottle::new(Duration::from_millis(100));
        throttle.push(progress(1, 1), start);
        throttle.push(progress(2, 2), start + Duration::from_millis(10));
        throttle.push(progress(3, 3), start + Duration::from_millis(20));
        assert_eq!(throttle.drain_ready().count(), 1);

        throttle.flush_due(start + Duration::from_millis(100));
        assert_eq!(
            throttle.drain_ready().collect::<Vec<_>>(),
            vec![progress(3, 3)]
        );
    }

    #[test]
    fn terminal_event_flushes_latest_progress_and_is_never_dropped() {
        let start = Instant::now();
        let mut throttle = FrontendEventThrottle::new(Duration::from_secs(1));
        throttle.push(progress(1, 1), start);
        throttle.drain_ready().for_each(drop);
        throttle.push(progress(2, 10), start + Duration::from_millis(10));
        let terminal = JobEvent {
            sequence: 3,
            job_id: id(1),
            kind: JobEventKind::StateChanged(JobState::Completed),
        };
        throttle.push(terminal.clone(), start + Duration::from_millis(20));

        assert_eq!(
            throttle.drain_ready().collect::<Vec<_>>(),
            vec![progress(2, 10), terminal]
        );
    }

    #[test]
    fn state_changes_bypass_progress_throttling() {
        let now = Instant::now();
        let mut throttle = FrontendEventThrottle::new(Duration::from_secs(1));
        let state = JobEvent {
            sequence: 1,
            job_id: id(1),
            kind: JobEventKind::StateChanged(JobState::Running),
        };
        throttle.push(state.clone(), now);
        assert_eq!(throttle.drain_ready().collect::<Vec<_>>(), vec![state]);
    }
}
