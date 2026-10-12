use std::{
    fs,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use panedeck_app::{
    AppServices, ClipboardIntentDto, ClipboardService, ClipboardStateDto, ConflictDecisionDto,
    CreateChildDirectoryRequestDto, CreateDirectoryRequestDto, EntryReferenceDto,
    NavigateDirectoryRequestDto, OperationItemOutcomeDto, OperationJobDto, OperationJobStateDto,
    OperationKindDto, PaneIdDto, PasteDraftRequestDto, PasteItemCompletion, PreparePasteRequestDto,
    RenameRequestDto, ResolveConflictRequestDto, SetClipboardRequestDto, SortDirectionDto,
    SortFieldDto, TransferDestinationDto, TransferRequestDto,
};
use panedeck_domain::NativePath;
use panedeck_fs::{
    FileClipboardBackend, FileClipboardError, FileClipboardSnapshot, OperationPlanner,
    OperationRequest, StandardPreflightProbe, TrashBackend, TrashBackendError, TrashExecutor,
    TrashItemOutcome,
};
use tempfile::TempDir;

fn navigation(pane: PaneIdDto, path: &std::path::Path) -> NavigateDirectoryRequestDto {
    NavigateDirectoryRequestDto {
        pane,
        path: path.to_string_lossy().into_owned(),
        show_hidden: false,
        sort_field: SortFieldDto::Name,
        sort_direction: SortDirectionDto::Ascending,
    }
}

fn entry(listing: &panedeck_app::DirectoryListingDto, name: &str) -> EntryReferenceDto {
    listing
        .entries
        .iter()
        .find(|item| item.display_name == name)
        .unwrap_or_else(|| panic!("missing fixture entry {name}"))
        .reference
        .clone()
}

fn wait_for_terminal(services: &AppServices, job_id: u64) -> OperationJobDto {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let job = services
            .operations()
            .job(job_id)
            .unwrap_or_else(|| panic!("missing job {job_id}"));
        if matches!(
            job.state,
            OperationJobStateDto::Completed
                | OperationJobStateDto::PartiallyFailed
                | OperationJobStateDto::Failed
                | OperationJobStateDto::Cancelled
        ) {
            return job;
        }
        assert!(Instant::now() < deadline, "job {job_id} did not finish");
        thread::sleep(Duration::from_millis(5));
    }
}

async fn reopen(
    services: &AppServices,
    pane: PaneIdDto,
    path: &std::path::Path,
) -> panedeck_app::DirectoryListingDto {
    services
        .browser()
        .open_path(navigation(pane, path))
        .await
        .expect("fixture directory opens")
}

