use std::{
    collections::{hash_map::RandomState, HashMap, VecDeque},
    fmt,
    hash::BuildHasher,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
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
pub struct HoldTreeNodeRequestDto {
    pub pane: PaneIdDto,
    pub node_id: String,
    pub directory: Option<DirectoryReferenceDto>,
    pub entry: Option<EntryReferenceDto>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeNodeRequestDto {
    pub pane: PaneIdDto,
    pub node_id: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadTreeNodeRequestDto {
    pub pane: PaneIdDto,
    pub node_id: String,
    pub show_hidden: bool,
    pub sort_field: SortFieldDto,
    pub sort_direction: SortDirectionDto,
}
struct TreeNode {
    root: NativePath,
    session: DirectorySessionId,
    reader: Arc<DirectoryReader>,
}
const MAX_TREE_NODES_PER_PANE: usize = 1024;

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
    ResourceLimit,
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
    pub tree_key: String,
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
    held: HashMap<DirectorySessionId, usize>,
}

impl SessionStore {
    fn insert(&mut self, session: DirectorySession) {
        let id = session.reference().session_id();
        self.sessions.insert(id, session);
        self.order.push_back(id);
        self.evict();
    }
    fn pin(&mut self, id: DirectorySessionId) {
        *self.held.entry(id).or_default() += 1;
    }
    fn unpin(&mut self, id: DirectorySessionId) {
        if let Some(count) = self.held.get_mut(&id) {
            *count -= 1;
            if *count == 0 {
                self.held.remove(&id);
            }
        }
        self.evict();
    }
    fn evict(&mut self) {
        let mut remaining = self
            .order
            .iter()
            .filter(|id| !self.held.contains_key(id))
            .count();
        self.order.retain(|id| {
            if remaining > MAX_STORED_SESSIONS && !self.held.contains_key(id) {
                self.sessions.remove(id);
                remaining -= 1;
                false
            } else {
                true
            }
        });
    }
}

pub struct BrowserService {
    readers: [DirectoryReader; 2],
    sessions: Mutex<SessionStore>,
    watches: Mutex<HashMap<(PaneIdDto, u64), ActiveWatch>>,
    next_watch_id: AtomicU64,
    tree_nodes: Mutex<HashMap<(PaneIdDto, String), TreeNode>>,
    entry_keys: RandomState,
}

struct ActiveWatch {
    _watch: DirectoryWatch,
}

impl Default for BrowserService {
    fn default() -> Self {
        Self {
            readers: [DirectoryReader::default(), DirectoryReader::default()],
            sessions: Mutex::new(SessionStore::default()),
            watches: Mutex::new(HashMap::new()),
            next_watch_id: AtomicU64::new(1),
            tree_nodes: Mutex::new(HashMap::new()),
            entry_keys: RandomState::new(),
        }
    }
}

impl BrowserService {
    pub fn hold_tree_node(&self, request: HoldTreeNodeRequestDto) -> Result<(), BrowseErrorDto> {
        if request.node_id.is_empty() || request.node_id.len() > 128 {
            return Err(error(BrowseErrorCode::InvalidPath));
        }
        let mut nodes = self
            .tree_nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A root rescan pins its replacement before releasing the previous root.
        // Reserve one bounded handover slot so a full tree never loses its root.
        let budget = MAX_TREE_NODES_PER_PANE
            + usize::from(request.directory.is_some() && request.entry.is_none());
        if !nodes.contains_key(&(request.pane, request.node_id.clone()))
            && nodes
                .keys()
                .filter(|(pane, _)| *pane == request.pane)
                .count()
                >= budget
        {
            return Err(error(BrowseErrorCode::ResourceLimit));
        }
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, id) = match (request.directory, request.entry) {
            (Some(directory), None) => {
                let reference = panedeck_domain::DirectoryReference::try_from(directory)
                    .map_err(|_| error(BrowseErrorCode::StaleReference))?;
                let id = reference.session_id();
                (
                    sessions
                        .sessions
                        .get(&id)
                        .ok_or_else(|| error(BrowseErrorCode::StaleReference))?
                        .native_root()
                        .clone(),
                    id,
                )
            }
            (None, Some(entry)) => {
                let reference = EntryReference::try_from(entry)
                    .map_err(|_| error(BrowseErrorCode::StaleReference))?;
                let entry = sessions
                    .sessions
                    .get(&reference.session_id())
                    .ok_or_else(|| error(BrowseErrorCode::StaleReference))?
                    .resolve(reference)
                    .map_err(|_| error(BrowseErrorCode::StaleReference))?;
                if entry.metadata().kind() != EntryKind::Directory {
                    return Err(error(BrowseErrorCode::NotDirectory));
                }
                (entry.native_path().clone(), reference.session_id())
            }
            _ => return Err(error(BrowseErrorCode::InvalidPath)),
        };
        sessions.pin(id);
        if let Some(old) = nodes.insert(
            (request.pane, request.node_id),
            TreeNode {
                root,
                session: id,
                reader: Arc::new(DirectoryReader::default()),
            },
        ) {
            old.reader.cancel();
            sessions.unpin(old.session);
        }
        Ok(())
    }
    pub fn release_tree_node(&self, request: TreeNodeRequestDto) {
        let mut nodes = self
            .tree_nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(node) = nodes.remove(&(request.pane, request.node_id)) {
            node.reader.cancel();
            self.sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .unpin(node.session);
        }
    }
    pub async fn read_tree_node(
        &self,
        request: ReadTreeNodeRequestDto,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let key = (request.pane, request.node_id);
        let (root, reader) = {
            let nodes = self
                .tree_nodes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let node = nodes
                .get(&key)
                .ok_or_else(|| error(BrowseErrorCode::Cancelled))?;
            (node.root.clone(), Arc::clone(&node.reader))
        };
        self.read_with_reader(
            &reader,
            root,
            request.show_hidden,
            request.sort_field,
            request.sort_direction,
            Some((&key, &reader)),
        )
        .await
    }

    pub fn start_watch<F>(
        &self,
        request: WatchDirectoryRequestDto,
        on_event: F,
    ) -> Result<u64, BrowseErrorDto>
    where
        F: Fn(DirectoryWatchEventDto) + Send + 'static,
    {
        // Reserve under one lock so concurrent starts cannot exceed the budget.
        let mut watches = self
            .watches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if watches
            .keys()
            .filter(|(pane, _)| *pane == request.pane)
            .count()
            >= 128
        {
            return Err(error(BrowseErrorCode::ResourceLimit));
        }
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
        watches.insert((pane, watch_id), ActiveWatch { _watch: watch });
        Ok(watch_id)
    }

    pub fn stop_watch(&self, request: StopDirectoryWatchRequestDto) {
        let removed = self
            .watches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(request.pane, request.watch_id));
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

    pub fn resolve_directory(
        &self,
        reference: DirectoryReferenceDto,
    ) -> Result<NativePath, BrowseErrorDto> {
        self.known_root(reference)
    }

    pub fn resolve_entries(
        &self,
        references: Vec<EntryReferenceDto>,
    ) -> Result<Vec<NativePath>, BrowseErrorDto> {
        let references = references
            .into_iter()
            .map(EntryReference::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| error(BrowseErrorCode::StaleReference))?;
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        references
            .into_iter()
            .map(|reference| {
                sessions
                    .sessions
                    .get(&reference.session_id())
                    .ok_or_else(|| error(BrowseErrorCode::StaleReference))?
                    .resolve(reference)
                    .map(|entry| entry.native_path().clone())
                    .map_err(|_| error(BrowseErrorCode::StaleReference))
            })
            .collect()
    }

    /// Validate every capability before removing duplicated parents/descendants.
    pub fn normalize_selection(
        &self,
        references: Vec<EntryReferenceDto>,
    ) -> Result<Vec<EntryReferenceDto>, BrowseErrorDto> {
        let paths = self.resolve_entries(references.clone())?;
        let directories: Vec<bool> = paths
            .iter()
            .map(|path| std::fs::symlink_metadata(path.as_path()).is_ok_and(|meta| meta.is_dir()))
            .collect();
        Ok(references
            .into_iter()
            .enumerate()
            .filter_map(|(i, reference)| {
                let covered = paths.iter().enumerate().any(|(j, other)| {
                    j != i
                        && ((other == &paths[i] && j < i)
                            || (directories[j]
                                && other != &paths[i]
                                && paths[i].as_path().starts_with(other.as_path())))
                });
                (!covered).then_some(reference)
            })
            .collect())
    }
    pub fn resolve_selection(
        &self,
        references: Vec<EntryReferenceDto>,
    ) -> Result<Vec<NativePath>, BrowseErrorDto> {
        self.resolve_entries(self.normalize_selection(references)?)
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
        self.read_with_reader(
            &self.readers[pane.index()],
            root,
            show_hidden,
            sort_field,
            sort_direction,
            None,
        )
        .await
    }
    async fn read_with_reader(
        &self,
        reader: &DirectoryReader,
        root: NativePath,
        show_hidden: bool,
        sort_field: SortFieldDto,
        sort_direction: SortDirectionDto,
        owner: Option<(&(PaneIdDto, String), &Arc<DirectoryReader>)>,
    ) -> Result<DirectoryListingDto, BrowseErrorDto> {
        let tree_key = format!("{:016x}", self.entry_keys.hash_one(&root));
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
        let mut handle = reader.start(root, options);
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
        for entry in &mut entries {
            if let Ok(reference) = EntryReference::try_from(entry.reference.clone()) {
                if let Ok(native) = session.resolve(reference) {
                    entry.tree_key = Some(format!(
                        "{:016x}",
                        self.entry_keys.hash_one(native.native_path())
                    ));
                }
            }
        }
        let mut nodes = self
            .tree_nodes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((key, reader)) = owner {
            let node = nodes
                .get_mut(key)
                .filter(|node| Arc::ptr_eq(&node.reader, reader))
                .ok_or_else(|| error(BrowseErrorCode::Cancelled))?;
            let old = node.session;
            node.session = session.reference().session_id();
            sessions.pin(node.session);
            sessions.insert(session);
            sessions.unpin(old);
        } else {
            sessions.insert(session);
        }
        Ok(DirectoryListingDto {
            tree_key,
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

    fn node_request(id: &str) -> ReadTreeNodeRequestDto {
        ReadTreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: id.into(),
            show_hidden: false,
            sort_field: SortFieldDto::Name,
            sort_direction: SortDirectionDto::Ascending,
        }
    }
    #[tokio::test]
    async fn repeated_tree_navigation_releases_native_resources_and_bounds_history() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("folder")).unwrap();
        fs::write(temp.path().join("folder/file"), b"owned audit fixture").unwrap();
        let service = BrowserService::default();
        for cycle in 0..200 {
            let root = service
                .open_path(request(temp.path().to_string_lossy().into_owned()))
                .await
                .unwrap();
            service
                .hold_tree_node(HoldTreeNodeRequestDto {
                    pane: PaneIdDto::Left,
                    node_id: "root-audit".into(),
                    directory: Some(root.directory.clone()),
                    entry: None,
                })
                .unwrap();
            service
                .hold_tree_node(HoldTreeNodeRequestDto {
                    pane: PaneIdDto::Left,
                    node_id: "branch-audit".into(),
                    directory: None,
                    entry: Some(root.entries[0].reference.clone()),
                })
                .unwrap();
            let children = service
                .read_tree_node(node_request("branch-audit"))
                .await
                .unwrap();
            assert_eq!(children.entries.len(), 1);
            if cycle < 12 {
                let id = service
                    .start_watch(
                        WatchDirectoryRequestDto {
                            pane: PaneIdDto::Left,
                            directory: children.directory,
                        },
                        |_| {},
                    )
                    .unwrap();
                service.stop_watch(StopDirectoryWatchRequestDto {
                    pane: PaneIdDto::Left,
                    watch_id: id,
                });
            }
            for id in ["branch-audit", "root-audit"] {
                service.release_tree_node(TreeNodeRequestDto {
                    pane: PaneIdDto::Left,
                    node_id: id.into(),
                });
            }
            assert!(service.tree_nodes.lock().unwrap().is_empty());
            assert!(service.watches.lock().unwrap().is_empty());
            let store = service.sessions.lock().unwrap();
            assert!(store.held.is_empty());
            assert!(store.sessions.len() <= MAX_STORED_SESSIONS);
        }
    }

    #[tokio::test]
    async fn full_tree_budget_keeps_a_slot_for_root_handover() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("child")).unwrap();
        let service = BrowserService::default();
        let root = service
            .open_path(request(temp.path().to_string_lossy().into_owned()))
            .await
            .unwrap();
        service
            .hold_tree_node(HoldTreeNodeRequestDto {
                pane: PaneIdDto::Left,
                node_id: "old-root".into(),
                directory: Some(root.directory.clone()),
                entry: None,
            })
            .unwrap();
        for i in 0..MAX_TREE_NODES_PER_PANE - 1 {
            service
                .hold_tree_node(HoldTreeNodeRequestDto {
                    pane: PaneIdDto::Left,
                    node_id: i.to_string(),
                    directory: None,
                    entry: Some(root.entries[0].reference.clone()),
                })
                .unwrap();
        }
        assert_eq!(
            service
                .hold_tree_node(HoldTreeNodeRequestDto {
                    pane: PaneIdDto::Left,
                    node_id: "over-budget".into(),
                    directory: None,
                    entry: Some(root.entries[0].reference.clone())
                })
                .unwrap_err()
                .code,
            BrowseErrorCode::ResourceLimit
        );
        service
            .hold_tree_node(HoldTreeNodeRequestDto {
                pane: PaneIdDto::Left,
                node_id: "new-root".into(),
                directory: Some(root.directory.clone()),
                entry: None,
            })
            .unwrap();
        service.release_tree_node(TreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: "old-root".into(),
        });
        assert!(service.resolve_directory(root.directory).is_ok());
        let keys: Vec<_> = service.tree_nodes.lock().unwrap().keys().cloned().collect();
        for (pane, node_id) in keys {
            service.release_tree_node(TreeNodeRequestDto { pane, node_id });
        }
        assert!(service.sessions.lock().unwrap().held.is_empty());
    }

    #[tokio::test]
    async fn independent_watches_rescan_external_changes_in_the_correct_directory() {
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };
        let temp = TempDir::new().unwrap();
        let branch = temp.path().join("branch");
        fs::create_dir(&branch).unwrap();
        fs::write(temp.path().join("sibling"), b"kept").unwrap();
        let service = BrowserService::default();
        let root = service
            .open_path(request(temp.path().to_string_lossy().into_owned()))
            .await
            .unwrap();
        service
            .hold_tree_node(HoldTreeNodeRequestDto {
                pane: PaneIdDto::Left,
                node_id: "branch".into(),
                directory: None,
                entry: Some(
                    root.entries
                        .iter()
                        .find(|entry| entry.display_name == "branch")
                        .unwrap()
                        .reference
                        .clone(),
                ),
            })
            .unwrap();
        let listing = service
            .read_tree_node(node_request("branch"))
            .await
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let branch_sender = sender.clone();
        let root_id = service
            .start_watch(
                WatchDirectoryRequestDto {
                    pane: PaneIdDto::Left,
                    directory: root.directory.clone(),
                },
                move |event| {
                    let _ = sender.send(event);
                },
            )
            .unwrap();
        let branch_id = service
            .start_watch(
                WatchDirectoryRequestDto {
                    pane: PaneIdDto::Left,
                    directory: listing.directory,
                },
                move |event| {
                    let _ = branch_sender.send(event);
                },
            )
            .unwrap();
        assert_ne!(root_id, branch_id);
        assert_eq!(service.watches.lock().unwrap().len(), 2);
        // FSEvents startup is asynchronous: use owned fixture mutations as readiness probes.
        for (id, path) in [(root_id, temp.path()), (branch_id, branch.as_path())] {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut index = 0;
            loop {
                fs::write(path.join(format!("ready-{index}")), b"probe").unwrap();
                index += 1;
                if receiver
                    .recv_timeout(Duration::from_millis(250))
                    .is_ok_and(|event| event.watch_id == id)
                {
                    break;
                }
                assert!(Instant::now() < deadline, "watcher did not become ready");
            }
        }
        let wait_branch = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if receiver
                    .recv_timeout(Duration::from_millis(250))
                    .is_ok_and(|event| event.watch_id == branch_id)
                {
                    break;
                }
                assert!(Instant::now() < deadline, "branch change not observed");
            }
        };
        fs::write(branch.join("created"), b"data").unwrap();
        wait_branch();
        let created = service
            .read_tree_node(node_request("branch"))
            .await
            .unwrap();
        assert!(created
            .entries
            .iter()
            .any(|entry| entry.display_name == "created"));
        fs::rename(branch.join("created"), branch.join("renamed")).unwrap();
        wait_branch();
        let renamed = service
            .read_tree_node(node_request("branch"))
            .await
            .unwrap();
        assert!(renamed
            .entries
            .iter()
            .any(|entry| entry.display_name == "renamed"));
        assert!(!renamed
            .entries
            .iter()
            .any(|entry| entry.display_name == "created"));
        fs::remove_file(branch.join("renamed")).unwrap();
        wait_branch();
        assert!(!service
            .read_tree_node(node_request("branch"))
            .await
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.display_name == "renamed"));
        service.stop_watch(StopDirectoryWatchRequestDto {
            pane: PaneIdDto::Left,
            watch_id: branch_id,
        });
        assert_eq!(service.watches.lock().unwrap().len(), 1);
        fs::remove_dir_all(&branch).unwrap();
        assert_eq!(
            service
                .read_tree_node(node_request("branch"))
                .await
                .unwrap_err()
                .code,
            BrowseErrorCode::NotFound
        );
        let refreshed = service
            .reopen(NavigateKnownRequestDto {
                pane: PaneIdDto::Left,
                directory: root.directory,
                show_hidden: false,
                sort_field: SortFieldDto::Name,
                sort_direction: SortDirectionDto::Ascending,
            })
            .await
            .unwrap();
        assert!(refreshed
            .entries
            .iter()
            .any(|entry| entry.display_name == "sibling"));
        service.stop_watch(StopDirectoryWatchRequestDto {
            pane: PaneIdDto::Left,
            watch_id: root_id,
        });
        service.release_tree_node(TreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: "branch".into(),
        });
        assert!(service.watches.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn normalizes_parent_and_nested_selection_after_validating_every_reference() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("parent")).unwrap();
        fs::write(temp.path().join("parent/file"), b"data").unwrap();
        let service = BrowserService::default();
        let root = service
            .open_path(request(temp.path().to_string_lossy().into_owned()))
            .await
            .unwrap();
        let parent = root.entries[0].reference.clone();
        let children = service
            .open_child(NavigateChildRequestDto {
                pane: PaneIdDto::Left,
                entry: parent.clone(),
                show_hidden: false,
                sort_field: SortFieldDto::Name,
                sort_direction: SortDirectionDto::Ascending,
            })
            .await
            .unwrap();
        let child = children.entries[0].reference.clone();
        assert_eq!(
            service
                .normalize_selection(vec![child.clone(), parent.clone(), child])
                .unwrap(),
            vec![parent.clone()]
        );
        assert!(service
            .normalize_selection(vec![
                parent,
                EntryReferenceDto {
                    session_id: "999999999".into(),
                    entry_id: "1".into()
                }
            ])
            .is_err());
    }

    #[tokio::test]
    async fn tree_reads_are_independent_and_held_sessions_survive_cache_eviction() {
        let temp = TempDir::new().unwrap();
        for i in 0..140 {
            fs::create_dir(temp.path().join(format!("dir{i}"))).unwrap();
            fs::write(temp.path().join(format!("dir{i}/file")), b"data").unwrap();
        }
        let service = BrowserService::default();
        let root = service
            .open_path(request(temp.path().to_string_lossy().into_owned()))
            .await
            .unwrap();
        service
            .hold_tree_node(HoldTreeNodeRequestDto {
                pane: PaneIdDto::Left,
                node_id: "root".into(),
                directory: Some(root.directory.clone()),
                entry: None,
            })
            .unwrap();
        for (i, entry) in root.entries.iter().enumerate() {
            service
                .hold_tree_node(HoldTreeNodeRequestDto {
                    pane: PaneIdDto::Left,
                    node_id: i.to_string(),
                    directory: None,
                    entry: Some(entry.reference.clone()),
                })
                .unwrap();
        }
        let (a, b) = tokio::join!(
            service.read_tree_node(node_request("0")),
            service.read_tree_node(node_request("1"))
        );
        let mut listings = vec![a.unwrap(), b.unwrap()];
        for i in 2..140 {
            listings.push(
                service
                    .read_tree_node(node_request(&i.to_string()))
                    .await
                    .unwrap(),
            );
        }
        for listing in &listings {
            assert_eq!(
                service
                    .resolve_entries(vec![listing.entries[0].reference.clone()])
                    .unwrap()
                    .len(),
                1
            );
        }
        assert!(service.resolve_directory(root.directory).is_ok());
        for i in 0..140 {
            service.release_tree_node(TreeNodeRequestDto {
                pane: PaneIdDto::Left,
                node_id: i.to_string(),
            });
        }
        service.release_tree_node(TreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: "root".into(),
        });
        assert!(service.tree_nodes.lock().unwrap().is_empty());
        let sessions = service.sessions.lock().unwrap();
        assert!(sessions.held.is_empty());
        assert!(sessions.sessions.len() <= MAX_STORED_SESSIONS);
    }
    #[tokio::test]
    async fn releasing_pending_node_prevents_publication_and_rejects_symlinks() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("child")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path().join("child"), temp.path().join("link")).unwrap();
        let service = BrowserService::default();
        let root = service
            .open_path(request(temp.path().to_string_lossy().into_owned()))
            .await
            .unwrap();
        service
            .hold_tree_node(HoldTreeNodeRequestDto {
                pane: PaneIdDto::Left,
                node_id: "pending".into(),
                directory: Some(root.directory),
                entry: None,
            })
            .unwrap();
        let pending = service.read_tree_node(node_request("pending"));
        tokio::pin!(pending);
        tokio::select! { biased; result = &mut pending => panic!("unexpected synchronous read: {result:?}"), () = std::future::ready(()) => {} }
        service.release_tree_node(TreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: "pending".into(),
        });
        assert_eq!(pending.await.unwrap_err().code, BrowseErrorCode::Cancelled);
        #[cfg(unix)]
        {
            let link = root
                .entries
                .iter()
                .find(|e| e.display_name == "link")
                .unwrap();
            assert_eq!(
                service
                    .hold_tree_node(HoldTreeNodeRequestDto {
                        pane: PaneIdDto::Right,
                        node_id: "link".into(),
                        directory: None,
                        entry: Some(link.reference.clone())
                    })
                    .unwrap_err()
                    .code,
                BrowseErrorCode::NotDirectory
            );
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
