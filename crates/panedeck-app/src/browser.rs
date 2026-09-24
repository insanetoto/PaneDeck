use std::{
    collections::{HashMap, VecDeque},
    fmt,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use panedeck_domain::{
    AccessErrorKind, DirectorySession, DirectorySessionId, EntryKind, EntryReference, NativePath,
    SymlinkTarget,
};
use panedeck_fs::{
    DirectoryReadError, DirectoryReadEvent, DirectoryReadOptions, DirectoryReader, DirectoryWatch,
    DirectoryWatcher, SortDirection, SortField, SortSpec, WatchError, WatchSignal,
};
use serde::{Deserialize, Serialize};

use crate::{DirectoryReferenceDto, EntryReferenceDto, EntrySnapshotDto};

const MAX_STORED_SESSIONS: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneIdDto {
    Left,
    Right,
}

impl PaneIdDto {
    const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortFieldDto {
    Name,
    Size,
    ModifiedAt,
    Kind,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirectionDto {
    Ascending,
    Descending,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigateDirectoryRequestDto {
    pub pane: PaneIdDto,
    pub path: String,
    pub show_hidden: bool,
    pub sort_field: SortFieldDto,
    pub sort_direction: SortDirectionDto,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigateChildRequestDto {
    pub pane: PaneIdDto,
    pub entry: EntryReferenceDto,
    pub show_hidden: bool,
    pub sort_field: SortFieldDto,
    pub sort_direction: SortDirectionDto,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigateKnownRequestDto {
    pub pane: PaneIdDto,
    pub directory: DirectoryReferenceDto,
    pub show_hidden: bool,
    pub sort_field: SortFieldDto,
    pub sort_direction: SortDirectionDto,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchDirectoryRequestDto {
    pub pane: PaneIdDto,
    pub directory: DirectoryReferenceDto,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopDirectoryWatchRequestDto {
    pub pane: PaneIdDto,
    pub watch_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryWatchReasonDto {
    Changed,
    WatchFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryWatchEventDto {
    pub pane: PaneIdDto,
    pub watch_id: u64,
    pub reason: DirectoryWatchReasonDto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowseErrorCode {
    InvalidPath,
    NotFound,
    PermissionDenied,
    NotDirectory,
    StaleReference,
    Cancelled,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseErrorDto {
    pub code: BrowseErrorCode,
}

impl fmt::Display for BrowseErrorDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "directory navigation failed: {:?}", self.code)
    }
}

impl std::error::Error for BrowseErrorDto {}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListingDto {
    pub directory: DirectoryReferenceDto,
    pub display_path: String,
    pub can_go_up: bool,
    pub entries: Vec<EntrySnapshotDto>,
    pub warnings: Vec<BrowseErrorCode>,
}

#[derive(Default)]
struct SessionStore {
    sessions: HashMap<DirectorySessionId, DirectorySession>,
    order: VecDeque<DirectorySessionId>,
}

impl SessionStore {
    fn insert(&mut self, session: DirectorySession) {
        let id = session.reference().session_id();
        self.sessions.insert(id, session);
        self.order.push_back(id);
        while self.order.len() > MAX_STORED_SESSIONS {
            if let Some(expired) = self.order.pop_front() {
                self.sessions.remove(&expired);
            }
        }
    }
}

pub struct BrowserService {
    readers: [DirectoryReader; 2],
    sessions: Mutex<SessionStore>,
    watches: Mutex<[Option<ActiveWatch>; 2]>,
    next_watch_id: AtomicU64,
}

struct ActiveWatch {
    id: u64,
    _watch: DirectoryWatch,
}

impl Default for BrowserService {
    fn default() -> Self {
        Self {
            readers: [DirectoryReader::default(), DirectoryReader::default()],
            sessions: Mutex::new(SessionStore::default()),
            watches: Mutex::new([None, None]),
            next_watch_id: AtomicU64::new(1),
        }
    }
}

impl BrowserService {
    pub fn start_watch<F>(
        &self,
        request: WatchDirectoryRequestDto,
        on_event: F,
    ) -> Result<u64, BrowseErrorDto>
    where
        F: Fn(DirectoryWatchEventDto) + Send + 'static,
    {
        let root = self.known_root(request.directory)?;
        let watch_id = self.next_watch_id.fetch_add(1, Ordering::Relaxed);
        let pane = request.pane;
        let watch = DirectoryWatcher
            .start(root.as_path(), move |signal| {
                on_event(DirectoryWatchEventDto {
                    pane,
                    watch_id,
                    reason: match signal {
                        WatchSignal::RescanRequired => DirectoryWatchReasonDto::Changed,
                        WatchSignal::WatchFailed => DirectoryWatchReasonDto::WatchFailed,
                    },
                });
            })
            .map_err(map_watch_error)?;
        let replaced = self
            .watches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[pane.index()]
        .replace(ActiveWatch {
            id: watch_id,
            _watch: watch,
        });
        drop(replaced);
        Ok(watch_id)
    }

    pub fn stop_watch(&self, request: StopDirectoryWatchRequestDto) {
        let removed = {
            let mut watches = self
                .watches
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if watches[request.pane.index()]
                .as_ref()
                .is_some_and(|active| active.id == request.watch_id)
            {
                watches[request.pane.index()].take()
            } else {
                None
            }
        };
        drop(removed);
    }

    pub async fn open_path(
        &self,
        request: NavigateDirectoryRequestDto,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let path = PathBuf::from(request.path);
        if !path.is_absolute() {
            return Err(error(BrowseErrorCode::InvalidPath));
        }
        self.read(
            request.pane,
            NativePath::new(path),
            request.show_hidden,
            request.sort_field,
            request.sort_direction,
        )
        .await
    }

    pub async fn open_child(
        &self,
        request: NavigateChildRequestDto,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let reference = EntryReference::try_from(request.entry)
            .map_err(|_| error(BrowseErrorCode::StaleReference))?;
        let path = {
            let sessions = self
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let session = sessions
                .sessions
                .get(&reference.session_id())
                .ok_or_else(|| error(BrowseErrorCode::StaleReference))?;
            let entry = session
                .resolve(reference)
                .map_err(|_| error(BrowseErrorCode::StaleReference))?;
            let directory_like = entry.metadata().kind() == EntryKind::Directory
                || entry.metadata().symlink_target() == Some(SymlinkTarget::Directory);
            if !directory_like {
                return Err(error(BrowseErrorCode::NotDirectory));
            }
            entry.native_path().clone()
        };
        self.read(
            request.pane,
            path,
            request.show_hidden,
            request.sort_field,
            request.sort_direction,
        )
        .await
    }

    pub async fn open_parent(
        &self,
        request: NavigateKnownRequestDto,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let root = self.known_root(request.directory)?;
        let parent = root
            .as_path()
            .parent()
            .ok_or_else(|| error(BrowseErrorCode::NotDirectory))?;
        self.read(
            request.pane,
            NativePath::new(parent),
            request.show_hidden,
            request.sort_field,
            request.sort_direction,
        )
        .await
    }

    pub async fn reopen(
        &self,
        request: NavigateKnownRequestDto,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let root = self.known_root(request.directory)?;
        self.read(
            request.pane,
            root,
            request.show_hidden,
            request.sort_field,
            request.sort_direction,
        )
        .await
    }

    fn known_root(&self, reference: DirectoryReferenceDto) -> Result<NativePath, BrowseErrorDto> {
        let reference = panedeck_domain::DirectoryReference::try_from(reference)
            .map_err(|_| error(BrowseErrorCode::StaleReference))?;
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions
            .sessions
            .get(&reference.session_id())
            .map(|session| session.native_root().clone())
            .ok_or_else(|| error(BrowseErrorCode::StaleReference))
    }

    async fn read(
        &self,
        pane: PaneIdDto,
        root: NativePath,
        show_hidden: bool,
        sort_field: SortFieldDto,
        sort_direction: SortDirectionDto,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let display_path = root.as_path().to_string_lossy().into_owned();
        let can_go_up = root.as_path().parent().is_some();
        let options = DirectoryReadOptions {
            batch_size: NonZeroUsize::new(256).expect("batch size is non-zero"),
            show_hidden,
            sort: SortSpec {
                field: map_sort_field(sort_field),
                direction: map_sort_direction(sort_direction),
            },
        };
        let mut handle = self.readers[pane.index()].start(root, options);
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        while let Some(event) = handle.next_event().await {
            match event {
                DirectoryReadEvent::Batch(batch) => {
                    entries.extend(batch.entries.iter().map(EntrySnapshotDto::from_summary));
                }
                DirectoryReadEvent::EntryError { error, .. } => {
                    warnings.push(map_access_error(error.kind()));
                }
                DirectoryReadEvent::Completed { .. } => break,
            }
        }
        let session = handle.finish().await.map_err(map_read_error)?;
        let directory = session.reference().into();
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session);
        Ok(DirectoryListingDto {
            directory,
            display_path,
            can_go_up,
            entries,
            warnings,
        })
    }
}

const fn map_sort_field(value: SortFieldDto) -> SortField {
    match value {
        SortFieldDto::Name => SortField::Name,
        SortFieldDto::Size => SortField::Size,
        SortFieldDto::ModifiedAt => SortField::ModifiedAt,
        SortFieldDto::Kind => SortField::Kind,
    }
}

const fn map_sort_direction(value: SortDirectionDto) -> SortDirection {
    match value {
        SortDirectionDto::Ascending => SortDirection::Ascending,
        SortDirectionDto::Descending => SortDirection::Descending,
    }
}

fn map_read_error(value: DirectoryReadError) -> BrowseErrorDto {
    match value {
        DirectoryReadError::Access(access) => error(map_access_error(access.kind())),
        DirectoryReadError::Cancelled => error(BrowseErrorCode::Cancelled),
        DirectoryReadError::InvalidModel(_) => error(BrowseErrorCode::InvalidPath),
        DirectoryReadError::WorkerFailed => error(BrowseErrorCode::Io),
    }
}

const fn map_access_error(value: AccessErrorKind) -> BrowseErrorCode {
    match value {
        AccessErrorKind::NotFound => BrowseErrorCode::NotFound,
        AccessErrorKind::PermissionDenied => BrowseErrorCode::PermissionDenied,
        AccessErrorKind::NotDirectory => BrowseErrorCode::NotDirectory,
        AccessErrorKind::InvalidPath => BrowseErrorCode::InvalidPath,
        AccessErrorKind::UnsupportedFileType | AccessErrorKind::Io => BrowseErrorCode::Io,
    }
}

const fn error(code: BrowseErrorCode) -> BrowseErrorDto {
    BrowseErrorDto { code }
}

const fn map_watch_error(value: WatchError) -> BrowseErrorDto {
    error(match value {
        WatchError::NotFound => BrowseErrorCode::NotFound,
        WatchError::PermissionDenied => BrowseErrorCode::PermissionDenied,
        WatchError::NotDirectory => BrowseErrorCode::NotDirectory,
        WatchError::Io => BrowseErrorCode::Io,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn request(path: String) -> NavigateDirectoryRequestDto {
        NavigateDirectoryRequestDto {
            pane: PaneIdDto::Left,
            path,
            show_hidden: false,
            sort_field: SortFieldDto::Name,
            sort_direction: SortDirectionDto::Ascending,
        }
    }

    #[tokio::test]
    async fn opens_directory_child_parent_and_known_history_reference() {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("child")).expect("child");
        fs::write(temp.path().join("visible.txt"), b"data").expect("file");
        fs::write(temp.path().join(".hidden"), b"hidden").expect("hidden");
        let service = BrowserService::default();
        let root = service
            .open_path(request(temp.path().to_string_lossy().into_owned()))
            .await
            .expect("root listing");
        assert_eq!(root.entries.len(), 2);
        let child = root
            .entries
            .iter()
            .find(|entry| entry.display_name == "child")
            .expect("child entry");
        let child_listing = service
            .open_child(NavigateChildRequestDto {
                pane: PaneIdDto::Left,
                entry: child.reference.clone(),
                show_hidden: false,
                sort_field: SortFieldDto::Name,
                sort_direction: SortDirectionDto::Ascending,
            })
            .await
            .expect("child listing");
        let parent = service
            .open_parent(NavigateKnownRequestDto {
                pane: PaneIdDto::Left,
                directory: child_listing.directory,
                show_hidden: false,
                sort_field: SortFieldDto::Name,
                sort_direction: SortDirectionDto::Ascending,
            })
            .await
            .expect("parent listing");
        assert!(parent
            .entries
            .iter()
            .any(|entry| entry.display_name == "visible.txt"));
        let reopened = service
            .reopen(NavigateKnownRequestDto {
                pane: PaneIdDto::Left,
                directory: root.directory,
                show_hidden: true,
                sort_field: SortFieldDto::Name,
                sort_direction: SortDirectionDto::Ascending,
            })
            .await
            .expect("history listing");
        assert!(reopened
            .entries
            .iter()
            .any(|entry| entry.display_name == ".hidden"));
    }

    #[tokio::test]
    async fn rejects_relative_and_missing_paths_with_structured_errors() {
        let service = BrowserService::default();
        assert!(matches!(
            service.open_path(request("relative".to_owned())).await,
            Err(BrowseErrorDto {
                code: BrowseErrorCode::InvalidPath
            })
        ));
        assert!(matches!(
            service
                .open_path(request("/path/that/does/not/exist/panedeck".to_owned()))
                .await,
            Err(BrowseErrorDto {
                code: BrowseErrorCode::NotFound
            })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reports_permission_denied_without_exposing_the_path_in_the_error() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temp dir");
        let restricted = temp.path().join("restricted");
        fs::create_dir(&restricted).expect("restricted");
        let mut permissions = fs::metadata(&restricted).expect("metadata").permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&restricted, permissions).expect("restrict");
        let result = BrowserService::default()
            .open_path(request(restricted.to_string_lossy().into_owned()))
            .await;
        let mut restore = fs::metadata(&restricted).expect("metadata").permissions();
        restore.set_mode(0o700);
        fs::set_permissions(&restricted, restore).expect("restore");
        let error = result.expect_err("restricted directory must fail");
        assert_eq!(error.code, BrowseErrorCode::PermissionDenied);
        assert!(!error
            .to_string()
            .contains(temp.path().to_string_lossy().as_ref()));
    }
}