#[tokio::test]
async fn newly_created_folder_can_receive_nested_folders_without_entering_it() {
    let temporary = TempDir::new().unwrap();
    let left = temporary.path().join("left");
    let right = temporary.path().join("right");
    fs::create_dir(&left).unwrap();
    fs::create_dir(&right).unwrap();
    fs::write(left.join("file.txt"), b"test-owned").unwrap();
    let services = AppServices::with_data_directory(temporary.path().join("app-data"));
    let root = reopen(&services, PaneIdDto::Left, &left).await;
    let parent = services
        .operations()
        .create_directory(
            services.browser(),
            CreateDirectoryRequestDto {
                directory: root.directory.clone(),
                name: "parent".into(),
            },
        )
        .unwrap();
    assert_eq!(
        wait_for_terminal(&services, parent.job_id).state,
        OperationJobStateDto::Completed
    );
    let refreshed = reopen(&services, PaneIdDto::Left, &left).await;
    let parent_reference = entry(&refreshed, "parent");
    // Opening the other pane does not redirect the captured parent entry.
    reopen(&services, PaneIdDto::Right, &right).await;
    let child = services
        .operations()
        .create_child_directory(
            services.browser(),
            CreateChildDirectoryRequestDto {
                entry: parent_reference.clone(),
                name: "child".into(),
            },
        )
        .unwrap();
    assert_eq!(
        wait_for_terminal(&services, child.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(left.join("parent/child").is_dir());
    assert!(!left.join("child").exists());
    assert!(!right.join("child").exists());
    let nested = reopen(&services, PaneIdDto::Left, &left.join("parent")).await;
    let grandchild = services
        .operations()
        .create_child_directory(
            services.browser(),
            CreateChildDirectoryRequestDto {
                entry: entry(&nested, "child"),
                name: "grandchild".into(),
            },
        )
        .unwrap();
    assert_eq!(
        wait_for_terminal(&services, grandchild.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(left.join("parent/child/grandchild").is_dir());

    assert!(services
        .operations()
        .create_child_directory(
            services.browser(),
            CreateChildDirectoryRequestDto {
                entry: parent_reference.clone(),
                name: "../escape".into(),
            }
        )
        .is_err());
    assert!(services
        .operations()
        .create_child_directory(
            services.browser(),
            CreateChildDirectoryRequestDto {
                entry: entry(&refreshed, "file.txt"),
                name: "rejected".into(),
            }
        )
        .is_err());
    assert!(services
        .operations()
        .create_child_directory(
            services.browser(),
            CreateChildDirectoryRequestDto {
                entry: parent_reference,
                name: "child".into(),
            }
        )
        .is_err());
    assert_eq!(fs::read(left.join("file.txt")).unwrap(), b"test-owned");
    assert!(!left.join("escape").exists());
}

#[tokio::test]
async fn mvp_browse_copy_move_rename_create_conflict_and_queue_form_one_safe_flow() {
    let temporary = TempDir::new().expect("temporary fixture");
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    let app_data = temporary.path().join("app-data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("copy.txt"), b"copy payload").unwrap();
    fs::write(source.join("move.txt"), b"move payload").unwrap();
    fs::write(source.join("rename.txt"), b"rename payload").unwrap();
    fs::write(source.join("conflict.txt"), b"incoming").unwrap();
    fs::write(destination.join("conflict.txt"), b"existing").unwrap();

    let services = AppServices::with_data_directory(app_data);
    let mut left = reopen(&services, PaneIdDto::Left, &source).await;
    let right = reopen(&services, PaneIdDto::Right, &destination).await;
    assert_eq!(left.display_path, source.to_string_lossy());
    assert_eq!(right.display_path, destination.to_string_lossy());
    assert_ne!(left.directory.session_id, right.directory.session_id);

    let copy = services
        .operations()
        .transfer(
            services.browser(),
            TransferRequestDto {
                kind: OperationKindDto::Copy,
                entries: vec![entry(&left, "copy.txt")],
                destination: TransferDestinationDto::Directory {
                    directory: right.directory.clone(),
                },
            },
        )
        .expect("copy is accepted");
    assert_eq!(
        wait_for_terminal(&services, copy.job_id).state,
        OperationJobStateDto::Completed
    );
    assert_eq!(
        fs::read(destination.join("copy.txt")).unwrap(),
        b"copy payload"
    );
    assert!(source.join("copy.txt").exists());

    let moving = services
        .operations()
        .transfer(
            services.browser(),
            TransferRequestDto {
                kind: OperationKindDto::Move,
                entries: vec![entry(&left, "move.txt")],
                destination: TransferDestinationDto::Directory {
                    directory: right.directory.clone(),
                },
            },
        )
        .expect("move is accepted");
    assert_eq!(
        wait_for_terminal(&services, moving.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(!source.join("move.txt").exists());
    assert_eq!(
        fs::read(destination.join("move.txt")).unwrap(),
        b"move payload"
    );

    left = reopen(&services, PaneIdDto::Left, &source).await;
    let rename = services
        .operations()
        .rename(
            services.browser(),
            RenameRequestDto {
                entry: entry(&left, "rename.txt"),
                new_name: "renamed.txt".to_owned(),
            },
        )
        .expect("rename is accepted");
    assert_eq!(
        wait_for_terminal(&services, rename.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(!source.join("rename.txt").exists());
    assert_eq!(
        fs::read(source.join("renamed.txt")).unwrap(),
        b"rename payload"
    );

    left = reopen(&services, PaneIdDto::Left, &source).await;
    let create = services
        .operations()
        .create_directory(
            services.browser(),
            CreateDirectoryRequestDto {
                directory: left.directory.clone(),
                name: "organized".to_owned(),
            },
        )
        .expect("new folder is accepted");
    assert_eq!(
        wait_for_terminal(&services, create.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(source.join("organized").is_dir());

    left = reopen(&services, PaneIdDto::Left, &source).await;
    let right = reopen(&services, PaneIdDto::Right, &destination).await;
    let conflict = services
        .operations()
        .transfer(
            services.browser(),
            TransferRequestDto {
                kind: OperationKindDto::Copy,
                entries: vec![entry(&left, "conflict.txt")],
                destination: TransferDestinationDto::Directory {
                    directory: right.directory,
                },
            },
        )
        .expect("conflict becomes a decision job");
    assert_eq!(conflict.state, OperationJobStateDto::AwaitingDecision);
    assert!(conflict.conflict.is_some());
    assert_eq!(
        fs::read(destination.join("conflict.txt")).unwrap(),
        b"existing"
    );

    let resolved = services
        .operations()
        .resolve_conflict(ResolveConflictRequestDto {
            job_id: conflict.job_id,
            decision: ConflictDecisionDto::KeepBoth,
            apply_to_remaining: false,
        })
        .expect("keep-both decision is accepted");
    assert_eq!(
        wait_for_terminal(&services, resolved.job_id).state,
        OperationJobStateDto::Completed
    );
    assert_eq!(
        fs::read(destination.join("conflict.txt")).unwrap(),
        b"existing"
    );
    let conflict_payloads = fs::read_dir(&destination)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|item| item.file_name().to_string_lossy().starts_with("conflict"))
        .map(|item| fs::read(item.path()).unwrap())
        .collect::<Vec<_>>();
    assert!(conflict_payloads.contains(&b"existing".to_vec()));
    assert!(conflict_payloads.contains(&b"incoming".to_vec()));

    let jobs = services.operations().jobs();
    assert!(jobs.len() >= 5);
    assert!(jobs
        .iter()
        .all(|job| job.state != OperationJobStateDto::Running));
    assert!(jobs
        .iter()
        .all(|job| !job.item_outcomes.contains(&OperationItemOutcomeDto::Failed)));
}

struct FakeClipboard {
    state: Mutex<FileClipboardSnapshot>,
}

impl Default for FakeClipboard {
    fn default() -> Self {
        Self {
            state: Mutex::new(FileClipboardSnapshot {
                change_count: 0,
                paths: Vec::new(),
            }),
        }
    }
}

impl FileClipboardBackend for FakeClipboard {
    fn write_files(&self, paths: &[NativePath]) -> Result<i64, FileClipboardError> {
        let mut state = self.state.lock().unwrap();
        state.change_count += 1;
        state.paths = paths.to_vec();
        Ok(state.change_count)
    }

    fn read_files(&self) -> Result<FileClipboardSnapshot, FileClipboardError> {
        Ok(self.state.lock().unwrap().clone())
    }
}

#[tokio::test]
async fn cut_and_paste_stays_declarative_until_confirmation_then_uses_the_queue() {
    let temporary = TempDir::new().unwrap();
    let source = temporary.path().join("source");
    let destination = temporary.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(source.join("cut.txt"), b"cut payload").unwrap();
    let services = AppServices::with_data_directory(temporary.path().join("app-data"));
    let left = reopen(&services, PaneIdDto::Left, &source).await;
    let right = reopen(&services, PaneIdDto::Right, &destination).await;
    let reference = entry(&left, "cut.txt");
    let clipboard = ClipboardService::new(FakeClipboard::default());

    let state = clipboard
        .set(
            services.browser(),
            SetClipboardRequestDto {
                pane: PaneIdDto::Left,
                intent: ClipboardIntentDto::Cut,
                entries: vec![reference.clone()],
            },
        )
        .unwrap();
    assert_eq!(state.cut_entries, vec![reference]);
    assert!(source.join("cut.txt").exists(), "cut must not mutate disk");

    let draft = clipboard
        .prepare(
            services.browser(),
            PreparePasteRequestDto {
                target_pane: PaneIdDto::Right,
                destination: right.directory,
            },
        )
        .unwrap();
    assert!(
        source.join("cut.txt").exists(),
        "preview must not mutate disk"
    );
    let confirmed = clipboard
        .confirm(PasteDraftRequestDto {
            draft_id: draft.draft_id,
        })
        .unwrap();
    let operation = clipboard.take_task(confirmed.task_id).unwrap();
    let final_clipboard = Arc::new(Mutex::new(None::<ClipboardStateDto>));
    let captured = Arc::clone(&final_clipboard);
    let completion_clipboard = clipboard.clone();
    let task_id = confirmed.task_id;
    let job = services
        .operations()
        .submit_with_completion(operation, move |outcomes| {
            let mapped = outcomes
                .iter()
                .map(|outcome| match outcome {
                    OperationItemOutcomeDto::Succeeded => PasteItemCompletion::Succeeded,
                    OperationItemOutcomeDto::Cancelled => PasteItemCompletion::Cancelled,
                    _ => PasteItemCompletion::Failed,
                })
                .collect::<Vec<_>>();
            *captured.lock().unwrap() = completion_clipboard.complete_task(task_id, &mapped);
        })
        .unwrap();
    assert_eq!(
        wait_for_terminal(&services, job.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(!source.join("cut.txt").exists());
    assert_eq!(
        fs::read(destination.join("cut.txt")).unwrap(),
        b"cut payload"
    );
    let final_state = final_clipboard.lock().unwrap().clone().unwrap();
    assert_eq!(final_state.item_count, 0);
    assert!(final_state.cut_entries.is_empty());
}

#[derive(Default)]
struct RecordingTrash(Mutex<Vec<NativePath>>);

impl TrashBackend for RecordingTrash {
    fn move_to_trash(&self, path: &std::path::Path) -> Result<(), TrashBackendError> {
        self.0.lock().unwrap().push(NativePath::new(path));
        Ok(())
    }
}

#[test]
fn trash_plan_reaches_the_platform_boundary_without_deleting_the_test_source() {
    let temporary = TempDir::new().unwrap();
    let source = temporary.path().join("trash-me.txt");
    fs::write(&source, b"recoverable").unwrap();
    let plan = OperationPlanner
        .plan(
            OperationRequest::Trash {
                sources: vec![NativePath::new(&source)],
            },
            &StandardPreflightProbe,
        )
        .unwrap();
    let backend = RecordingTrash::default();
    let result = TrashExecutor.execute(&plan, &StandardPreflightProbe, &backend);

    assert_eq!(result.items[0].outcome, TrashItemOutcome::Trashed);
    assert_eq!(
        backend.0.lock().unwrap().as_slice(),
        &[NativePath::new(&source)]
    );
    assert_eq!(fs::read(&source).unwrap(), b"recoverable");
}

#[tokio::test]
async fn tree_parent_and_descendant_selection_copies_once_and_keeps_clipboard_references_aligned() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::create_dir_all(source.join("parent")).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(source.join("parent/file"), b"data").unwrap();
    let services = AppServices::with_data_directory(temp.path().join("app-data"));
    let root = reopen(&services, PaneIdDto::Left, &source).await;
    let parent = entry(&root, "parent");
    services
        .browser()
        .hold_tree_node(panedeck_app::HoldTreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: "parent".into(),
            entry: Some(parent.clone()),
            directory: None,
        })
        .unwrap();
    let children = services
        .browser()
        .read_tree_node(panedeck_app::ReadTreeNodeRequestDto {
            pane: PaneIdDto::Left,
            node_id: "parent".into(),
            show_hidden: false,
            sort_field: SortFieldDto::Name,
            sort_direction: SortDirectionDto::Ascending,
        })
        .await
        .unwrap();
    let refs = vec![entry(&children, "file"), parent.clone()];
    let right = reopen(&services, PaneIdDto::Right, &destination).await;
    let copy = services
        .operations()
        .transfer(
            services.browser(),
            TransferRequestDto {
                kind: OperationKindDto::Copy,
                entries: refs.clone(),
                destination: TransferDestinationDto::Directory {
                    directory: right.directory,
                },
            },
        )
        .unwrap();
    let job = wait_for_terminal(&services, copy.job_id);
    assert_eq!(job.state, OperationJobStateDto::Completed);
    assert_eq!(job.item_count, 1);
    assert_eq!(fs::read(destination.join("parent/file")).unwrap(), b"data");
    assert!(!destination.join("file").exists());
    let clipboard = ClipboardService::new(FakeClipboard::default());
    let state = clipboard
        .set(
            services.browser(),
            SetClipboardRequestDto {
                pane: PaneIdDto::Left,
                intent: ClipboardIntentDto::Cut,
                entries: refs,
            },
        )
        .unwrap();
    assert_eq!(state.item_count, 1);
    assert_eq!(state.cut_entries, vec![parent]);
}

#[tokio::test]
async fn drag_preflight_rejects_unsafe_targets_without_jobs_or_disk_changes() {
    use panedeck_app::OperationErrorCode;
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(root.join("parent/child")).unwrap();
    fs::create_dir(root.join("locked")).unwrap();
    fs::write(root.join("parent/data"), b"unchanged").unwrap();
    fs::write(root.join("file"), b"original").unwrap();
    symlink(root.join("parent/child"), root.join("alias")).unwrap();
    let services = AppServices::with_data_directory(temp.path().join("app-data"));
    let listing = reopen(&services, PaneIdDto::Left, &root).await;
    let parent = entry(&listing, "parent");
    let descendant = reopen(&services, PaneIdDto::Right, &root.join("parent/child")).await;
    let alias = reopen(&services, PaneIdDto::Right, &root.join("alias")).await;
    let locked = reopen(&services, PaneIdDto::Right, &root.join("locked")).await;
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o555)).unwrap();
    let cases = [
        (
            parent.clone(),
            TransferDestinationDto::Directory {
                directory: listing.directory.clone(),
            },
            OperationErrorCode::SamePath,
        ),
        (
            parent.clone(),
            TransferDestinationDto::Entry {
                entry: parent.clone(),
            },
            OperationErrorCode::DirectoryIntoItself,
        ),
        (
            parent.clone(),
            TransferDestinationDto::Directory {
                directory: descendant.directory,
            },
            OperationErrorCode::DirectoryIntoItself,
        ),
        (
            parent.clone(),
            TransferDestinationDto::Directory {
                directory: alias.directory,
            },
            OperationErrorCode::DirectoryIntoItself,
        ),
        (
            entry(&listing, "file"),
            TransferDestinationDto::Directory {
                directory: locked.directory,
            },
            OperationErrorCode::PermissionDenied,
        ),
    ];
    for (source, destination, expected) in cases {
        let request = TransferRequestDto {
            kind: OperationKindDto::Move,
            entries: vec![source],
            destination,
        };
        assert_eq!(
            services
                .operations()
                .validate_transfer(services.browser(), request.clone())
                .unwrap_err()
                .code,
            expected
        );
        assert_eq!(
            services
                .operations()
                .transfer(services.browser(), request)
                .unwrap_err()
                .code,
            expected
        );
    }
    fs::remove_file(root.join("file")).unwrap();
    let stale = TransferRequestDto {
        kind: OperationKindDto::Copy,
        entries: vec![entry(&listing, "file")],
        destination: TransferDestinationDto::Entry { entry: parent },
    };
    assert_eq!(
        services
            .operations()
            .validate_transfer(services.browser(), stale)
            .unwrap_err()
            .code,
        OperationErrorCode::StaleReference
    );
    assert!(services.operations().jobs().is_empty());
    assert_eq!(fs::read(root.join("parent/data")).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(root.join("parent/child")).unwrap().count(), 0);
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn same_pane_multi_drag_and_nested_drag_use_existing_transfer_service() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(root.join("destination/nested")).unwrap();
    fs::write(root.join("a"), b"a").unwrap();
    fs::write(root.join("b"), b"b").unwrap();
    let services = AppServices::with_data_directory(temp.path().join("app-data"));
    let listing = reopen(&services, PaneIdDto::Left, &root).await;
    let request = TransferRequestDto {
        kind: OperationKindDto::Move,
        entries: vec![entry(&listing, "a"), entry(&listing, "b")],
        destination: TransferDestinationDto::Entry {
            entry: entry(&listing, "destination"),
        },
    };
    services
        .operations()
        .validate_transfer(services.browser(), request.clone())
        .unwrap();
    assert!(services.operations().jobs().is_empty());
    let job = services
        .operations()
        .transfer(services.browser(), request)
        .unwrap();
    assert_eq!(
        wait_for_terminal(&services, job.job_id).state,
        OperationJobStateDto::Completed
    );
    assert!(!root.join("a").exists());
    assert!(!root.join("b").exists());
    let children = reopen(&services, PaneIdDto::Left, &root.join("destination")).await;
    let job = services
        .operations()
        .transfer(
            services.browser(),
            TransferRequestDto {
                kind: OperationKindDto::Copy,
                entries: vec![entry(&children, "a")],
                destination: TransferDestinationDto::Entry {
                    entry: entry(&children, "nested"),
                },
            },
        )
        .unwrap();
    assert_eq!(
        wait_for_terminal(&services, job.job_id).state,
        OperationJobStateDto::Completed
    );
    assert_eq!(fs::read(root.join("destination/nested/a")).unwrap(), b"a");
    assert_eq!(fs::read(root.join("destination/a")).unwrap(), b"a");
}

#[tokio::test]
async fn operation_log_correlates_conflict_followups_and_records_rejections_without_secrets() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("private-source");
    let destination = temp.path().join("private-target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(source.join("secret-name"), b"secret-contents").unwrap();
    fs::write(destination.join("secret-name"), b"existing").unwrap();
    fs::write(source.join("other"), b"other").unwrap();
    let services = AppServices::with_data_directory(temp.path().join("app-data"));
    let left = reopen(&services, PaneIdDto::Left, &source).await;
    let right = reopen(&services, PaneIdDto::Right, &destination).await;
    let rejected = services.operations().transfer(
        services.browser(),
        TransferRequestDto {
            kind: OperationKindDto::Move,
            entries: vec![entry(&left, "other")],
            destination: TransferDestinationDto::Directory {
                directory: left.directory.clone(),
            },
        },
    );
    assert!(rejected.is_err());
    let pending = services
        .operations()
        .transfer(
            services.browser(),
            TransferRequestDto {
                kind: OperationKindDto::Copy,
                entries: vec![entry(&left, "secret-name"), entry(&left, "other")],
                destination: TransferDestinationDto::Directory {
                    directory: right.directory,
                },
            },
        )
        .unwrap();
    let id = pending.operation_id.clone();
    let job = services
        .operations()
        .resolve_conflict(ResolveConflictRequestDto {
            job_id: pending.job_id,
            decision: ConflictDecisionDto::KeepBoth,
            apply_to_remaining: false,
        })
        .unwrap();
    wait_for_terminal(&services, job.job_id);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if services
            .operations()
            .log()
            .recent()
            .iter()
            .any(|e| e.operation_id == id && e.event == "finished")
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let events = services.operations().log().recent();
    assert!(events
        .iter()
        .any(|e| e.event == "rejected" && e.errors.iter().any(|e| e.code == "SamePath")));
    let correlated: Vec<_> = events.iter().filter(|e| e.operation_id == id).collect();
    assert_eq!(
        correlated.iter().filter(|e| e.event == "finished").count(),
        1
    );
    assert!(correlated.iter().any(|e| e.event == "conflict"));
    assert!(
        correlated
            .iter()
            .filter_map(|e| e.job_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 3
    );
    let diagnostic = services.diagnostics().preview_with_log(
        &services.operations().jobs(),
        services.recovery(),
        services.operations().log(),
    );
    assert!(!diagnostic.content.contains("secret-name"));
    assert!(!diagnostic.content.contains("secret-contents"));
    assert!(!diagnostic.content.contains("private-source"));
    assert!(diagnostic.content.contains("SamePath"));
}
