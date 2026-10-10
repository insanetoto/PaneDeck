use panedeck_app::{
    AppServices, BrowseErrorDto, CancelOperationRequestDto, CleanupRecoveryRequestDto,
    ClipboardErrorDto, ClipboardStateDto, CreateDirectoryRequestDto, DiagnosticExportRequestDto,
    DiagnosticPreviewDto, DirectoryListingDto, HoldTreeNodeRequestDto, LocalLocationsDto,
    NavigateChildRequestDto, NavigateDirectoryRequestDto, NavigateKnownRequestDto,
    OperationErrorDto, OperationItemOutcomeDto, OperationJobDto, PasteDraftDto,
    PasteDraftRequestDto, PasteItemCompletion, PreparePasteRequestDto, ReadTreeNodeRequestDto,
    RecoveryItemDto, RenameRequestDto, ResolveConflictRequestDto, SetClipboardRequestDto,
    StopDirectoryWatchRequestDto, TransferRequestDto, TrashRequestDto, TreeNodeRequestDto,
    WatchDirectoryRequestDto,
};
use tauri::{Emitter, Manager};
#[derive(Default)]
struct OperationWatchState(std::sync::Mutex<std::collections::BTreeMap<String, u64>>);
mod context_menu;
mod desktop_menu;
use context_menu::{popup_context_menu, ContextMenuState};
use desktop_menu::{sync_desktop_menu, DesktopMenuState};

#[tauri::command]
fn hold_tree_node(
    state: tauri::State<'_, AppServices>,
    request: HoldTreeNodeRequestDto,
) -> Result<(), BrowseErrorDto> {
    state.browser().hold_tree_node(request)
}
#[tauri::command]
fn release_tree_node(state: tauri::State<'_, AppServices>, request: TreeNodeRequestDto) {
    state.browser().release_tree_node(request);
}
#[tauri::command]
async fn read_tree_node(
    state: tauri::State<'_, AppServices>,
    request: ReadTreeNodeRequestDto,
) -> Result<DirectoryListingDto, BrowseErrorDto> {
    state.browser().read_tree_node(request).await
}

#[tauri::command]
async fn open_directory(
    state: tauri::State<'_, AppServices>,
    request: NavigateDirectoryRequestDto,
) -> Result<DirectoryListingDto, BrowseErrorDto> {
    state.browser().open_path(request).await
}

#[tauri::command]
async fn open_child_directory(
    state: tauri::State<'_, AppServices>,
    request: NavigateChildRequestDto,
) -> Result<DirectoryListingDto, BrowseErrorDto> {
    state.browser().open_child(request).await
}

#[tauri::command]
async fn open_parent_directory(
    state: tauri::State<'_, AppServices>,
    request: NavigateKnownRequestDto,
) -> Result<DirectoryListingDto, BrowseErrorDto> {
    state.browser().open_parent(request).await
}

#[tauri::command]
async fn reopen_directory(
    state: tauri::State<'_, AppServices>,
    request: NavigateKnownRequestDto,
) -> Result<DirectoryListingDto, BrowseErrorDto> {
    state.browser().reopen(request).await
}

#[tauri::command]
fn list_local_locations(state: tauri::State<'_, AppServices>) -> LocalLocationsDto {
    state.locations().discover()
}

#[tauri::command]
fn start_directory_watch(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppServices>,
    request: WatchDirectoryRequestDto,
) -> Result<u64, BrowseErrorDto> {
    state.browser().start_watch(request, move |event| {
        let _ = app.emit("directory-rescan", event);
    })
}

#[tauri::command]
fn stop_directory_watch(
    state: tauri::State<'_, AppServices>,
    request: StopDirectoryWatchRequestDto,
) {
    state.browser().stop_watch(request);
}

#[tauri::command]
fn set_file_clipboard(
    state: tauri::State<'_, AppServices>,
    request: SetClipboardRequestDto,
) -> Result<ClipboardStateDto, ClipboardErrorDto> {
    state.clipboard().set(state.browser(), request)
}

