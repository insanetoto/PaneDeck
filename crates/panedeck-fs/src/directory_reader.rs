use std::{
    cmp::Ordering,
    error::Error,
    ffi::OsString,
    fmt, fs, io,
    num::{NonZeroU64, NonZeroUsize},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
        Arc, Mutex,
    },
};

use panedeck_domain::{
    AccessErrorKind, AccessOperation, AccessSubject, DirectoryReference, DirectorySession,
    DirectorySessionId, EntryKind, EntryMetadata, EntryReference, FileAccessError, NativePath,
    SessionModelError, SymlinkTarget,
};
use tokio::{sync::mpsc, task::JoinHandle};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ReadRequestId(NonZeroU64);

impl ReadRequestId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortField {
    Name,
    Size,
    ModifiedAt,
    Kind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::Name,
            direction: SortDirection::Ascending,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectoryReadOptions {
    pub batch_size: NonZeroUsize,
    pub show_hidden: bool,
    pub sort: SortSpec,
}

impl Default for DirectoryReadOptions {
    fn default() -> Self {
        Self {
            batch_size: NonZeroUsize::new(256).expect("256 is non-zero"),
            show_hidden: false,
            sort: SortSpec::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntrySummary {
    pub reference: EntryReference,
    pub display_name: String,
    pub display_name_is_lossy: bool,
    pub metadata: EntryMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryBatch {
    pub request_id: ReadRequestId,
    pub entries: Vec<DirectoryEntrySummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectoryReadEvent {
    Batch(DirectoryBatch),
    EntryError {
        request_id: ReadRequestId,
        error: FileAccessError,
    },
    Completed {
        request_id: ReadRequestId,
        entry_count: usize,
    },
}

impl DirectoryReadEvent {
    #[must_use]
    pub const fn request_id(&self) -> ReadRequestId {
        match self {
            Self::Batch(batch) => batch.request_id,
            Self::EntryError { request_id, .. } | Self::Completed { request_id, .. } => *request_id,
        }
    }
}

#[derive(Debug)]
pub enum DirectoryReadError {
    Access(FileAccessError),
    InvalidModel(SessionModelError),
    Cancelled,
    WorkerFailed,
}

impl fmt::Display for DirectoryReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Access(error) => write!(formatter, "directory access failed: {error}"),
            Self::InvalidModel(error) => write!(formatter, "invalid directory model: {error}"),
            Self::Cancelled => formatter.write_str("directory read was cancelled"),
            Self::WorkerFailed => formatter.write_str("directory read worker failed"),
        }
    }
}

impl Error for DirectoryReadError {}

#[derive(Clone, Default)]
struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    fn cancel(&self) {
        self.0.store(true, AtomicOrdering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(AtomicOrdering::Acquire)
    }
}

/// One reader is intended for one pane. Starting a new read cancels the old
/// request, while request IDs let consumers discard already-buffered events.
#[derive(Default)]
pub struct DirectoryReader {
    active: Mutex<Option<CancellationToken>>,
}

impl DirectoryReader {
    #[must_use]
    pub fn start(&self, root: NativePath, options: DirectoryReadOptions) -> DirectoryReadHandle {
        let request_id = next_request_id();
        let session_id = next_session_id();
        let cancellation = CancellationToken::default();

        {
            let mut active = self
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(previous) = active.replace(cancellation.clone()) {
                previous.cancel();
            }
        }

        let (sender, events) = mpsc::unbounded_channel();
        let worker_cancellation = cancellation.clone();
        let task = tokio::task::spawn_blocking(move || {
            read_directory_blocking(
                request_id,
                session_id,
                root,
                options,
                worker_cancellation,
                sender,
            )
        });

        DirectoryReadHandle {
            request_id,
            cancellation,
            events,
            task: Some(task),
        }
    }
}

pub struct DirectoryReadHandle {
    request_id: ReadRequestId,
    cancellation: CancellationToken,
    events: mpsc::UnboundedReceiver<DirectoryReadEvent>,
    task: Option<JoinHandle<Result<DirectorySession, DirectoryReadError>>>,
}

impl DirectoryReadHandle {
    #[must_use]
    pub const fn request_id(&self) -> ReadRequestId {
        self.request_id
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub async fn next_event(&mut self) -> Option<DirectoryReadEvent> {
        self.events.recv().await
    }

    pub async fn finish(mut self) -> Result<DirectorySession, DirectoryReadError> {
        self.task
            .take()
            .expect("directory read task exists until finish")
            .await
            .map_err(|_| DirectoryReadError::WorkerFailed)?
    }
}

impl Drop for DirectoryReadHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct PendingEntry {
    path: NativePath,
    sort_name: OsString,
    metadata: EntryMetadata,
    access_error: Option<AccessErrorKind>,
}

fn read_directory_blocking(
    request_id: ReadRequestId,
    session_id: DirectorySessionId,
    root: NativePath,
    options: DirectoryReadOptions,
    cancellation: CancellationToken,
    sender: mpsc::UnboundedSender<DirectoryReadEvent>,
) -> Result<DirectorySession, DirectoryReadError> {
    ensure_not_cancelled(&cancellation)?;
    let directory_reference = DirectoryReference::new(session_id);
    let read_dir = fs::read_dir(root.as_path()).map_err(|error| {
        DirectoryReadError::Access(access_error(
            AccessOperation::OpenDirectory,
            map_io_error_kind(error.kind()),
            AccessSubject::Directory(directory_reference),
        ))
    })?;

    let mut pending = Vec::new();
    let mut enumeration_errors = Vec::new();
    for result in read_dir {
        ensure_not_cancelled(&cancellation)?;
        match result {
            Ok(entry) => {
                let file_name = entry.file_name();
                if !options.show_hidden && is_hidden_name(&file_name) {
                    continue;
                }

                if let Some(entry) = read_pending_entry(entry.path(), file_name)? {
                    pending.push(entry);
                }
            }
            Err(error) => enumeration_errors.push(access_error(
                AccessOperation::ReadEntry,
                map_io_error_kind(error.kind()),
                AccessSubject::Directory(directory_reference),
            )),
        }
    }

    pending.sort_by(|left, right| compare_pending(left, right, options.sort));
    let mut session =
        DirectorySession::new(session_id, root).map_err(DirectoryReadError::InvalidModel)?;

    for error in enumeration_errors {
        let _ = sender.send(DirectoryReadEvent::EntryError { request_id, error });
    }

    let mut batch = Vec::with_capacity(options.batch_size.get());
    for pending_entry in pending {
        ensure_not_cancelled(&cancellation)?;
        let reference = session
            .register_entry(pending_entry.path, pending_entry.metadata.clone())
            .map_err(DirectoryReadError::InvalidModel)?;
        let entry = session
            .resolve(reference)
            .map_err(|_| DirectoryReadError::WorkerFailed)?;

        if let Some(kind) = pending_entry.access_error {
            let _ = sender.send(DirectoryReadEvent::EntryError {
                request_id,
                error: access_error(
                    AccessOperation::ReadMetadata,
                    kind,
                    AccessSubject::Entry(reference),
                ),
            });
        }

        batch.push(DirectoryEntrySummary {
            reference,
            display_name: entry.display_name().as_str().to_owned(),
            display_name_is_lossy: entry.display_name().is_lossy(),
            metadata: entry.metadata().clone(),
        });

        if batch.len() == options.batch_size.get() {
            let _ = sender.send(DirectoryReadEvent::Batch(DirectoryBatch {
                request_id,
                entries: std::mem::take(&mut batch),
            }));
            batch = Vec::with_capacity(options.batch_size.get());
        }
    }

    if !batch.is_empty() {
        let _ = sender.send(DirectoryReadEvent::Batch(DirectoryBatch {
            request_id,
            entries: batch,
        }));
    }

    ensure_not_cancelled(&cancellation)?;
    let _ = sender.send(DirectoryReadEvent::Completed {
        request_id,
        entry_count: session.len(),
    });
    Ok(session)
}

fn read_pending_entry(
    path: PathBuf,
    file_name: OsString,
) -> Result<Option<PendingEntry>, DirectoryReadError> {
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            let entry_metadata = metadata_from(
                &metadata,
                symlink_target(&path, &metadata),
                is_hidden_name(&file_name),
            );
            Ok(Some(PendingEntry {
                path: NativePath::new(path),
                sort_name: file_name,
                metadata: entry_metadata,
                access_error: None,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Ok(Some(PendingEntry {
            path: NativePath::new(path),
            sort_name: file_name,
            metadata: EntryMetadata::new(EntryKind::Other),
            access_error: Some(map_io_error_kind(error.kind())),
        })),
    }
}

fn metadata_from(
    metadata: &fs::Metadata,
    symlink_target: Option<SymlinkTarget>,
    is_hidden: bool,
) -> EntryMetadata {
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };

    let mut result = EntryMetadata::new(kind)
        .with_byte_len(metadata.len())
        .with_hidden(is_hidden)
        .with_read_only(metadata.permissions().readonly());
    if let Ok(modified_at) = metadata.modified() {
        result = result.with_modified_at(modified_at);
    }
    if let Some(target) = symlink_target {
        result = result.with_symlink_target(target);
    }
    result
}

fn symlink_target(path: &Path, metadata: &fs::Metadata) -> Option<SymlinkTarget> {
    if !metadata.file_type().is_symlink() {
        return None;
    }

    Some(match fs::metadata(path) {
        Ok(target) if target.is_file() => SymlinkTarget::File,
        Ok(target) if target.is_dir() => SymlinkTarget::Directory,
        Ok(_) => SymlinkTarget::Other,
        Err(error) if error.kind() == io::ErrorKind::NotFound => SymlinkTarget::Missing,
        Err(_) => SymlinkTarget::Unknown,
    })
}

fn compare_pending(left: &PendingEntry, right: &PendingEntry, sort: SortSpec) -> Ordering {
    let primary = match sort.field {
        SortField::Name => left.sort_name.cmp(&right.sort_name),
        SortField::Size => left.metadata.byte_len().cmp(&right.metadata.byte_len()),
        SortField::ModifiedAt => left
            .metadata
            .modified_at()
            .cmp(&right.metadata.modified_at()),
        SortField::Kind => kind_rank(left.metadata.kind()).cmp(&kind_rank(right.metadata.kind())),
    };
    let ordering = primary.then_with(|| left.sort_name.cmp(&right.sort_name));
    match sort.direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

const fn kind_rank(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::Directory => 0,
        EntryKind::File => 1,
        EntryKind::Symlink => 2,
        EntryKind::Other => 3,
    }
}

fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), DirectoryReadError> {
    if cancellation.is_cancelled() {
        Err(DirectoryReadError::Cancelled)
    } else {
        Ok(())
    }
}

const fn map_io_error_kind(kind: io::ErrorKind) -> AccessErrorKind {
    match kind {
        io::ErrorKind::NotFound => AccessErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => AccessErrorKind::PermissionDenied,
        io::ErrorKind::NotADirectory => AccessErrorKind::NotDirectory,
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => AccessErrorKind::InvalidPath,
        _ => AccessErrorKind::Io,
    }
}

const fn access_error(
    operation: AccessOperation,
    kind: AccessErrorKind,
    subject: AccessSubject,
) -> FileAccessError {
    FileAccessError::new(operation, kind, subject)
}

fn next_request_id() -> ReadRequestId {
    next_non_zero(&NEXT_REQUEST_ID).map_or_else(|| ReadRequestId(NonZeroU64::MAX), ReadRequestId)
}

fn next_session_id() -> DirectorySessionId {
    next_non_zero(&NEXT_SESSION_ID)
        .and_then(|value| DirectorySessionId::from_raw(value.get()))
        .unwrap_or_else(|| DirectorySessionId::from_raw(u64::MAX).expect("u64::MAX is non-zero"))
}

fn next_non_zero(counter: &AtomicU64) -> Option<NonZeroU64> {
    let value = counter.fetch_add(1, AtomicOrdering::Relaxed);
    NonZeroU64::new(value)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write, num::NonZeroUsize};

    use tempfile::TempDir;

    use super::*;

    async fn collect(
        mut handle: DirectoryReadHandle,
    ) -> (
        Vec<DirectoryReadEvent>,
        Result<DirectorySession, DirectoryReadError>,
    ) {
        let mut events = Vec::new();
        while let Some(event) = handle.next_event().await {
            events.push(event);
        }
        let result = handle.finish().await;
        (events, result)
    }

    fn create_files(directory: &Path, count: usize) {
        for index in 0..count {
            File::create(directory.join(format!("entry-{index:05}"))).unwrap();
        }
    }

    fn batch_size(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("test batch sizes are non-zero")
    }

    #[tokio::test]
    async fn reads_empty_directory() {
        let temp = TempDir::new().unwrap();
        let reader = DirectoryReader::default();
        let handle = reader.start(
            NativePath::new(temp.path()),
            DirectoryReadOptions::default(),
        );
        let (events, result) = collect(handle).await;

        assert!(result.unwrap().is_empty());
        assert!(matches!(
            events.as_slice(),
            [DirectoryReadEvent::Completed { entry_count: 0, .. }]
        ));
    }

    #[tokio::test]
    async fn reads_one_thousand_entries_in_batches() {
        assert_batch_read(1_000, 128).await;
    }

    #[tokio::test]
    async fn reads_ten_thousand_entries_in_batches() {
        assert_batch_read(10_000, 333).await;
    }

    async fn assert_batch_read(entry_count: usize, configured_batch_size: usize) {
        let temp = TempDir::new().unwrap();
        create_files(temp.path(), entry_count);
        let reader = DirectoryReader::default();
        let handle = reader.start(
            NativePath::new(temp.path()),
            DirectoryReadOptions {
                batch_size: batch_size(configured_batch_size),
                show_hidden: true,
                sort: SortSpec::default(),
            },
        );
        let request_id = handle.request_id();
        let (events, result) = collect(handle).await;
        let session = result.unwrap();
        let batches: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DirectoryReadEvent::Batch(batch) => Some(batch),
                _ => None,
            })
            .collect();

        assert_eq!(session.len(), entry_count);
        assert!(batches.len() > 1);
        assert!(batches
            .iter()
            .all(|batch| batch.request_id == request_id
                && batch.entries.len() <= configured_batch_size));
        assert_eq!(
            batches
                .iter()
                .map(|batch| batch.entries.len())
                .sum::<usize>(),
            entry_count
        );
    }

