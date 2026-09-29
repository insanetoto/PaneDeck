use std::{
    fs,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use panedeck_app::{
    AppServices, ClipboardIntentDto, ClipboardService, ClipboardStateDto, ConflictDecisionDto,
    CreateDirectoryRequestDto, EntryReferenceDto, NavigateDirectoryRequestDto,
    OperationItemOutcomeDto, OperationJobDto, OperationJobStateDto, OperationKindDto, PaneIdDto,
    PasteDraftRequestDto, PasteItemCompletion, PreparePasteRequestDto, RenameRequestDto,
    ResolveConflictRequestDto, SetClipboardRequestDto, SortDirectionDto, SortFieldDto,
    TransferDestinationDto, TransferRequestDto,
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
