//! State-machine and bounded-queue contracts for file-operation jobs.
//!
//! This crate schedules immutable operation plans. Concrete file mutation is
//! intentionally left to the operation-specific tasks that follow PD-021.

mod queue;
mod throttle;

pub use queue::{
    CancellationCheckpoint, DecisionReason, JobEvent, JobEventKind, JobFailureCode, JobId,
    JobProgress, JobQueue, JobQueueError, JobState,
};
pub use throttle::FrontendEventThrottle;

use panedeck_domain::DomainLayer;
use panedeck_fs::FileSystemLayer;

/// Composition marker for background job services.
#[derive(Debug, Default)]
pub struct JobsLayer {
    _domain: DomainLayer,
    _file_system: FileSystemLayer,
}