#[tauri::command]
fn prepare_file_paste(
    state: tauri::State<'_, AppServices>,
    request: PreparePasteRequestDto,
) -> Result<PasteDraftDto, ClipboardErrorDto> {
    state.clipboard().prepare(state.browser(), request)
}

#[tauri::command]
fn confirm_file_paste(
    state: tauri::State<'_, AppServices>,
    request: PasteDraftRequestDto,
) -> Result<OperationJobDto, String> {
    let confirmed = state
        .clipboard()
        .confirm(request)
        .map_err(|error| error.to_string())?;
    let operation = state
        .clipboard()
        .take_task(confirmed.task_id)
        .ok_or_else(|| "confirmed paste task expired".to_owned())?;
    let clipboard = state.clipboard().clone();
    state
        .operations()
        .submit_with_completion(operation, move |outcomes| {
            let outcomes: Vec<_> = outcomes
                .iter()
                .map(|outcome| match outcome {
                    OperationItemOutcomeDto::Succeeded => PasteItemCompletion::Succeeded,
                    OperationItemOutcomeDto::Skipped => PasteItemCompletion::Failed,
                    OperationItemOutcomeDto::Cancelled => PasteItemCompletion::Cancelled,
                    OperationItemOutcomeDto::Failed
                    | OperationItemOutcomeDto::CopiedButSourceNotRemoved => {
                        PasteItemCompletion::Failed
                    }
                })
                .collect();
            clipboard.complete_task(confirmed.task_id, &outcomes);
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_file_paste(
    state: tauri::State<'_, AppServices>,
    request: PasteDraftRequestDto,
) -> Result<(), ClipboardErrorDto> {
    state.clipboard().cancel(request)
}

#[tauri::command]
fn validate_transfer(
    state: tauri::State<'_, AppServices>,
    request: TransferRequestDto,
) -> Result<(), OperationErrorDto> {
    state
        .operations()
        .validate_transfer(state.browser(), request)
}

#[tauri::command]
fn execute_transfer(
    state: tauri::State<'_, AppServices>,
    request: TransferRequestDto,
) -> Result<OperationJobDto, OperationErrorDto> {
    state.operations().transfer(state.browser(), request)
}

#[tauri::command]
fn rename_entry(
    state: tauri::State<'_, AppServices>,
    request: RenameRequestDto,
) -> Result<OperationJobDto, OperationErrorDto> {
    state.operations().rename(state.browser(), request)
}

#[tauri::command]
fn create_directory(
    state: tauri::State<'_, AppServices>,
    request: CreateDirectoryRequestDto,
) -> Result<OperationJobDto, OperationErrorDto> {
    state
        .operations()
        .create_directory(state.browser(), request)
}

#[tauri::command]
fn trash_entries(
    state: tauri::State<'_, AppServices>,
    request: TrashRequestDto,
) -> Result<OperationJobDto, OperationErrorDto> {
    state.operations().trash(state.browser(), request)
}

#[tauri::command]
fn operation_log(state: tauri::State<'_, AppServices>) -> panedeck_app::OperationLogSnapshotDto {
    panedeck_app::OperationLogSnapshotDto {
        events: state.operations().log().recent(),
        status: state.operations().log().status(),
        recovery_status: state.recovery().log_status(),
    }
}
#[tauri::command]
fn start_operation_watch(
    window: tauri::Window,
    state: tauri::State<'_, AppServices>,
    token: String,
    watches: tauri::State<'_, OperationWatchState>,
) -> Result<(u64, panedeck_app::OperationSnapshotDto), String> {
    if token.is_empty()
        || token.len() > 64
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid watch token".into());
    }
    let mut owned = watches
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let label = window.label().to_owned();
    if let Some(previous) = owned.remove(&label) {
        state.operations().unwatch(previous);
    }
    let result = state.operations().watch(move |snapshot| {
        let _ = window.emit("operation-state", (&token, snapshot));
    });
    owned.insert(label, result.0);
    Ok(result)
}
#[tauri::command]
fn stop_operation_watch(
    window: tauri::Window,
    state: tauri::State<'_, AppServices>,
    watches: tauri::State<'_, OperationWatchState>,
    watch_id: u64,
) {
    let mut owned = watches
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if owned.get(window.label()) == Some(&watch_id) {
        owned.remove(window.label());
        state.operations().unwatch(watch_id);
    }
}

#[tauri::command]
fn list_operation_jobs(state: tauri::State<'_, AppServices>) -> Vec<OperationJobDto> {
    state.operations().jobs()
}

#[tauri::command]
fn cancel_operation_job(
    state: tauri::State<'_, AppServices>,
    request: CancelOperationRequestDto,
) -> Result<OperationJobDto, OperationErrorDto> {
    state.operations().cancel(request.job_id)
}

#[tauri::command]
fn resolve_operation_conflict(
    state: tauri::State<'_, AppServices>,
    request: ResolveConflictRequestDto,
) -> Result<OperationJobDto, OperationErrorDto> {
    state.operations().resolve_conflict(request)
}

#[tauri::command]
fn list_recovery_items(state: tauri::State<'_, AppServices>) -> Vec<RecoveryItemDto> {
    state.recovery().items()
}

#[tauri::command]
fn confirm_recovery_cleanup(
    state: tauri::State<'_, AppServices>,
    request: CleanupRecoveryRequestDto,
) -> Result<(), String> {
    state
        .recovery()
        .confirm_cleanup(&request.recovery_id)
        .map_err(|_| "could not clean the selected recovery residue".to_owned())
}

#[tauri::command]
fn preview_diagnostics(state: tauri::State<'_, AppServices>) -> DiagnosticPreviewDto {
    state.diagnostics().preview_with_log(
        &state.operations().jobs(),
        state.recovery(),
        state.operations().log(),
    )
}

#[tauri::command]
fn export_diagnostics(
    state: tauri::State<'_, AppServices>,
    request: DiagnosticExportRequestDto,
) -> Result<(), String> {
    state.diagnostics().export(request)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            app.manage(AppServices::with_data_directory(directory));
            app.manage(DesktopMenuState::default());
            app.manage(ContextMenuState::default());
            app.manage(OperationWatchState::default());
            app.set_menu(desktop_menu::build(
                app.handle(),
                &panedeck_app::DesktopMenuSnapshotDto {
                    revision: "0".to_owned(),
                    ..Default::default()
                },
            )?)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let state = window.state::<OperationWatchState>();
                let id = state
                    .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(window.label());
                if let Some(id) = id {
                    window.state::<AppServices>().operations().unwatch(id);
                }
            }
        })
        .on_menu_event(|app, event| {
            context_menu::handle_event(app, event.id().as_ref());
            desktop_menu::handle_event(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            hold_tree_node,
            read_tree_node,
            release_tree_node,
            open_directory,
            open_child_directory,
            open_parent_directory,
            reopen_directory,
            list_local_locations,
            start_directory_watch,
            stop_directory_watch,
            set_file_clipboard,
            prepare_file_paste,
            confirm_file_paste,
            cancel_file_paste,
            execute_transfer,
            validate_transfer,
            rename_entry,
            create_directory,
            trash_entries,
            list_operation_jobs,
            operation_log,
            start_operation_watch,
            stop_operation_watch,
            cancel_operation_job,
            resolve_operation_conflict,
            list_recovery_items,
            confirm_recovery_cleanup,
            preview_diagnostics,
            export_diagnostics,
            sync_desktop_menu,
            popup_context_menu
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| panic!("failed to run {}: {error}", panedeck_app::product_name()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_name_is_stable() {
        assert_eq!(panedeck_app::product_name(), "PaneDeck");
    }
}