    #[tokio::test]
    async fn filters_hidden_entries_and_sorts_names() {
        let temp = TempDir::new().unwrap();
        File::create(temp.path().join("zeta")).unwrap();
        File::create(temp.path().join("alpha")).unwrap();
        File::create(temp.path().join(".hidden")).unwrap();
        let reader = DirectoryReader::default();
        let handle = reader.start(
            NativePath::new(temp.path()),
            DirectoryReadOptions {
                batch_size: batch_size(10),
                show_hidden: false,
                sort: SortSpec::default(),
            },
        );
        let (events, result) = collect(handle).await;
        assert_eq!(result.unwrap().len(), 2);
        let names: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DirectoryReadEvent::Batch(batch) => Some(batch.entries.as_slice()),
                _ => None,
            })
            .flatten()
            .map(|entry| entry.display_name.as_str())
            .collect();
        assert_eq!(names, ["alpha", "zeta"]);
    }

    #[tokio::test]
    async fn sorts_by_size_in_descending_order() {
        let temp = TempDir::new().unwrap();
        File::create(temp.path().join("small"))
            .unwrap()
            .write_all(b"1")
            .unwrap();
        File::create(temp.path().join("large"))
            .unwrap()
            .write_all(b"12345")
            .unwrap();
        let reader = DirectoryReader::default();
        let handle = reader.start(
            NativePath::new(temp.path()),
            DirectoryReadOptions {
                batch_size: batch_size(10),
                show_hidden: true,
                sort: SortSpec {
                    field: SortField::Size,
                    direction: SortDirection::Descending,
                },
            },
        );
        let (events, result) = collect(handle).await;
        assert_eq!(result.unwrap().len(), 2);
        let names: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                DirectoryReadEvent::Batch(batch) => Some(batch.entries.as_slice()),
                _ => None,
            })
            .flatten()
            .map(|entry| entry.display_name.as_str())
            .collect();
        assert_eq!(names, ["large", "small"]);
    }

    #[tokio::test]
    async fn replacing_a_request_cancels_the_old_one_and_tags_all_events() {
        let first_directory = TempDir::new().unwrap();
        create_files(first_directory.path(), 1_000);
        let second_directory = TempDir::new().unwrap();
        let reader = DirectoryReader::default();
        let first = reader.start(
            NativePath::new(first_directory.path()),
            DirectoryReadOptions::default(),
        );
        let first_id = first.request_id();
        let second = reader.start(
            NativePath::new(second_directory.path()),
            DirectoryReadOptions::default(),
        );
        let second_id = second.request_id();

        assert_ne!(first_id, second_id);
        assert!(first.is_cancelled());
        let (events, result) = collect(second).await;
        assert!(result.is_ok());
        assert!(events.iter().all(|event| event.request_id() == second_id));

        let (_, first_result) = collect(first).await;
        assert!(matches!(first_result, Err(DirectoryReadError::Cancelled)));
    }

    #[test]
    fn maps_permission_denied_to_a_structured_error_kind() {
        assert_eq!(
            map_io_error_kind(io::ErrorKind::PermissionDenied),
            AccessErrorKind::PermissionDenied
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn opening_a_permission_denied_directory_returns_structured_error() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let blocked = temp.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        let reader = DirectoryReader::default();
        let handle = reader.start(NativePath::new(&blocked), DirectoryReadOptions::default());
        let (_, result) = collect(handle).await;
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();

        match result {
            Err(DirectoryReadError::Access(error)) => {
                assert_eq!(error.kind(), AccessErrorKind::PermissionDenied);
            }
            _ => panic!("expected a structured permission-denied error"),
        }
    }

    #[test]
    fn entry_disappearing_before_metadata_is_skipped() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("vanishing");
        File::create(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(read_pending_entry(path, OsString::from("vanishing"))
            .unwrap()
            .is_none());
    }
}
