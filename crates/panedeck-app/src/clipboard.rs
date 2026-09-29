use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use panedeck_domain::NativePath;
use panedeck_fs::{FileClipboardBackend, OperationRequest};
use panedeck_platform::SystemFileClipboardAdapter;
use serde::{Deserialize, Serialize};

use crate::{BrowseErrorDto, BrowserService, DirectoryReferenceDto, EntryReferenceDto, PaneIdDto};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardIntentDto {
    Copy,
    Cut,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetClipboardRequestDto {
    pub pane: PaneIdDto,
    pub intent: ClipboardIntentDto,
    pub entries: Vec<EntryReferenceDto>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparePasteRequestDto {
    pub target_pane: PaneIdDto,
    pub destination: DirectoryReferenceDto,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasteDraftRequestDto {
    pub draft_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardStateDto {
    pub intent: ClipboardIntentDto,
    pub source_pane: Option<PaneIdDto>,
    pub item_count: usize,
    pub cut_entries: Vec<EntryReferenceDto>,
    pub system_synced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteDraftDto {
    pub draft_id: u64,
    pub intent: ClipboardIntentDto,
    pub item_count: usize,
    pub target_pane: PaneIdDto,
    pub destination_display_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedPasteDto {
    pub task_id: u64,
    pub intent: ClipboardIntentDto,
    pub item_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasteItemCompletion {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardErrorCode {
    EmptySelection,
    EmptyClipboard,
    StaleReference,
    StaleDraft,
    SystemClipboard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardErrorDto {
    pub code: ClipboardErrorCode,
}

impl fmt::Display for ClipboardErrorDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "clipboard operation failed: {:?}", self.code)
    }
}

impl std::error::Error for ClipboardErrorDto {}

#[derive(Clone)]
struct ClipboardItem {
    path: NativePath,
    reference: Option<EntryReferenceDto>,
}

struct ClipboardContents {
    intent: ClipboardIntentDto,
    source_pane: Option<PaneIdDto>,
    items: Vec<ClipboardItem>,
    system_change_count: i64,
}

struct PasteDraft {
    contents: ClipboardContents,
    target_pane: PaneIdDto,
    destination: NativePath,
}

struct ConfirmedPaste {
    request: OperationRequest,
    intent: ClipboardIntentDto,
    sources: Vec<NativePath>,
}

#[derive(Default)]
struct ClipboardStore {
    contents: Option<ClipboardContents>,
    drafts: HashMap<u64, PasteDraft>,
    tasks: HashMap<u64, ConfirmedPaste>,
    next_id: u64,
}

pub struct ClipboardService<B = SystemFileClipboardAdapter> {
    backend: Arc<B>,
    store: Arc<Mutex<ClipboardStore>>,
}

impl<B> Clone for ClipboardService<B> {
    fn clone(&self) -> Self {
        Self {
            backend: Arc::clone(&self.backend),
            store: Arc::clone(&self.store),
        }
    }
}

impl Default for ClipboardService<SystemFileClipboardAdapter> {
    fn default() -> Self {
        Self::new(SystemFileClipboardAdapter)
    }
}

impl<B: FileClipboardBackend> ClipboardService<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            store: Arc::new(Mutex::new(ClipboardStore {
                next_id: 1,
                ..ClipboardStore::default()
            })),
        }
    }

    pub fn set(
        &self,
        browser: &BrowserService,
        request: SetClipboardRequestDto,
    ) -> Result<ClipboardStateDto, ClipboardErrorDto> {
        if request.entries.is_empty() {
            return Err(error(ClipboardErrorCode::EmptySelection));
        }
        let paths = browser
            .resolve_entries(request.entries.clone())
            .map_err(map_browse_error)?;
        let change_count = self
            .backend
            .write_files(&paths)
            .map_err(|_| error(ClipboardErrorCode::SystemClipboard))?;
        let contents = ClipboardContents {
            intent: request.intent,
            source_pane: Some(request.pane),
            items: paths
                .into_iter()
                .zip(request.entries.into_iter().map(Some))
                .map(|(path, reference)| ClipboardItem { path, reference })
                .collect(),
            system_change_count: change_count,
        };
        let dto = state_dto(&contents, true);
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contents = Some(contents);
        Ok(dto)
    }

    pub fn prepare(
        &self,
        browser: &BrowserService,
        request: PreparePasteRequestDto,
    ) -> Result<PasteDraftDto, ClipboardErrorDto> {
        let destination = browser
            .resolve_directory(request.destination)
            .map_err(map_browse_error)?;
        let system = self
            .backend
            .read_files()
            .map_err(|_| error(ClipboardErrorCode::SystemClipboard))?;
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let use_internal = store
            .contents
            .as_ref()
            .is_some_and(|contents| contents.system_change_count == system.change_count);
        let contents = if use_internal {
            store
                .contents
                .as_ref()
                .expect("checked above")
                .clone_contents()
        } else {
            if system.paths.is_empty() {
                return Err(error(ClipboardErrorCode::EmptyClipboard));
            }
            ClipboardContents {
                intent: ClipboardIntentDto::Copy,
                source_pane: None,
                items: system
                    .paths
                    .into_iter()
                    .map(|path| ClipboardItem {
                        path,
                        reference: None,
                    })
                    .collect(),
                system_change_count: system.change_count,
            }
        };
        let draft_id = store.next_id;
        store.next_id += 1;
        let dto = PasteDraftDto {
            draft_id,
            intent: contents.intent,
            item_count: contents.items.len(),
            target_pane: request.target_pane,
            destination_display_path: destination.as_path().to_string_lossy().into_owned(),
        };
        store.drafts.insert(
            draft_id,
            PasteDraft {
                contents,
                target_pane: request.target_pane,
                destination,
            },
        );
        Ok(dto)
    }

    pub fn confirm(
        &self,
        request: PasteDraftRequestDto,
    ) -> Result<ConfirmedPasteDto, ClipboardErrorDto> {
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let draft = store
            .drafts
            .remove(&request.draft_id)
            .ok_or_else(|| error(ClipboardErrorCode::StaleDraft))?;
        let sources: Vec<_> = draft
            .contents
            .items
            .iter()
            .map(|item| item.path.clone())
            .collect();
        let operation = match draft.contents.intent {
            ClipboardIntentDto::Copy => OperationRequest::Copy {
                sources: sources.clone(),
                destination_directory: draft.destination,
            },
            ClipboardIntentDto::Cut => OperationRequest::Move {
                sources: sources.clone(),
                destination_directory: draft.destination,
            },
        };
        let task_id = store.next_id;
        store.next_id += 1;
        let dto = ConfirmedPasteDto {
            task_id,
            intent: draft.contents.intent,
            item_count: sources.len(),
        };
        let _target_pane = draft.target_pane;
        store.tasks.insert(
            task_id,
            ConfirmedPaste {
                request: operation,
                intent: draft.contents.intent,
                sources,
            },
        );
        Ok(dto)
    }

    pub fn cancel(&self, request: PasteDraftRequestDto) -> Result<(), ClipboardErrorDto> {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drafts
            .remove(&request.draft_id)
            .map(|_| ())
            .ok_or_else(|| error(ClipboardErrorCode::StaleDraft))
    }

    pub fn take_task(&self, task_id: u64) -> Option<OperationRequest> {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tasks
            .get(&task_id)
            .map(|task| task.request.clone())
    }

    pub fn complete_task(
        &self,
        task_id: u64,
        outcomes: &[PasteItemCompletion],
    ) -> Option<ClipboardStateDto> {
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let task = store.tasks.remove(&task_id)?;
        if task.intent == ClipboardIntentDto::Cut {
            if let Some(contents) = &mut store.contents {
                contents.items.retain(|item| {
                    let index = task.sources.iter().position(|source| source == &item.path);
                    !index.is_some_and(|index| {
                        outcomes.get(index) == Some(&PasteItemCompletion::Succeeded)
                    })
                });
                if contents.items.is_empty() {
                    store.contents = None;
                }
            }
        }
        Some(store.contents.as_ref().map_or(
            ClipboardStateDto {
                intent: ClipboardIntentDto::Copy,
                source_pane: None,
                item_count: 0,
                cut_entries: Vec::new(),
                system_synced: false,
            },
            |contents| state_dto(contents, false),
        ))
    }
}

impl ClipboardContents {
    fn clone_contents(&self) -> Self {
        Self {
            intent: self.intent,
            source_pane: self.source_pane,
            items: self.items.clone(),
            system_change_count: self.system_change_count,
        }
    }
}

fn state_dto(contents: &ClipboardContents, system_synced: bool) -> ClipboardStateDto {
    ClipboardStateDto {
        intent: contents.intent,
        source_pane: contents.source_pane,
        item_count: contents.items.len(),
        cut_entries: if contents.intent == ClipboardIntentDto::Cut {
            contents
                .items
                .iter()
                .filter_map(|item| item.reference.clone())
                .collect()
        } else {
            Vec::new()
        },
        system_synced,
    }
}

fn map_browse_error(_: BrowseErrorDto) -> ClipboardErrorDto {
    error(ClipboardErrorCode::StaleReference)
}

const fn error(code: ClipboardErrorCode) -> ClipboardErrorDto {
    ClipboardErrorDto { code }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use panedeck_fs::{FileClipboardError, FileClipboardSnapshot};
    use tempfile::TempDir;

    use super::*;
    use crate::{NavigateDirectoryRequestDto, SortDirectionDto, SortFieldDto};

    #[derive(Clone)]
    struct FakeClipboard {
        state: Arc<Mutex<FileClipboardSnapshot>>,
    }

    impl Default for FakeClipboard {
        fn default() -> Self {
            Self {
                state: Arc::new(Mutex::new(FileClipboardSnapshot {
                    change_count: 0,
                    paths: Vec::new(),
                })),
            }
        }
    }

    impl FileClipboardBackend for FakeClipboard {
        fn write_files(&self, paths: &[NativePath]) -> Result<i64, FileClipboardError> {
            let mut state = self.state.lock().expect("clipboard state");
            state.change_count += 1;
            state.paths = paths.to_vec();
            Ok(state.change_count)
        }

        fn read_files(&self) -> Result<FileClipboardSnapshot, FileClipboardError> {
            Ok(self.state.lock().expect("clipboard state").clone())
        }
    }

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

    #[tokio::test]
    async fn cut_is_declarative_and_cancel_keeps_sources() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).expect("source dir");
        fs::create_dir_all(&destination).expect("destination dir");
        let source_path = source.join("item.txt");
        fs::write(&source_path, b"owned by test").expect("source file");
        let browser = BrowserService::default();
        let source_listing = open(&browser, PaneIdDto::Left, &source).await;
        let destination_listing = open(&browser, PaneIdDto::Right, &destination).await;
        let clipboard = ClipboardService::new(FakeClipboard::default());

        let state = clipboard
            .set(
                &browser,
                SetClipboardRequestDto {
                    pane: PaneIdDto::Left,
                    intent: ClipboardIntentDto::Cut,
                    entries: vec![source_listing.entries[0].reference.clone()],
                },
            )
            .expect("cut state");
        assert!(source_path.exists(), "cut must not modify the file system");
        assert_eq!(state.cut_entries.len(), 1);

        let draft = clipboard
            .prepare(
                &browser,
                PreparePasteRequestDto {
                    target_pane: PaneIdDto::Right,
                    destination: destination_listing.directory,
                },
            )
            .expect("paste draft");
        assert_eq!(draft.target_pane, PaneIdDto::Right);
        assert_eq!(
            draft.destination_display_path,
            destination.to_string_lossy()
        );
        clipboard
            .cancel(PasteDraftRequestDto {
                draft_id: draft.draft_id,
            })
            .expect("cancel draft");
        assert!(source_path.exists(), "cancel must keep the source");
        assert_eq!(state.item_count, 1);
    }

    #[tokio::test]
    async fn only_successful_move_items_leave_cut_state() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).expect("source dir");
        fs::create_dir_all(&destination).expect("destination dir");
        fs::write(source.join("a.txt"), b"a").expect("a");
        fs::write(source.join("b.txt"), b"b").expect("b");
        let browser = BrowserService::default();
        let source_listing = open(&browser, PaneIdDto::Left, &source).await;
        let destination_listing = open(&browser, PaneIdDto::Right, &destination).await;
        let clipboard = ClipboardService::new(FakeClipboard::default());
        clipboard
            .set(
                &browser,
                SetClipboardRequestDto {
                    pane: PaneIdDto::Left,
                    intent: ClipboardIntentDto::Cut,
                    entries: source_listing
                        .entries
                        .iter()
                        .map(|entry| entry.reference.clone())
                        .collect(),
                },
            )
            .expect("cut state");
        let draft = clipboard
            .prepare(
                &browser,
                PreparePasteRequestDto {
                    target_pane: PaneIdDto::Right,
                    destination: destination_listing.directory,
                },
            )
            .expect("draft");
        let confirmed = clipboard
            .confirm(PasteDraftRequestDto {
                draft_id: draft.draft_id,
            })
            .expect("confirmed");
        assert!(matches!(
            clipboard.take_task(confirmed.task_id),
            Some(OperationRequest::Move { .. })
        ));
        let state = clipboard
            .complete_task(
                confirmed.task_id,
                &[PasteItemCompletion::Succeeded, PasteItemCompletion::Failed],
            )
            .expect("completion state");
        assert_eq!(state.item_count, 1);
        assert_eq!(state.cut_entries.len(), 1);
        assert!(source.join("a.txt").exists());
        assert!(source.join("b.txt").exists());
    }

    #[tokio::test]
    async fn externally_replaced_system_clipboard_is_imported_as_copy() {
        let temp = TempDir::new().expect("temp dir");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(&source).expect("source dir");
        fs::create_dir_all(&destination).expect("destination dir");
        fs::write(source.join("internal.txt"), b"internal").expect("internal");
        let external = temp.path().join("external.txt");
        fs::write(&external, b"external").expect("external");
        let browser = BrowserService::default();
        let source_listing = open(&browser, PaneIdDto::Left, &source).await;
        let destination_listing = open(&browser, PaneIdDto::Right, &destination).await;
        let backend = FakeClipboard::default();
        let clipboard = ClipboardService::new(backend.clone());
        clipboard
            .set(
                &browser,
                SetClipboardRequestDto {
                    pane: PaneIdDto::Left,
                    intent: ClipboardIntentDto::Cut,
                    entries: vec![source_listing.entries[0].reference.clone()],
                },
            )
            .expect("cut state");
        backend
            .write_files(&[NativePath::new(&external)])
            .expect("external replacement");

        let draft = clipboard
            .prepare(
                &browser,
                PreparePasteRequestDto {
                    target_pane: PaneIdDto::Right,
                    destination: destination_listing.directory,
                },
            )
            .expect("external draft");
        assert_eq!(draft.intent, ClipboardIntentDto::Copy);
    }
}
