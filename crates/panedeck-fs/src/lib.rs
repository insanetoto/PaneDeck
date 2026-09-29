//! File-system contracts and implementations.
//!
//! Concrete operations are added only by their dedicated tasks. The crate may
//! depend on domain types, but the domain must never depend on this crate.

mod clipboard;
mod conflict;
mod copy;
mod directory_reader;
mod directory_watcher;
mod fault_injection;
mod move_operation;
mod mutations;
mod operation_plan;
mod trash;

pub use clipboard::{FileClipboardBackend, FileClipboardError, FileClipboardSnapshot};
pub use conflict::{
    ActionReadiness, Conflict, ConflictAction, ConflictDecision, ConflictError, ConflictGuard,
    ConflictKind, ConflictResolution, ConflictScope, ConflictSession,
};
pub use copy::{
    AtomicCancellation, CopyBatchResult, CopyCancellation, CopyErrorCode, CopyExecutor,
    CopyItemOutcome, CopyItemResult, CopyProgress, CopyProgressObserver, NeverCancel,
};

pub use directory_reader::{
    DirectoryBatch, DirectoryEntrySummary, DirectoryReadError, DirectoryReadEvent,
    DirectoryReadHandle, DirectoryReadOptions, DirectoryReader, ReadRequestId, SortDirection,
    SortField, SortSpec,
};
pub use directory_watcher::{DirectoryWatch, DirectoryWatcher, WatchError, WatchSignal};
pub use fault_injection::{FaultInjectingProbe, InjectedFault};
pub use move_operation::{
    MoveBatchResult, MoveErrorCode, MoveExecutor, MoveItemOutcome, MoveItemResult, SourceRemover,
    StandardSourceRemover,
};
pub use mutations::{rename_no_replace, MutationErrorCode, MutationExecutor};
pub use operation_plan::{
    FileIdentity, OperationKind, OperationPlan, OperationPlanner, OperationRequest,
    PathPreflightState, PlanValidationError, PlanValidationErrorKind, PlannedSource, PlannedTarget,
    PreflightProbe, ProbeError, StandardPreflightProbe,
};
pub use trash::{
    TrashBackend, TrashBackendError, TrashBatchResult, TrashErrorCode, TrashExecutor,
    TrashItemOutcome, TrashItemResult,
};

use panedeck_domain::DomainLayer;

/// Composition marker for file-system services.
#[derive(Debug, Default)]
pub struct FileSystemLayer {
    _domain: DomainLayer,
}
