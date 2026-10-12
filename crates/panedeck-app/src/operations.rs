use std::{
    collections::BTreeMap,
    ffi::OsString,
    fmt, fs,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use panedeck_fs::{
    Conflict, ConflictAction, ConflictDecision, ConflictKind, ConflictResolution, ConflictScope,
    ConflictSession, CopyExecutor, CopyItemOutcome, MoveExecutor, MoveItemOutcome,
    MutationExecutor, OperationKind, OperationPlanner, OperationRequest, PlanValidationErrorKind,
    PreflightProbe, StandardPreflightProbe, TrashExecutor, TrashItemOutcome,
};
use panedeck_jobs::{
    CancellationCheckpoint, JobFailureCode, JobId, JobProgress, JobQueue, JobState,
};
use panedeck_platform::SystemTrashAdapter;
use serde::{Deserialize, Serialize};

use crate::{
    BrowseErrorDto, BrowserService, DirectoryReferenceDto, EntryReferenceDto, RecoveryService,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKindDto {
    Copy,
    Move,
    Rename,
    CreateDirectory,
    Trash,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferRequestDto {
    pub kind: OperationKindDto,
    pub entries: Vec<EntryReferenceDto>,
    pub destination: TransferDestinationDto,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferDestinationDto {
    Directory { directory: DirectoryReferenceDto },
    Entry { entry: EntryReferenceDto },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameRequestDto {
    pub entry: EntryReferenceDto,
    pub new_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDirectoryRequestDto {
    pub directory: DirectoryReferenceDto,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateChildDirectoryRequestDto {
    pub entry: EntryReferenceDto,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrashRequestDto {
    pub entries: Vec<EntryReferenceDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationItemOutcomeDto {
    Succeeded,
    Skipped,
    Failed,
    Cancelled,
    CopiedButSourceNotRemoved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResultDto {
    pub job_id: u64,
    pub kind: OperationKindDto,
    pub completed_items: usize,
    pub failed_items: usize,
    pub item_outcomes: Vec<OperationItemOutcomeDto>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationJobStateDto {
    Queued,
    Validating,
    Running,
    AwaitingDecision,
    CancellationRequested,
    Completed,
    PartiallyFailed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationPhaseDto {
    Preparing,
    Copying,
    Verifying,
    CleaningSource,
    Finished,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshotDto {
    pub sequence: u64,
    pub jobs: Vec<OperationJobDto>,
    pub log_status: crate::LogStatusDto,
    pub recovery_log_status: crate::LogStatusDto,
}
type Watcher = Arc<dyn Fn(OperationSnapshotDto) + Send + Sync>;
struct EventHub {
    sequence: u64,
    next_watch: u64,
    watchers: BTreeMap<u64, Watcher>,
    throttle: panedeck_jobs::FrontendEventThrottle,
}
impl Default for EventHub {
    fn default() -> Self {
        Self {
            sequence: 0,
            next_watch: 1,
            watchers: BTreeMap::new(),
            throttle: panedeck_jobs::FrontendEventThrottle::new(Duration::from_millis(50)),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJobDto {
    pub job_id: u64,
    pub operation_id: String,
    pub operation_item_count: usize,
    pub operation_state: Option<OperationJobStateDto>,
    pub operation_outcomes: Option<Vec<OperationItemOutcomeDto>>,
    pub phase: OperationPhaseDto,
    pub item_errors: Vec<crate::ItemErrorDto>,
    pub kind: OperationKindDto,
    pub state: OperationJobStateDto,
    pub item_count: usize,
    pub completed_items: usize,
    pub failed_items: usize,
    pub completed_units: Option<u64>,
    pub total_units: Option<u64>,
    pub bytes_per_second: Option<u64>,
    pub elapsed_millis: u64,
    pub item_outcomes: Vec<OperationItemOutcomeDto>,
    pub can_cancel: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<OperationConflictDto>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictDecisionDto {
    Skip,
    Replace,
    KeepBoth,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKindDto {
    FileReplacement,
    DirectoryMerge,
    TypeMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictEntryDto {
    pub name: String,
    pub parent_context: String,
    pub kind: String,
    pub byte_len: u64,
    pub modified_millis: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationConflictDto {
    pub kind: ConflictKindDto,
    pub source: ConflictEntryDto,
    pub target: ConflictEntryDto,
    pub replace_allowed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveConflictRequestDto {
    pub job_id: u64,
    pub decision: ConflictDecisionDto,
    pub apply_to_remaining: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelOperationRequestDto {
    pub job_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationErrorCode {
    EmptySelection,
    InvalidName,
    StaleReference,
    NotWritable,
    SamePath,
    DirectoryIntoItself,
    Conflict,
    InsufficientSpace,
    SourceChanged,
    PermissionDenied,
    Unsupported,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationErrorDto {
    pub code: OperationErrorCode,
}

impl fmt::Display for OperationErrorDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "operation failed: {:?}", self.code)
    }
}

impl std::error::Error for OperationErrorDto {}

type CompletionCallback = Box<dyn FnOnce(&[OperationItemOutcomeDto]) + Send>;

struct Completion {
    operation_id: String,
    item_count: usize,
    callback: CompletionCallback,
}
impl Completion {
    fn run(self, outcomes: &[OperationItemOutcomeDto]) {
        (self.callback)(outcomes);
    }
    fn wrap<F>(self, callback: F) -> Self
    where
        F: FnOnce(&[OperationItemOutcomeDto], Completion) + Send + 'static,
    {
        Self {
            operation_id: self.operation_id.clone(),
            item_count: self.item_count,
            callback: Box::new(move |outcomes| callback(outcomes, self)),
        }
    }
}

struct PendingConflict {
    request: OperationRequest,
    remaining_request: Option<OperationRequest>,
    conflict: Conflict,
    session: ConflictSession,
    completion: Completion,
}

struct OperationRecord {
    id: JobId,
    operation_id: String,
    operation_item_count: usize,
    item_errors: Vec<crate::ItemErrorDto>,
    phase: OperationPhaseDto,
    kind: OperationKindDto,
    state: OperationJobStateDto,
    item_count: usize,
    submitted_at: Instant,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    progress: Option<JobProgress>,
    outcomes: Vec<OperationItemOutcomeDto>,
    cancellation_requested: bool,
    source_paths: Vec<PathBuf>,
    destination_directories: Vec<PathBuf>,
}

struct OperationInner {
    execution_lock: Mutex<()>,
    queue: Mutex<JobQueue>,
    records: Mutex<BTreeMap<u64, OperationRecord>>,
    cancellations: Mutex<BTreeMap<u64, Arc<panedeck_fs::AtomicCancellation>>>,
    completions: Mutex<BTreeMap<u64, Completion>>,
    recovery: RecoveryService,
    log: crate::OperationLog,
    hub: Mutex<EventHub>,
    pump_started: std::sync::atomic::AtomicBool,
    next_pending_id: AtomicU64,
    pending_conflicts: Mutex<BTreeMap<u64, PendingConflict>>,
    external_jobs: Mutex<BTreeMap<u64, OperationJobDto>>,
}

#[derive(Clone)]
pub struct OperationService {
    inner: Arc<OperationInner>,
}

impl Default for OperationService {
    fn default() -> Self {
        Self {
            inner: Arc::new(OperationInner {
                execution_lock: Mutex::new(()),
                queue: Mutex::new(JobQueue::new(
                    NonZeroUsize::new(1).expect("operation concurrency is non-zero"),
                )),
                records: Mutex::new(BTreeMap::new()),
                cancellations: Mutex::new(BTreeMap::new()),
                completions: Mutex::new(BTreeMap::new()),
                recovery: RecoveryService::default(),
                log: crate::OperationLog::default(),
                hub: Mutex::new(EventHub::default()),
                pump_started: std::sync::atomic::AtomicBool::new(false),
                next_pending_id: AtomicU64::new(1_000_000_000),
                pending_conflicts: Mutex::new(BTreeMap::new()),
                external_jobs: Mutex::new(BTreeMap::new()),
            }),
        }
    }
}

impl OperationService {
    #[must_use]
    pub fn with_recovery(recovery: RecoveryService) -> Self {
        Self::with_logs(recovery, crate::OperationLog::default())
    }
    pub fn log(&self) -> &crate::OperationLog {
        &self.inner.log
    }
    pub fn with_logs(recovery: RecoveryService, log: crate::OperationLog) -> Self {
        Self {
            inner: Arc::new(OperationInner {
                execution_lock: Mutex::new(()),
                queue: Mutex::new(JobQueue::new(
                    NonZeroUsize::new(1).expect("operation concurrency is non-zero"),
                )),
                records: Mutex::new(BTreeMap::new()),
                cancellations: Mutex::new(BTreeMap::new()),
                completions: Mutex::new(BTreeMap::new()),
                recovery,
                log,
                hub: Mutex::new(EventHub::default()),
                pump_started: std::sync::atomic::AtomicBool::new(false),
                next_pending_id: AtomicU64::new(1_000_000_000),
                pending_conflicts: Mutex::new(BTreeMap::new()),
                external_jobs: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn watch<F>(&self, callback: F) -> (u64, OperationSnapshotDto)
    where
        F: Fn(OperationSnapshotDto) + Send + Sync + 'static,
    {
        let mut hub = self
            .inner
            .hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = hub.next_watch;
        hub.next_watch += 1;
        hub.watchers.insert(id, Arc::new(callback));
        let snapshot = self.state_snapshot(hub.sequence);
        drop(hub);
        if !self.inner.pump_started.swap(true, Ordering::AcqRel) {
            let weak = Arc::downgrade(&self.inner);
            thread::spawn(move || loop {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                publish(&inner, false);
                drop(inner);
                thread::sleep(Duration::from_millis(50));
            });
        }
        (id, snapshot)
    }
    pub fn unwatch(&self, id: u64) {
        self.inner
            .hub
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .watchers
            .remove(&id);
    }
    fn state_snapshot(&self, sequence: u64) -> OperationSnapshotDto {
        OperationSnapshotDto {
            sequence,
            jobs: self.jobs(),
            log_status: self.inner.log.status(),
            recovery_log_status: self.inner.recovery.log_status(),
        }
    }

    /// Read-only drag feedback: no jobs, journaling, recursive sizing or mutations.
    pub fn validate_transfer(
        &self,
        browser: &BrowserService,
        request: TransferRequestDto,
    ) -> Result<(), OperationErrorDto> {
        self.resolve_transfer(browser, request).map(|_| ())
    }

    pub fn transfer(
        &self,
        browser: &BrowserService,
        request: TransferRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        self.logged_request(
            request.kind,
            || self.resolve_transfer(browser, request),
            Box::new(|_| {}),
        )
    }

    fn resolve_transfer(
        &self,
        browser: &BrowserService,
        request: TransferRequestDto,
    ) -> Result<OperationRequest, OperationErrorDto> {
        if !matches!(
            request.kind,
            OperationKindDto::Copy | OperationKindDto::Move
        ) {
            return Err(error(OperationErrorCode::Unsupported));
        }
        let sources = browser
            .resolve_selection(request.entries)
            .map_err(map_browse_error)?;
        let destination = match request.destination {
            TransferDestinationDto::Directory { directory } => browser
                .resolve_directory(directory)
                .map_err(map_browse_error)?,
            TransferDestinationDto::Entry { entry } => browser
                .resolve_entries(vec![entry])
                .map_err(map_browse_error)?
                .into_iter()
                .next()
                .ok_or_else(|| error(OperationErrorCode::StaleReference))?,
        };
        // Resolve aliases on the native side so both feedback and execution use the same directory.
        let destination = panedeck_domain::NativePath::new(
            fs::canonicalize(destination.as_path())
                .map_err(|_| error(OperationErrorCode::StaleReference))?,
        );
        let operation = match request.kind {
            OperationKindDto::Copy => OperationRequest::Copy {
                sources,
                destination_directory: destination,
            },
            OperationKindDto::Move => OperationRequest::Move {
                sources,
                destination_directory: destination,
            },
            _ => unreachable!("validated transfer kind"),
        };
        validate_transfer_paths(&operation)?;
        Ok(operation)
    }

    pub fn rename(
        &self,
        browser: &BrowserService,
        request: RenameRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        self.logged_request(
            OperationKindDto::Rename,
            || {
                let source = browser
                    .resolve_entries(vec![request.entry])
                    .map_err(map_browse_error)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| error(OperationErrorCode::EmptySelection))?;
                Ok(OperationRequest::Rename {
                    source,
                    new_name: OsString::from(request.new_name),
                })
            },
            Box::new(|_| {}),
        )
    }

    pub fn create_directory(
        &self,
        browser: &BrowserService,
        request: CreateDirectoryRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        self.logged_request(
            OperationKindDto::CreateDirectory,
            || {
                let parent = browser
                    .resolve_directory(request.directory)
                    .map_err(map_browse_error)?;
                Ok(OperationRequest::CreateDirectory {
                    parent,
                    name: OsString::from(request.name),
                })
            },
            Box::new(|_| {}),
        )
    }

    pub fn create_child_directory(
        &self,
        browser: &BrowserService,
        request: CreateChildDirectoryRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        self.logged_request(
            OperationKindDto::CreateDirectory,
            || {
                let parent = browser
                    .resolve_entries(vec![request.entry])
                    .map_err(map_browse_error)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| error(OperationErrorCode::EmptySelection))?;
                Ok(OperationRequest::CreateDirectory {
                    parent,
                    name: OsString::from(request.name),
                })
            },
            Box::new(|_| {}),
        )
    }

    pub fn trash(
        &self,
        browser: &BrowserService,
        request: TrashRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        self.logged_request(
            OperationKindDto::Trash,
            || {
                let sources = browser
                    .resolve_selection(request.entries)
                    .map_err(map_browse_error)?;
                Ok(OperationRequest::Trash { sources })
            },
            Box::new(|_| {}),
        )
    }

    pub fn submit(&self, request: OperationRequest) -> Result<OperationJobDto, OperationErrorDto> {
        self.submit_with_completion(request, |_| {})
    }

    pub fn submit_with_completion<F>(
        &self,
        request: OperationRequest,
        completion: F,
    ) -> Result<OperationJobDto, OperationErrorDto>
    where
        F: FnOnce(&[OperationItemOutcomeDto]) + Send + 'static,
    {
        let kind = request_kind(&request);
        self.logged_request(kind, || Ok(request), Box::new(completion))
    }

    fn logged_request<F>(
        &self,
        kind: OperationKindDto,
        resolve: F,
        callback: CompletionCallback,
    ) -> Result<OperationJobDto, OperationErrorDto>
    where
        F: FnOnce() -> Result<OperationRequest, OperationErrorDto>,
    {
        let id = self.inner.log.begin(kind);
        let request = match resolve() {
            Ok(request) => request,
            Err(error) => {
                self.reject(&id, kind, error);
                publish(&self.inner, true);
                return Err(error);
            }
        };
        let log = self.inner.log.clone();
        let final_id = id.clone();
        let completion = Completion {
            operation_id: id.clone(),
            item_count: request_item_count(&request),
            callback: Box::new(move |outcomes| {
                callback(outcomes);
                log.record(
                    &final_id,
                    None,
                    kind,
                    "finished",
                    Some(outcome_state(outcomes)),
                    outcomes,
                    &[],
                );
            }),
        };
        let result = self.submit_boxed(request, completion).inspect_err(|error| {
            if self.inner.log.result(&id).is_none() {
                self.reject(&id, kind, *error);
            }
        });
        publish(&self.inner, true);
        result
    }
    fn reject(&self, id: &str, kind: OperationKindDto, error: OperationErrorDto) {
        self.inner.log.record(
            id,
            None,
            kind,
            "rejected",
            Some(OperationJobStateDto::Failed),
            &[],
            &[crate::ItemErrorDto {
                index: None,
                domain: "operation".into(),
                code: format!("{:?}", error.code),
            }],
        );
    }

    fn submit_boxed(
        &self,
        request: OperationRequest,
        completion: Completion,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let (source_paths, destination_directories) = journal_paths(&request);
        let plan = match OperationPlanner.plan(request.clone(), &StandardPreflightProbe) {
            Ok(plan) => plan,
            Err(value) => {
                if let PlanValidationErrorKind::Conflict { target_index } = value.kind() {
                    return self.create_pending_conflict(request, *target_index, completion);
                }
                let failure = error(map_plan_error(value.kind()));
                self.reject(&completion.operation_id, request_kind(&request), failure);
                completion.run(&vec![
                    OperationItemOutcomeDto::Failed;
                    request_item_count(&request)
                ]);
                return Err(failure);
            }
        };
        let kind = map_operation_kind(plan.kind());
        let item_count = plan.sources().len().max(plan.targets().len());
        let job_id = {
            let mut queue = self
                .inner
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue
                .enqueue(plan)
                .map_err(|_| error(OperationErrorCode::Io))?
        };
        self.inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                job_id.get(),
                OperationRecord {
                    id: job_id,
                    operation_id: completion.operation_id.clone(),
                    operation_item_count: completion.item_count,
                    item_errors: Vec::new(),
                    phase: OperationPhaseDto::Preparing,
                    kind,
                    state: OperationJobStateDto::Queued,
                    item_count,
                    submitted_at: Instant::now(),
                    started_at: None,
                    finished_at: None,
                    progress: None,
                    outcomes: Vec::new(),
                    cancellation_requested: false,
                    source_paths: source_paths.clone(),
                    destination_directories: destination_directories.clone(),
                },
            );
        self.inner.log.record(
            &completion.operation_id,
            Some(job_id.get()),
            kind,
            "queued",
            Some(OperationJobStateDto::Queued),
            &[],
            &[],
        );
        self.inner.recovery.record(
            job_id.get(),
            kind_name(kind),
            "queued",
            "planned",
            &source_paths,
            &destination_directories,
        );
        self.inner
            .cancellations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                job_id.get(),
                Arc::new(panedeck_fs::AtomicCancellation::default()),
            );
        self.inner
            .completions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(job_id.get(), completion);
        publish(&self.inner, true);
        let inner = Arc::clone(&self.inner);
        thread::spawn(move || run_next(inner));
        self.job(job_id.get())
            .ok_or_else(|| error(OperationErrorCode::Io))
    }

    #[must_use]
    pub fn jobs(&self) -> Vec<OperationJobDto> {
        let records = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut jobs: Vec<_> = records.values().map(snapshot).collect();
        jobs.extend(
            self.inner
                .external_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .cloned(),
        );
        for job in &mut jobs {
            if let Some((state, outcomes)) = self.inner.log.result(&job.operation_id) {
                job.operation_state = Some(state);
                job.operation_outcomes = Some(outcomes);
            }
        }
        jobs.sort_by_key(|job| std::cmp::Reverse(job.job_id));
        jobs
    }

    #[must_use]
    pub fn job(&self, job_id: u64) -> Option<OperationJobDto> {
        self.jobs().into_iter().find(|job| job.job_id == job_id)
    }

    fn create_pending_conflict(
        &self,
        request: OperationRequest,
        target_index: usize,
        completion: Completion,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let item_count = request_item_count(&request);
        let (request, remaining_request) = split_conflict_request(request, target_index);
        let (kind, source, target) =
            conflict_paths(&request, 0).ok_or_else(|| error(OperationErrorCode::Conflict))?;
        let mut session =
            ConflictSession::new(kind).map_err(|_| error(OperationErrorCode::Unsupported))?;
        let conflict = match session
            .encounter(0, source, target, &StandardPreflightProbe)
            .map_err(|_| error(OperationErrorCode::Conflict))?
        {
            ConflictResolution::AwaitingDecision(conflict) => conflict,
            _ => return Err(error(OperationErrorCode::Conflict)),
        };
        let job_id = self.inner.next_pending_id.fetch_add(1, Ordering::Relaxed);
        let conflict_dto = conflict_dto(&conflict)?;
        let kind_dto = map_operation_kind(kind);
        let job = OperationJobDto {
            job_id,
            operation_id: completion.operation_id.clone(),
            operation_item_count: completion.item_count,
            operation_state: None,
            operation_outcomes: None,
            phase: OperationPhaseDto::Preparing,
            item_errors: Vec::new(),
            kind: kind_dto,
            state: OperationJobStateDto::AwaitingDecision,
            item_count,
            completed_items: 0,
            failed_items: 0,
            completed_units: None,
            total_units: None,
            bytes_per_second: None,
            elapsed_millis: 0,
            item_outcomes: Vec::new(),
            can_cancel: true,
            conflict: Some(conflict_dto),
        };
        self.inner.log.record(
            &completion.operation_id,
            Some(job_id),
            kind_dto,
            "conflict",
            Some(OperationJobStateDto::AwaitingDecision),
            &[],
            &[],
        );
        let (journal_sources, journal_destinations) = journal_paths(&request);
        self.inner
            .pending_conflicts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                job_id,
                PendingConflict {
                    request,
                    remaining_request,
                    conflict,
                    session,
                    completion,
                },
            );
        self.inner
            .external_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(job_id, job.clone());
        self.inner.recovery.record(
            job_id,
            kind_name(kind_dto),
            "awaitingDecision",
            "conflict-detected",
            &journal_sources,
            &journal_destinations,
        );
        publish(&self.inner, true);
        Ok(job)
    }

    pub fn resolve_conflict(
        &self,
        request: ResolveConflictRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let result = self.resolve_conflict_inner(request);
        publish(&self.inner, true);
        result
    }
    fn resolve_conflict_inner(
        &self,
        request: ResolveConflictRequestDto,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let mut pending = self
            .inner
            .pending_conflicts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request.job_id)
            .ok_or_else(|| error(OperationErrorCode::StaleReference))?;
        self.inner.log.record(
            &pending.completion.operation_id,
            Some(request.job_id),
            map_operation_kind(pending.session.operation()),
            "decision",
            None,
            &[],
            &[],
        );
        let decision = match request.decision {
            ConflictDecisionDto::Skip => ConflictDecision::Skip,
            ConflictDecisionDto::Replace => ConflictDecision::Replace,
            ConflictDecisionDto::KeepBoth => ConflictDecision::KeepBoth,
            ConflictDecisionDto::Cancel => ConflictDecision::Cancel,
        };
        let preferred = request.apply_to_remaining.then_some(request.decision);
        let scope = if request.apply_to_remaining {
            ConflictScope::RemainingConflicts
        } else {
            ConflictScope::ThisConflict
        };
        let resolution = match pending.session.decide(
            pending.conflict.clone(),
            decision,
            scope,
            &StandardPreflightProbe,
        ) {
            Ok(resolution) => resolution,
            Err(_) => {
                self.inner
                    .pending_conflicts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(request.job_id, pending);
                return Err(error(OperationErrorCode::Conflict));
            }
        };
        match resolution {
            ConflictResolution::AwaitingDecision(conflict) => {
                let dto = conflict_dto(&conflict)?;
                pending.conflict = conflict;
                self.inner
                    .pending_conflicts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(request.job_id, pending);
                let mut jobs = self
                    .inner
                    .external_jobs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let job = jobs
                    .get_mut(&request.job_id)
                    .ok_or_else(|| error(OperationErrorCode::StaleReference))?;
                job.conflict = Some(dto);
                Ok(job.clone())
            }
            ConflictResolution::NoConflict { .. } => {
                self.record_pending_state(
                    request.job_id,
                    &pending,
                    "completed",
                    "conflict-vacated",
                );
                self.inner
                    .external_jobs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&request.job_id);
                let completion = self.continuation_completion(
                    pending.remaining_request,
                    pending.completion,
                    Vec::new(),
                    preferred,
                );
                self.submit_followup(pending.request, completion, preferred)
            }
            ConflictResolution::Ready(action) => {
                self.submit_conflict_action(request.job_id, pending, action, preferred)
            }
            ConflictResolution::Skipped { .. } => {
                self.record_pending_state(request.job_id, &pending, "completed", "skipped");
                let outcomes = vec![OperationItemOutcomeDto::Skipped];
                if let Some(remaining) = pending.remaining_request {
                    self.remove_external_job(request.job_id);
                    let completion =
                        self.continuation_completion(None, pending.completion, outcomes, preferred);
                    self.submit_followup(remaining, completion, preferred)
                } else {
                    pending.completion.run(&outcomes);
                    self.finish_external(request.job_id, OperationJobStateDto::Completed, outcomes)
                }
            }
            ConflictResolution::Cancelled => {
                self.record_pending_state(request.job_id, &pending, "cancelled", "cancelled");
                let outcomes = vec![
                    OperationItemOutcomeDto::Cancelled;
                    1 + pending
                        .remaining_request
                        .as_ref()
                        .map_or(0, request_item_count)
                ];
                pending.completion.run(&outcomes);
                self.finish_external(request.job_id, OperationJobStateDto::Cancelled, outcomes)
            }
        }
    }

    fn finish_external(
        &self,
        job_id: u64,
        state: OperationJobStateDto,
        outcomes: Vec<OperationItemOutcomeDto>,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let mut jobs = self
            .inner
            .external_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let job = jobs
            .get_mut(&job_id)
            .ok_or_else(|| error(OperationErrorCode::StaleReference))?;
        job.state = state;
        job.completed_items = outcomes
            .iter()
            .filter(|outcome| {
                matches!(
                    outcome,
                    OperationItemOutcomeDto::Succeeded | OperationItemOutcomeDto::Skipped
                )
            })
            .count();
        job.failed_items = outcomes.len().saturating_sub(job.completed_items);
        job.item_outcomes = outcomes;
        self.inner.log.record(
            &job.operation_id,
            Some(job_id),
            job.kind,
            "jobFinished",
            Some(state),
            &job.item_outcomes,
            &job.item_errors,
        );
        job.phase = OperationPhaseDto::Finished;
        job.can_cancel = false;
        job.conflict = None;
        Ok(job.clone())
    }

    fn submit_conflict_action(
        &self,
        pending_job_id: u64,
        pending: PendingConflict,
        action: ConflictAction,
        preferred: Option<ConflictDecisionDto>,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let kind = pending.session.operation();
        let source = pending.conflict.source().clone();
        let target = action.target().clone();
        let (journal_sources, journal_destinations) = journal_paths(&pending.request);
        let recovery = self.inner.recovery.clone();
        let mark_decision_applied = || {
            recovery.record(
                pending_job_id,
                kind_name(map_operation_kind(kind)),
                "completed",
                "decision-applied",
                &journal_sources,
                &journal_destinations,
            );
        };
        let completion = self.continuation_completion(
            pending.remaining_request,
            pending.completion,
            Vec::new(),
            preferred,
        );
        match action {
            ConflictAction::KeepBoth(_) => {
                let result = self.submit_boxed(exact_request(kind, source, target), completion);
                if result.is_ok() {
                    mark_decision_applied();
                    self.remove_external_job(pending_job_id);
                }
                result
            }
            ConflictAction::ReplaceFile(_) => {
                let original_target = pending.conflict.target().as_path().to_owned();
                let expected_target = pending.conflict.target_identity();
                let backup = replacement_backup(&original_target, pending_job_id)?;
                panedeck_fs::rename_no_replace(&original_target, &backup)
                    .map_err(|_| error(OperationErrorCode::Io))?;
                let moved_identity = StandardPreflightProbe
                    .inspect(&backup)
                    .ok()
                    .flatten()
                    .and_then(|state| state.identity);
                if moved_identity != Some(expected_target) {
                    let _ = panedeck_fs::rename_no_replace(&backup, &original_target);
                    return Err(error(OperationErrorCode::Conflict));
                }
                let target_for_restore = original_target.clone();
                let backup_for_completion = backup.clone();
                let result = self
                    .submit_boxed(
                        exact_request(
                            kind,
                            source,
                            panedeck_domain::NativePath::new(original_target.clone()),
                        ),
                        completion.wrap(move |outcomes, completion| {
                            let succeeded = outcomes
                                .iter()
                                .all(|outcome| *outcome == OperationItemOutcomeDto::Succeeded);
                            if succeeded {
                                remove_entry(&backup_for_completion);
                            } else if fs::symlink_metadata(&target_for_restore).is_err() {
                                let _ = fs::rename(&backup_for_completion, &target_for_restore);
                            }
                            completion.run(outcomes);
                        }),
                    )
                    .inspect_err(|_| {
                        if fs::symlink_metadata(&original_target).is_err() {
                            let _ = fs::rename(&backup, &original_target);
                        }
                    });
                if result.is_ok() {
                    mark_decision_applied();
                    self.remove_external_job(pending_job_id);
                }
                result
            }
            ConflictAction::MergeDirectory(_) => {
                let children = fs::read_dir(source.as_path())
                    .map_err(|_| error(OperationErrorCode::Io))?
                    .filter_map(Result::ok)
                    .map(|entry| panedeck_domain::NativePath::new(entry.path()))
                    .collect::<Vec<_>>();
                if children.is_empty() {
                    if kind != OperationKind::Copy {
                        let _ = fs::remove_dir(source.as_path());
                    }
                    let outcomes = vec![OperationItemOutcomeDto::Succeeded];
                    completion.run(&outcomes);
                    mark_decision_applied();
                    return self.finish_external(
                        pending_job_id,
                        OperationJobStateDto::Completed,
                        outcomes,
                    );
                }
                let merged_request = match kind {
                    OperationKind::Copy => OperationRequest::Copy {
                        sources: children,
                        destination_directory: target,
                    },
                    OperationKind::Move | OperationKind::Rename => OperationRequest::Move {
                        sources: children,
                        destination_directory: target,
                    },
                    _ => return Err(error(OperationErrorCode::Unsupported)),
                };
                let source_directory = source.as_path().to_owned();
                let result = self.submit_followup(
                    merged_request,
                    completion.wrap(move |outcomes, completion| {
                        if kind != OperationKind::Copy
                            && outcomes.iter().all(|outcome| {
                                matches!(
                                    outcome,
                                    OperationItemOutcomeDto::Succeeded
                                        | OperationItemOutcomeDto::Skipped
                                )
                            })
                        {
                            let _ = fs::remove_dir(&source_directory);
                        }
                        completion.run(outcomes);
                    }),
                    preferred,
                );
                if result.is_ok() {
                    mark_decision_applied();
                    self.remove_external_job(pending_job_id);
                }
                result
            }
        }
    }

    fn remove_external_job(&self, job_id: u64) {
        self.inner
            .external_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&job_id);
    }

    fn record_pending_state(
        &self,
        job_id: u64,
        pending: &PendingConflict,
        state: &str,
        checkpoint: &str,
    ) {
        let (sources, destinations) = journal_paths(&pending.request);
        self.inner.recovery.record(
            job_id,
            kind_name(map_operation_kind(pending.session.operation())),
            state,
            checkpoint,
            &sources,
            &destinations,
        );
    }

    fn submit_followup(
        &self,
        request: OperationRequest,
        completion: Completion,
        preferred: Option<ConflictDecisionDto>,
    ) -> Result<OperationJobDto, OperationErrorDto> {
        let job = self.submit_boxed(request, completion)?;
        if job.state == OperationJobStateDto::AwaitingDecision {
            if let Some(decision) = preferred {
                return self.resolve_conflict(ResolveConflictRequestDto {
                    job_id: job.job_id,
                    decision,
                    apply_to_remaining: true,
                });
            }
        }
        Ok(job)
    }

    fn continuation_completion(
        &self,
        remaining: Option<OperationRequest>,
        completion: Completion,
        mut prefix: Vec<OperationItemOutcomeDto>,
        preferred: Option<ConflictDecisionDto>,
    ) -> Completion {
        let service = self.clone();
        completion.wrap(move |outcomes, completion| {
            prefix.extend_from_slice(outcomes);
            if let Some(remaining) = remaining {
                if outcomes.contains(&OperationItemOutcomeDto::Cancelled) {
                    prefix.extend(vec![
                        OperationItemOutcomeDto::Cancelled;
                        request_item_count(&remaining)
                    ]);
                    completion.run(&prefix);
                    return;
                }
                let collected = prefix;
                let final_completion = completion.wrap(move |remaining_outcomes, completion| {
                    let mut combined = collected;
                    combined.extend_from_slice(remaining_outcomes);
                    completion.run(&combined);
                });
                let _ = service.submit_followup(remaining, final_completion, preferred);
            } else {
                completion.run(&prefix);
            }
        })
    }

    pub fn cancel(&self, job_id: u64) -> Result<OperationJobDto, OperationErrorDto> {
        let result = self.cancel_inner(job_id);
        publish(&self.inner, true);
        result
    }
    fn cancel_inner(&self, job_id: u64) -> Result<OperationJobDto, OperationErrorDto> {
        if let Some(pending) = self
            .inner
            .pending_conflicts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&job_id)
        {
            self.record_pending_state(job_id, &pending, "cancelled", "cancelled");
            let outcomes = vec![
                OperationItemOutcomeDto::Cancelled;
                1 + pending
                    .remaining_request
                    .as_ref()
                    .map_or(0, request_item_count)
            ];
            pending.completion.run(&outcomes);
            let mut jobs = self
                .inner
                .external_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let job = jobs
                .get_mut(&job_id)
                .ok_or_else(|| error(OperationErrorCode::StaleReference))?;
            job.state = OperationJobStateDto::Cancelled;
            job.item_outcomes = outcomes;
            self.inner.log.record(
                &job.operation_id,
                Some(job_id),
                job.kind,
                "jobFinished",
                Some(job.state),
                &job.item_outcomes,
                &job.item_errors,
            );
            job.failed_items = job.item_outcomes.len();
            job.phase = OperationPhaseDto::Finished;
            job.can_cancel = false;
            return Ok(job.clone());
        }
        let (id, item_count) = {
            let records = self
                .inner
                .records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let record = records
                .get(&job_id)
                .ok_or_else(|| error(OperationErrorCode::StaleReference))?;
            if !snapshot(record).can_cancel {
                return Err(error(OperationErrorCode::Unsupported));
            }
            (record.id, record.item_count)
        };
        let was_queued = {
            let mut queue = self
                .inner
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let was_queued = queue
                .state(id)
                .is_ok_and(|state| *state == JobState::Queued);
            queue
                .request_cancel(id)
                .map_err(|_| error(OperationErrorCode::Unsupported))?;
            was_queued
        };
        if let Some(cancellation) = self
            .inner
            .cancellations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&job_id)
        {
            cancellation.request();
        }
        let mut records = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let record = records.get_mut(&job_id).expect("record checked above");
        record.cancellation_requested = true;
        record.state = OperationJobStateDto::CancellationRequested;
        if was_queued {
            let outcomes = vec![OperationItemOutcomeDto::Cancelled; item_count];
            drop(records);
            let completion = self
                .inner
                .completions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&job_id);
            if let Some(completion) = completion {
                completion.run(&outcomes);
            }
            let mut records = self
                .inner
                .records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let record = records.get_mut(&job_id).expect("cancelled queued record");
            record.state = OperationJobStateDto::Cancelled;
            record.phase = OperationPhaseDto::Finished;
            record.finished_at = Some(Instant::now());
            record.outcomes = outcomes;
            self.inner.log.record(
                &record.operation_id,
                Some(job_id),
                record.kind,
                "jobFinished",
                Some(record.state),
                &record.outcomes,
                &record.item_errors,
            );
            self.inner.recovery.record(
                job_id,
                kind_name(record.kind),
                "cancelled",
                "cancelled-before-start",
                &record.source_paths,
                &record.destination_directories,
            );
            Ok(snapshot(record))
        } else {
            self.inner.recovery.record(
                job_id,
                kind_name(record.kind),
                "cancellationRequested",
                "awaiting-safe-checkpoint",
                &record.source_paths,
                &record.destination_directories,
            );
            Ok(snapshot(record))
        }
    }
}

fn run_next(inner: Arc<OperationInner>) {
    let _execution = inner
        .execution_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(job_id) = inner
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .claim_next()
    else {
        return;
    };
    {
        let mut records = inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let record = records
            .get_mut(&job_id.get())
            .expect("queued job has record");
        record.state = OperationJobStateDto::Validating;
        record.started_at = Some(Instant::now());
        inner.recovery.record(
            job_id.get(),
            kind_name(record.kind),
            "validating",
            "revalidating-plan",
            &record.source_paths,
            &record.destination_directories,
        );
    }
    publish(&inner, true);
    let plan = inner
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .plan(job_id)
        .expect("claimed job has plan")
        .clone();
    inner
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .mark_validation_succeeded(job_id)
        .expect("claimed job validates");
    if let Some(record) = inner
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_mut(&job_id.get())
    {
        record.state = OperationJobStateDto::Running;
        inner.recovery.record(
            job_id.get(),
            kind_name(record.kind),
            "running",
            "executing",
            &record.source_paths,
            &record.destination_directories,
        );
    }
    publish(&inner, true);
    let cancellation = inner
        .cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&job_id.get())
        .cloned()
        .expect("job cancellation exists");
    let mut item_errors = Vec::new();
    let outcomes = match plan.kind() {
        OperationKind::Copy => {
            let mut observer = OperationObserver {
                inner: &inner,
                job_id,
            };
            CopyExecutor
                .execute(&plan, &StandardPreflightProbe, &cancellation, &mut observer)
                .items
                .into_iter()
                .map(|item| {
                    if let CopyItemOutcome::Failed(code) = item.outcome {
                        item_errors.push(item_error(item.source_index, "copy", code));
                    }
                    match item.outcome {
                        CopyItemOutcome::Copied => OperationItemOutcomeDto::Succeeded,
                        CopyItemOutcome::Failed(panedeck_fs::CopyErrorCode::Cancelled) => {
                            OperationItemOutcomeDto::Cancelled
                        }
                        CopyItemOutcome::Failed(_) => OperationItemOutcomeDto::Failed,
                    }
                })
                .collect()
        }
        OperationKind::Move => {
            let mut observer = OperationObserver {
                inner: &inner,
                job_id,
            };
            MoveExecutor
                .execute(&plan, &StandardPreflightProbe, &cancellation, &mut observer)
                .items
                .into_iter()
                .map(|item| {
                    match item.outcome {
                        MoveItemOutcome::Failed(code)
                        | MoveItemOutcome::CopiedButSourceNotRemoved(code) => {
                            item_errors.push(item_error(item.source_index, "move", code))
                        }
                        _ => {}
                    }
                    match item.outcome {
                        MoveItemOutcome::Moved => OperationItemOutcomeDto::Succeeded,
                        MoveItemOutcome::Failed(panedeck_fs::MoveErrorCode::Cancelled) => {
                            OperationItemOutcomeDto::Cancelled
                        }
                        MoveItemOutcome::CopiedButSourceNotRemoved(_) => {
                            OperationItemOutcomeDto::CopiedButSourceNotRemoved
                        }
                        MoveItemOutcome::Failed(_) => OperationItemOutcomeDto::Failed,
                    }
                })
                .collect()
        }
        OperationKind::Rename | OperationKind::CreateDirectory => vec![match MutationExecutor
            .execute(&plan, &StandardPreflightProbe)
        {
            Ok(()) => OperationItemOutcomeDto::Succeeded,
            Err(code) => {
                item_errors.push(item_error(0, "mutation", code));
                OperationItemOutcomeDto::Failed
            }
        }],
        OperationKind::Trash => TrashExecutor
            .execute(&plan, &StandardPreflightProbe, &SystemTrashAdapter)
            .items
            .into_iter()
            .map(|item| match item.outcome {
                TrashItemOutcome::Trashed => OperationItemOutcomeDto::Succeeded,
                TrashItemOutcome::Failed(code) => {
                    item_errors.push(item_error(item.source_index, "trash", code));
                    OperationItemOutcomeDto::Failed
                }
            })
            .collect(),
    };
    let completed_items = outcomes
        .iter()
        .filter(|outcome| **outcome == OperationItemOutcomeDto::Succeeded)
        .count();
    let failed_items = outcomes.len().saturating_sub(completed_items);
    let all_cancelled = !outcomes.is_empty()
        && outcomes
            .iter()
            .all(|outcome| *outcome == OperationItemOutcomeDto::Cancelled);
    let mut queue = inner
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = if all_cancelled
        && queue.cancellation_checkpoint(job_id).ok() == Some(CancellationCheckpoint::Stop)
    {
        OperationJobStateDto::Cancelled
    } else if failed_items == 0 {
        queue.mark_completed(job_id).expect("running job completes");
        OperationJobStateDto::Completed
    } else if completed_items > 0 {
        queue
            .mark_partially_failed(job_id, completed_items as u64, failed_items as u64)
            .expect("running job partially fails");
        OperationJobStateDto::PartiallyFailed
    } else {
        queue
            .mark_failed(job_id, JobFailureCode::Execution)
            .expect("running job fails");
        OperationJobStateDto::Failed
    };
    drop(queue);
    inner
        .cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&job_id.get());
    let completion = inner
        .completions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&job_id.get());
    // Completion updates dependent state such as the cut clipboard. Publish
    // the terminal snapshot only after those updates finish, without holding
    // service locks while a callback may submit a follow-up operation.
    if let Some(completion) = completion {
        completion.run(&outcomes);
    }
    if let Some(record) = inner
        .records
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_mut(&job_id.get())
    {
        record.item_errors = item_errors;
        record.phase = OperationPhaseDto::Finished;
        record.state = state;
        record.finished_at = Some(Instant::now());
        record.outcomes.clone_from(&outcomes);
        inner.log.record(
            &record.operation_id,
            Some(job_id.get()),
            record.kind,
            "jobFinished",
            Some(state),
            &outcomes,
            &record.item_errors,
        );
        inner.recovery.record(
            job_id.get(),
            kind_name(record.kind),
            state_name(state),
            if matches!(state, OperationJobStateDto::Completed) {
                "commit-complete"
            } else {
                "execution-stopped"
            },
            &record.source_paths,
            &record.destination_directories,
        );
    }
    publish(&inner, true);
}

fn conflict_paths(
    request: &OperationRequest,
    target_index: usize,
) -> Option<(
    OperationKind,
    panedeck_domain::NativePath,
    panedeck_domain::NativePath,
)> {
    match request {
        OperationRequest::Copy {
            sources,
            destination_directory,
        }
        | OperationRequest::Move {
            sources,
            destination_directory,
        } => {
            let source = sources.get(target_index)?.clone();
            let name = source.as_path().file_name()?;
            let target =
                panedeck_domain::NativePath::new(destination_directory.as_path().join(name));
            let kind = if matches!(request, OperationRequest::Copy { .. }) {
                OperationKind::Copy
            } else {
                OperationKind::Move
            };
            Some((kind, source, target))
        }
        OperationRequest::CopyTo { source, target } => {
            Some((OperationKind::Copy, source.clone(), target.clone()))
        }
        OperationRequest::MoveTo { source, target } => {
            Some((OperationKind::Move, source.clone(), target.clone()))
        }
        OperationRequest::Rename { source, new_name } => Some((
            OperationKind::Rename,
            source.clone(),
            panedeck_domain::NativePath::new(source.as_path().parent()?.join(new_name)),
        )),
        OperationRequest::RenameTo { source, target } => {
            Some((OperationKind::Rename, source.clone(), target.clone()))
        }
        OperationRequest::CreateDirectory { .. } | OperationRequest::Trash { .. } => None,
    }
}

fn split_conflict_request(
    request: OperationRequest,
    target_index: usize,
) -> (OperationRequest, Option<OperationRequest>) {
    match request {
        OperationRequest::Copy {
            mut sources,
            destination_directory,
        } => {
            let source = sources.remove(target_index);
            let remaining = (!sources.is_empty()).then(|| OperationRequest::Copy {
                sources,
                destination_directory: destination_directory.clone(),
            });
            (
                OperationRequest::Copy {
                    sources: vec![source],
                    destination_directory,
                },
                remaining,
            )
        }
        OperationRequest::Move {
            mut sources,
            destination_directory,
        } => {
            let source = sources.remove(target_index);
            let remaining = (!sources.is_empty()).then(|| OperationRequest::Move {
                sources,
                destination_directory: destination_directory.clone(),
            });
            (
                OperationRequest::Move {
                    sources: vec![source],
                    destination_directory,
                },
                remaining,
            )
        }
        other => (other, None),
    }
}

fn conflict_dto(conflict: &Conflict) -> Result<OperationConflictDto, OperationErrorDto> {
    Ok(OperationConflictDto {
        kind: match conflict.kind() {
            ConflictKind::FileReplacement => ConflictKindDto::FileReplacement,
            ConflictKind::DirectoryMerge => ConflictKindDto::DirectoryMerge,
            ConflictKind::TypeMismatch => ConflictKindDto::TypeMismatch,
        },
        source: conflict_entry(conflict.source().as_path())?,
        target: conflict_entry(conflict.target().as_path())?,
        replace_allowed: conflict.kind() != ConflictKind::TypeMismatch,
    })
}

fn conflict_entry(path: &std::path::Path) -> Result<ConflictEntryDto, OperationErrorDto> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error(OperationErrorCode::Io))?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        "directory"
    } else if file_type.is_file() {
        "file"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "other"
    };
    Ok(ConflictEntryDto {
        name: path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        parent_context: path
            .parent()
            .map_or_else(String::new, |parent| parent.to_string_lossy().into_owned()),
        kind: kind.to_owned(),
        byte_len: metadata.len(),
        modified_millis: metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_millis().min(u128::from(u64::MAX)) as u64),
    })
}

fn request_item_count(request: &OperationRequest) -> usize {
    match request {
        OperationRequest::Copy { sources, .. }
        | OperationRequest::Move { sources, .. }
        | OperationRequest::Trash { sources } => sources.len(),
        _ => 1,
    }
}

fn exact_request(
    kind: OperationKind,
    source: panedeck_domain::NativePath,
    target: panedeck_domain::NativePath,
) -> OperationRequest {
    match kind {
        OperationKind::Copy => OperationRequest::CopyTo { source, target },
        OperationKind::Move => OperationRequest::MoveTo { source, target },
        OperationKind::Rename => OperationRequest::RenameTo { source, target },
        _ => unreachable!("only conflict-capable operations have exact targets"),
    }
}

fn replacement_backup(target: &std::path::Path, job_id: u64) -> Result<PathBuf, OperationErrorDto> {
    let parent = target.parent().unwrap_or_else(|| std::path::Path::new("/"));
    for attempt in 0..10_000_u32 {
        let candidate = parent.join(format!(
            ".panedeck-replaced-{job_id}-{}-{attempt}.backup",
            std::process::id()
        ));
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(error(OperationErrorCode::Io))
}

fn remove_entry(path: &std::path::Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn journal_paths(request: &OperationRequest) -> (Vec<PathBuf>, Vec<PathBuf>) {
    match request {
        OperationRequest::Copy {
            sources,
            destination_directory,
        }
        | OperationRequest::Move {
            sources,
            destination_directory,
        } => (
            sources
                .iter()
                .map(|path| path.as_path().to_owned())
                .collect(),
            vec![destination_directory.as_path().to_owned()],
        ),
        OperationRequest::CopyTo { source, target }
        | OperationRequest::MoveTo { source, target }
        | OperationRequest::RenameTo { source, target } => (
            vec![source.as_path().to_owned()],
            target
                .as_path()
                .parent()
                .map(PathBuf::from)
                .into_iter()
                .collect(),
        ),
        OperationRequest::Rename { source, .. } => (
            vec![source.as_path().to_owned()],
            source
                .as_path()
                .parent()
                .map(PathBuf::from)
                .into_iter()
                .collect(),
        ),
        OperationRequest::CreateDirectory { parent, .. } => {
            (Vec::new(), vec![parent.as_path().to_owned()])
        }
        OperationRequest::Trash { sources } => (
            sources
                .iter()
                .map(|path| path.as_path().to_owned())
                .collect(),
            Vec::new(),
        ),
    }
}

const fn kind_name(kind: OperationKindDto) -> &'static str {
    match kind {
        OperationKindDto::Copy => "copy",
        OperationKindDto::Move => "move",
        OperationKindDto::Rename => "rename",
        OperationKindDto::CreateDirectory => "createDirectory",
        OperationKindDto::Trash => "trash",
    }
}

const fn state_name(state: OperationJobStateDto) -> &'static str {
    match state {
        OperationJobStateDto::Queued => "queued",
        OperationJobStateDto::Validating => "validating",
        OperationJobStateDto::Running => "running",
        OperationJobStateDto::AwaitingDecision => "awaitingDecision",
        OperationJobStateDto::CancellationRequested => "cancellationRequested",
        OperationJobStateDto::Completed => "completed",
        OperationJobStateDto::PartiallyFailed => "partiallyFailed",
        OperationJobStateDto::Failed => "failed",
        OperationJobStateDto::Cancelled => "cancelled",
    }
}

fn snapshot(record: &OperationRecord) -> OperationJobDto {
    let started = record.started_at.unwrap_or(record.submitted_at);
    let elapsed = record
        .finished_at
        .unwrap_or_else(Instant::now)
        .saturating_duration_since(started);
    let completed_items = record
        .outcomes
        .iter()
        .filter(|outcome| **outcome == OperationItemOutcomeDto::Succeeded)
        .count();
    let failed_items = record.outcomes.len().saturating_sub(completed_items);
    let progress = record.progress;
    OperationJobDto {
        job_id: record.id.get(),
        operation_id: record.operation_id.clone(),
        operation_item_count: record.operation_item_count,
        operation_state: None,
        operation_outcomes: None,
        phase: record.phase,
        item_errors: record.item_errors.clone(),
        kind: record.kind,
        state: record.state,
        item_count: record.item_count,
        completed_items,
        failed_items,
        completed_units: progress.map(JobProgress::completed_units),
        total_units: progress
            .and_then(|value| (value.total_units() > 0).then_some(value.total_units())),
        bytes_per_second: progress.and_then(|value| {
            let seconds = elapsed.as_secs_f64();
            (seconds > 0.0).then_some((value.completed_units() as f64 / seconds) as u64)
        }),
        elapsed_millis: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
        item_outcomes: record.outcomes.clone(),
        can_cancel: !matches!(
            record.phase,
            OperationPhaseDto::Verifying
                | OperationPhaseDto::CleaningSource
                | OperationPhaseDto::Finished
        ) && matches!(
            record.state,
            OperationJobStateDto::Queued | OperationJobStateDto::Running
        ) && !record.cancellation_requested,
        conflict: None,
    }
}

fn map_operation_kind(kind: OperationKind) -> OperationKindDto {
    match kind {
        OperationKind::Copy => OperationKindDto::Copy,
        OperationKind::Move => OperationKindDto::Move,
        OperationKind::Rename => OperationKindDto::Rename,
        OperationKind::CreateDirectory => OperationKindDto::CreateDirectory,
        OperationKind::Trash => OperationKindDto::Trash,
    }
}

fn map_plan_error(kind: &PlanValidationErrorKind) -> OperationErrorCode {
    match kind {
        PlanValidationErrorKind::EmptySources => OperationErrorCode::EmptySelection,
        PlanValidationErrorKind::InvalidName => OperationErrorCode::InvalidName,
        PlanValidationErrorKind::MissingSource { .. }
        | PlanValidationErrorKind::SourceChanged { .. } => OperationErrorCode::SourceChanged,
        PlanValidationErrorKind::MissingDestination
        | PlanValidationErrorKind::DestinationNotDirectory => OperationErrorCode::StaleReference,
        PlanValidationErrorKind::SourceNotReadable { .. }
        | PlanValidationErrorKind::ParentNotWritable
        | PlanValidationErrorKind::ParentChanged { .. } => OperationErrorCode::PermissionDenied,
        PlanValidationErrorKind::SamePath { .. } => OperationErrorCode::SamePath,
        PlanValidationErrorKind::DirectoryIntoItself { .. } => {
            OperationErrorCode::DirectoryIntoItself
        }
        PlanValidationErrorKind::Conflict { .. }
        | PlanValidationErrorKind::TargetChanged { .. } => OperationErrorCode::Conflict,
        PlanValidationErrorKind::InsufficientSpace { .. } => OperationErrorCode::InsufficientSpace,
        PlanValidationErrorKind::IdentityUnavailable => OperationErrorCode::Unsupported,
        PlanValidationErrorKind::ProbeFailed(panedeck_fs::ProbeError::PermissionDenied) => {
            OperationErrorCode::PermissionDenied
        }
        PlanValidationErrorKind::ProbeFailed(_) => OperationErrorCode::Io,
    }
}

fn map_browse_error(_: BrowseErrorDto) -> OperationErrorDto {
    error(OperationErrorCode::StaleReference)
}

const fn error(code: OperationErrorCode) -> OperationErrorDto {
    OperationErrorDto { code }
}

fn validate_transfer_paths(request: &OperationRequest) -> Result<(), OperationErrorDto> {
    let (sources, destination, moving) = match request {
        OperationRequest::Copy {
            sources,
            destination_directory,
        } => (sources, destination_directory, false),
        OperationRequest::Move {
            sources,
            destination_directory,
        } => (sources, destination_directory, true),
        _ => return Err(error(OperationErrorCode::Unsupported)),
    };
    let dest = fs::canonicalize(destination.as_path())
        .map_err(|_| error(OperationErrorCode::StaleReference))?;
    let state = StandardPreflightProbe
        .inspect(&dest)
        .map_err(|_| error(OperationErrorCode::Io))?
        .ok_or_else(|| error(OperationErrorCode::StaleReference))?;
    if state.kind != panedeck_domain::EntryKind::Directory {
        return Err(error(OperationErrorCode::Unsupported));
    }
    if !state.is_writable {
        return Err(error(OperationErrorCode::PermissionDenied));
    }
    for source in sources {
        let metadata = fs::symlink_metadata(source.as_path())
            .map_err(|_| error(OperationErrorCode::StaleReference))?;
        let parent = source
            .as_path()
            .parent()
            .ok_or_else(|| error(OperationErrorCode::Unsupported))?;
        let parent =
            fs::canonicalize(parent).map_err(|_| error(OperationErrorCode::StaleReference))?;
        if parent == dest {
            return Err(error(OperationErrorCode::SamePath));
        }
        if metadata.is_dir() {
            let canonical = fs::canonicalize(source.as_path())
                .map_err(|_| error(OperationErrorCode::StaleReference))?;
            if dest.starts_with(canonical) {
                return Err(error(OperationErrorCode::DirectoryIntoItself));
            }
        }
        if moving
            && !StandardPreflightProbe
                .inspect(&parent)
                .map_err(|_| error(OperationErrorCode::Io))?
                .is_some_and(|value| value.is_writable)
        {
            return Err(error(OperationErrorCode::PermissionDenied));
        }
    }
    Ok(())
}

fn request_kind(request: &OperationRequest) -> OperationKindDto {
    match request {
        OperationRequest::Copy { .. } | OperationRequest::CopyTo { .. } => OperationKindDto::Copy,
        OperationRequest::Move { .. } | OperationRequest::MoveTo { .. } => OperationKindDto::Move,
        OperationRequest::Rename { .. } | OperationRequest::RenameTo { .. } => {
            OperationKindDto::Rename
        }
        OperationRequest::CreateDirectory { .. } => OperationKindDto::CreateDirectory,
        OperationRequest::Trash { .. } => OperationKindDto::Trash,
    }
}
fn outcome_state(outcomes: &[OperationItemOutcomeDto]) -> OperationJobStateDto {
    if !outcomes.is_empty()
        && outcomes
            .iter()
            .all(|o| *o == OperationItemOutcomeDto::Cancelled)
    {
        OperationJobStateDto::Cancelled
    } else if outcomes.iter().all(|o| {
        matches!(
            o,
            OperationItemOutcomeDto::Succeeded | OperationItemOutcomeDto::Skipped
        )
    }) {
        OperationJobStateDto::Completed
    } else if outcomes.iter().any(|o| {
        matches!(
            o,
            OperationItemOutcomeDto::Succeeded | OperationItemOutcomeDto::Skipped
        )
    }) {
        OperationJobStateDto::PartiallyFailed
    } else {
        OperationJobStateDto::Failed
    }
}
fn item_error(index: usize, domain: &str, code: impl std::fmt::Debug) -> crate::ItemErrorDto {
    crate::ItemErrorDto {
        index: Some(index),
        domain: domain.into(),
        code: format!("{code:?}"),
    }
}

fn publish(inner: &Arc<OperationInner>, force: bool) {
    let events: Vec<_> = inner
        .queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain_events()
        .collect();
    let mut hub = inner
        .hub
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for event in events {
        hub.throttle.push(event, Instant::now());
    }
    hub.throttle.flush_due(Instant::now());
    let ready = hub.throttle.drain_ready().count() > 0;
    if (!force && !ready) || hub.watchers.is_empty() {
        return;
    }
    hub.sequence += 1;
    let snapshot = OperationService {
        inner: Arc::clone(inner),
    }
    .state_snapshot(hub.sequence);
    let watchers: Vec<_> = hub.watchers.values().cloned().collect();
    drop(hub);
    for watcher in watchers {
        watcher(snapshot.clone());
    }
}
struct OperationObserver<'a> {
    inner: &'a Arc<OperationInner>,
    job_id: JobId,
}
impl panedeck_fs::CopyProgressObserver for OperationObserver<'_> {
    fn on_progress(&mut self, progress: panedeck_fs::CopyProgress) {
        let progress = JobProgress::new(progress.bytes_copied, progress.total_bytes, 0)
            .expect("bounded executor progress");
        let _ = self
            .inner
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .report_progress(self.job_id, progress);
        if let Some(record) = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&self.job_id.get())
        {
            record.progress = Some(progress);
        }
        publish(self.inner, false);
    }
    fn on_phase(&mut self, phase: panedeck_fs::TransferPhase) {
        let phase = match phase {
            panedeck_fs::TransferPhase::Preparing => OperationPhaseDto::Preparing,
            panedeck_fs::TransferPhase::Copying => OperationPhaseDto::Copying,
            panedeck_fs::TransferPhase::Verifying => OperationPhaseDto::Verifying,
            panedeck_fs::TransferPhase::CleaningSource => OperationPhaseDto::CleaningSource,
        };
        if let Some(record) = self
            .inner
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&self.job_id.get())
        {
            record.phase = phase;
        }
        publish(self.inner, true);
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use tempfile::TempDir;

    use super::*;
    use crate::{NavigateDirectoryRequestDto, PaneIdDto, SortDirectionDto, SortFieldDto};

    async fn open(
        browser: &BrowserService,
        pane: PaneIdDto,
        path: &std::path::Path,
    ) -> crate::DirectoryListingDto {
        browser
            .open_path(NavigateDirectoryRequestDto {
                pane,
                path: path.to_string_lossy().into_owned(),
                show_hidden: false,
                sort_field: SortFieldDto::Name,
                sort_direction: SortDirectionDto::Ascending,
            })
            .await
            .expect("listing")
    }

    fn wait_for_terminal(service: &OperationService, job_id: u64) -> OperationJobDto {
        for _ in 0..500 {
            let job = service.job(job_id).expect("job snapshot");
            if matches!(
                job.state,
                OperationJobStateDto::Completed
                    | OperationJobStateDto::PartiallyFailed
                    | OperationJobStateDto::Failed
                    | OperationJobStateDto::Cancelled
            ) {
                return job;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("job did not finish");
    }

    #[test]
    fn events_and_logs_preserve_runtime_errors_and_publish_terminal_after_callback() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("file");
        let dest = temp.path().join("dest");
        fs::write(&source, b"data").unwrap();
        fs::create_dir(&dest).unwrap();
        let service = OperationService::with_logs(
            RecoveryService::default(),
            crate::OperationLog::new(temp.path().join("operations.jsonl")),
        );
        let received = Arc::new(Mutex::new(Vec::new()));
        let events = received.clone();
        let (watch, initial) = service.watch(move |event| events.lock().unwrap().push(event));
        assert!(initial.jobs.is_empty());
        let gate = service.inner.execution_lock.lock().unwrap();
        let job = service
            .submit(OperationRequest::Copy {
                sources: vec![panedeck_domain::NativePath::new(source.clone())],
                destination_directory: panedeck_domain::NativePath::new(dest),
            })
            .unwrap();
        fs::remove_file(source).unwrap();
        drop(gate);
        let done = wait_for_terminal(&service, job.job_id);
        assert_eq!(done.state, OperationJobStateDto::Failed);
        assert!(done.item_errors.iter().any(|e| e.code == "SourceChanged"));
        assert!(service
            .log()
            .recent()
            .iter()
            .any(|e| e.operation_id == job.operation_id
                && e.errors.iter().any(|e| e.code == "SourceChanged")));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !received.lock().unwrap().iter().any(|e| {
            e.jobs
                .iter()
                .any(|j| j.job_id == job.job_id && j.state == OperationJobStateDto::Failed)
        }) {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        service.unwatch(watch);
        assert!(service.inner.hub.lock().unwrap().watchers.is_empty());
    }
    #[test]
    fn cancelling_a_split_operation_does_not_schedule_remaining_work() {
        let service = OperationService::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let received = observed.clone();
        let completion = Completion {
            operation_id: "cancelled-batch".into(),
            item_count: 2,
            callback: Box::new(move |outcomes| *received.lock().unwrap() = outcomes.to_vec()),
        };
        let remaining = OperationRequest::Copy {
            sources: vec![panedeck_domain::NativePath::new(PathBuf::from(
                "/unreachable",
            ))],
            destination_directory: panedeck_domain::NativePath::new(PathBuf::from(
                "/unreachable-target",
            )),
        };
        service
            .continuation_completion(Some(remaining), completion, vec![], None)
            .run(&[OperationItemOutcomeDto::Cancelled]);
        assert_eq!(
            *observed.lock().unwrap(),
            vec![OperationItemOutcomeDto::Cancelled; 2]
        );
        assert!(service.jobs().is_empty());
        assert!(service.log().recent().is_empty());
    }

    #[test]
    fn queued_cancel_releases_locks_and_runs_callback_before_terminal() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("file");
        let dest = temp.path().join("dest");
        fs::write(&source, b"data").unwrap();
        fs::create_dir(&dest).unwrap();
        let service = OperationService::default();
        let gate = service.inner.execution_lock.lock().unwrap();
        let callback_service = service.clone();
        let observed = Arc::new(Mutex::new(false));
        let checked = observed.clone();
        let job = service
            .submit_with_completion(
                OperationRequest::Copy {
                    sources: vec![panedeck_domain::NativePath::new(source.clone())],
                    destination_directory: panedeck_domain::NativePath::new(dest),
                },
                move |_| {
                    assert!(callback_service
                        .jobs()
                        .iter()
                        .all(|j| j.state != OperationJobStateDto::Cancelled));
                    *checked.lock().unwrap() = true;
                },
            )
            .unwrap();
        let done = service.cancel(job.job_id).unwrap();
        assert!(*observed.lock().unwrap());
        assert_eq!(done.state, OperationJobStateDto::Cancelled);
        assert!(source.exists());
        drop(gate);
    }

    #[test]
    fn terminal_state_waits_for_completion_callback() {
        let temp = TempDir::new().expect("temp dir");
        let service = OperationService::default();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let job = service
            .submit_with_completion(
                OperationRequest::CreateDirectory {
                    parent: panedeck_domain::NativePath::new(temp.path()),
                    name: OsString::from("created"),
                },
                move |_| {
                    entered_tx.send(()).expect("notify callback entry");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release callback");
                },
            )
            .expect("submit job");
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("callback starts");
        let state_during_completion = service.job(job.job_id).expect("job snapshot").state;
        release_tx.send(()).expect("release callback");
        assert_eq!(state_during_completion, OperationJobStateDto::Running);
        assert_eq!(
            wait_for_terminal(&service, job.job_id).state,
            OperationJobStateDto::Completed
        );
    }

    #[tokio::test]
    async fn copy_and_move_use_plans_queue_and_safe_executors() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source");
        let copy_target = temp.path().join("copy-target");
        let move_target = temp.path().join("move-target");
        for directory in [&source, &copy_target, &move_target] {
            fs::create_dir(directory).expect("directory");
        }
        fs::write(source.join("item.txt"), b"payload").expect("source file");
        let browser = BrowserService::default();
        let source_listing = open(&browser, PaneIdDto::Left, &source).await;
        let copy_listing = open(&browser, PaneIdDto::Right, &copy_target).await;
        let service = OperationService::default();
        let copy = service
            .transfer(
                &browser,
                TransferRequestDto {
                    kind: OperationKindDto::Copy,
                    entries: vec![source_listing.entries[0].reference.clone()],
                    destination: TransferDestinationDto::Directory {
                        directory: copy_listing.directory,
                    },
                },
            )
            .expect("copy");
        let copy = wait_for_terminal(&service, copy.job_id);
        assert_eq!(copy.item_outcomes, vec![OperationItemOutcomeDto::Succeeded]);
        assert_eq!(
            fs::read(copy_target.join("item.txt")).expect("copy"),
            b"payload"
        );

        let refreshed_source = open(&browser, PaneIdDto::Left, &source).await;
        let move_listing = open(&browser, PaneIdDto::Right, &move_target).await;
        let moved = service
            .transfer(
                &browser,
                TransferRequestDto {
                    kind: OperationKindDto::Move,
                    entries: vec![refreshed_source.entries[0].reference.clone()],
                    destination: TransferDestinationDto::Directory {
                        directory: move_listing.directory,
                    },
                },
            )
            .expect("move");
        let moved = wait_for_terminal(&service, moved.job_id);
        assert_eq!(moved.completed_items, 1);
        assert!(!source.join("item.txt").exists());
        assert_eq!(
            fs::read(move_target.join("item.txt")).expect("move"),
            b"payload"
        );
        assert!(moved.job_id > copy.job_id);
    }

    #[tokio::test]
    async fn rename_and_create_directory_act_on_resolved_references() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("before.txt"), b"payload").expect("source");
        let browser = BrowserService::default();
        let listing = open(&browser, PaneIdDto::Left, temp.path()).await;
        let service = OperationService::default();
        let renamed = service
            .rename(
                &browser,
                RenameRequestDto {
                    entry: listing.entries[0].reference.clone(),
                    new_name: "after.txt".to_owned(),
                },
            )
            .expect("rename");
        wait_for_terminal(&service, renamed.job_id);
        assert!(temp.path().join("after.txt").is_file());
        let created = service
            .create_directory(
                &browser,
                CreateDirectoryRequestDto {
                    directory: listing.directory,
                    name: "folder".to_owned(),
                },
            )
            .expect("create folder");
        wait_for_terminal(&service, created.job_id);
        assert!(temp.path().join("folder").is_dir());
    }

    #[tokio::test]
    async fn conflicts_wait_for_decision_and_invalid_names_do_not_write() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("source.txt"), b"source").expect("source");
        fs::write(temp.path().join("occupied.txt"), b"occupied").expect("occupied");
        let browser = BrowserService::default();
        let listing = open(&browser, PaneIdDto::Left, temp.path()).await;
        let source = listing
            .entries
            .iter()
            .find(|entry| entry.display_name == "source.txt")
            .expect("source entry");
        let service = OperationService::default();
        let conflict = service
            .rename(
                &browser,
                RenameRequestDto {
                    entry: source.reference.clone(),
                    new_name: "occupied.txt".to_owned(),
                },
            )
            .expect("conflict task");
        assert_eq!(conflict.state, OperationJobStateDto::AwaitingDecision);
        assert_eq!(
            fs::read(temp.path().join("occupied.txt")).expect("occupied"),
            b"occupied"
        );
        let invalid = service.create_directory(
            &browser,
            CreateDirectoryRequestDto {
                directory: listing.directory,
                name: "nested/name".to_owned(),
            },
        );
        assert_eq!(
            invalid.expect_err("invalid name").code,
            OperationErrorCode::InvalidName
        );
        assert!(!temp.path().join("nested").exists());
    }

    #[test]
    fn conflict_waits_for_safe_default_decision_and_keep_both_uses_queue() {
        let temporary = TempDir::new().unwrap();
        let source_parent = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        fs::create_dir_all(&source_parent).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_parent.join("same.txt");
        fs::write(&source, b"incoming").unwrap();
        fs::write(destination.join("same.txt"), b"existing").unwrap();
        let service = OperationService::default();

        let awaiting = service
            .submit(OperationRequest::Copy {
                sources: vec![panedeck_domain::NativePath::new(&source)],
                destination_directory: panedeck_domain::NativePath::new(&destination),
            })
            .unwrap();
        assert_eq!(awaiting.state, OperationJobStateDto::AwaitingDecision);
        assert_eq!(
            awaiting.conflict.as_ref().unwrap().kind,
            ConflictKindDto::FileReplacement
        );
        let queued = service
            .resolve_conflict(ResolveConflictRequestDto {
                job_id: awaiting.job_id,
                decision: ConflictDecisionDto::KeepBoth,
                apply_to_remaining: false,
            })
            .unwrap();
        let completed = wait_for_terminal(&service, queued.job_id);
        assert_eq!(completed.state, OperationJobStateDto::Completed);
        assert_eq!(fs::read(destination.join("same.txt")).unwrap(), b"existing");
        assert_eq!(
            fs::read(destination.join("same copy.txt")).unwrap(),
            b"incoming"
        );
    }

    #[test]
    fn explicit_replace_keeps_source_until_replacement_commits() {
        let temporary = TempDir::new().unwrap();
        let source_parent = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        fs::create_dir_all(&source_parent).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_parent.join("same.txt");
        fs::write(&source, b"incoming").unwrap();
        fs::write(destination.join("same.txt"), b"existing").unwrap();
        let service = OperationService::default();
        let awaiting = service
            .submit(OperationRequest::Copy {
                sources: vec![panedeck_domain::NativePath::new(&source)],
                destination_directory: panedeck_domain::NativePath::new(&destination),
            })
            .unwrap();

        let queued = service
            .resolve_conflict(ResolveConflictRequestDto {
                job_id: awaiting.job_id,
                decision: ConflictDecisionDto::Replace,
                apply_to_remaining: false,
            })
            .unwrap();
        let completed = wait_for_terminal(&service, queued.job_id);
        assert_eq!(completed.state, OperationJobStateDto::Completed);
        assert_eq!(fs::read(&source).unwrap(), b"incoming");
        assert_eq!(fs::read(destination.join("same.txt")).unwrap(), b"incoming");
    }

    #[test]
    fn apply_to_remaining_resolves_every_conflict_in_the_batch() {
        let temporary = TempDir::new().unwrap();
        let source_parent = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        fs::create_dir_all(&source_parent).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let sources = ["one.txt", "two.txt", "three.txt"]
            .into_iter()
            .map(|name| {
                let source = source_parent.join(name);
                fs::write(&source, format!("incoming-{name}")).unwrap();
                fs::write(destination.join(name), format!("existing-{name}")).unwrap();
                panedeck_domain::NativePath::new(source)
            })
            .collect();
        let service = OperationService::default();
        let awaiting = service
            .submit(OperationRequest::Copy {
                sources,
                destination_directory: panedeck_domain::NativePath::new(&destination),
            })
            .unwrap();
        service
            .resolve_conflict(ResolveConflictRequestDto {
                job_id: awaiting.job_id,
                decision: ConflictDecisionDto::KeepBoth,
                apply_to_remaining: true,
            })
            .unwrap();

        for _ in 0..200 {
            if ["one copy.txt", "two copy.txt", "three copy.txt"]
                .iter()
                .all(|name| destination.join(name).is_file())
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("all remaining conflicts should use the selected policy");
    }

    #[test]
    fn directory_merge_preserves_existing_children() {
        let temporary = TempDir::new().unwrap();
        let source_parent = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        let source_directory = source_parent.join("folder");
        let target_directory = destination.join("folder");
        fs::create_dir_all(&source_directory).unwrap();
        fs::create_dir_all(&target_directory).unwrap();
        fs::write(source_directory.join("incoming.txt"), b"incoming").unwrap();
        fs::write(target_directory.join("existing.txt"), b"existing").unwrap();
        let service = OperationService::default();
        let awaiting = service
            .submit(OperationRequest::Copy {
                sources: vec![panedeck_domain::NativePath::new(&source_directory)],
                destination_directory: panedeck_domain::NativePath::new(&destination),
            })
            .unwrap();
        assert_eq!(
            awaiting.conflict.as_ref().unwrap().kind,
            ConflictKindDto::DirectoryMerge
        );
        let queued = service
            .resolve_conflict(ResolveConflictRequestDto {
                job_id: awaiting.job_id,
                decision: ConflictDecisionDto::Replace,
                apply_to_remaining: false,
            })
            .unwrap();
        wait_for_terminal(&service, queued.job_id);
        assert_eq!(
            fs::read(target_directory.join("existing.txt")).unwrap(),
            b"existing"
        );
        assert_eq!(
            fs::read(target_directory.join("incoming.txt")).unwrap(),
            b"incoming"
        );
    }

    #[tokio::test]
    async fn rejects_drop_onto_self_descendant_and_read_only_directory() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source");
        let child = source.join("child");
        let read_only = temp.path().join("read-only");
        fs::create_dir_all(&child).expect("child");
        fs::create_dir(&read_only).expect("read only");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&read_only).expect("metadata").permissions();
            permissions.set_mode(0o555);
            fs::set_permissions(&read_only, permissions).expect("permissions");
        }
        let browser = BrowserService::default();
        let root = open(&browser, PaneIdDto::Left, temp.path()).await;
        let source_entry = root
            .entries
            .iter()
            .find(|entry| entry.display_name == "source")
            .expect("source entry");
        let read_only_entry = root
            .entries
            .iter()
            .find(|entry| entry.display_name == "read-only")
            .expect("read only entry");
        let source_listing = open(&browser, PaneIdDto::Right, &source).await;
        let child_entry = source_listing
            .entries
            .iter()
            .find(|entry| entry.display_name == "child")
            .expect("child entry");
        let service = OperationService::default();

        for destination in [
            source_entry.reference.clone(),
            child_entry.reference.clone(),
        ] {
            let result = service.transfer(
                &browser,
                TransferRequestDto {
                    kind: OperationKindDto::Move,
                    entries: vec![source_entry.reference.clone()],
                    destination: TransferDestinationDto::Entry { entry: destination },
                },
            );
            assert_eq!(
                result.expect_err("self or descendant must fail").code,
                OperationErrorCode::DirectoryIntoItself
            );
        }
        let read_only_result = service.transfer(
            &browser,
            TransferRequestDto {
                kind: OperationKindDto::Move,
                entries: vec![source_entry.reference.clone()],
                destination: TransferDestinationDto::Entry {
                    entry: read_only_entry.reference.clone(),
                },
            },
        );
        assert_eq!(
            read_only_result.expect_err("read only target").code,
            OperationErrorCode::PermissionDenied
        );
        assert!(source.is_dir());
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&read_only).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&read_only, permissions).expect("restore permissions");
        }
    }
}
