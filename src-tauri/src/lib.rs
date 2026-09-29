use panedeck_app::{
    AppServices, BrowseErrorDto, CancelOperationRequestDto, CleanupRecoveryRequestDto,
    ClipboardErrorDto, ClipboardStateDto, CreateDirectoryRequestDto, DiagnosticExportRequestDto,
    DiagnosticPreviewDto, DirectoryListingDto, LocalLocationsDto, NavigateChildRequestDto,
    NavigateDirectoryRequestDto, NavigateKnownRequestDto, OperationErrorDto,
    OperationItemOutcomeDto, OperationJobDto, PasteDraftDto, PasteDraftRequestDto,
    PasteItemCompletion, PreparePasteRequestDto, RecoveryItemDto, RenameRequestDto,
    ResolveConflictRequestDto, SetClipboardRequestDto, StopDirectoryWatchRequestDto,
    TransferRequestDto, TrashRequestDto, WatchDirectoryRequestDto,
};
use tauri::{Emitter, Manager};

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
    state
        .diagnostics()
        .preview(&state.operations().jobs(), state.recovery())
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            rename_entry,
            create_directory,
            trash_entries,
            list_operation_jobs,
            cancel_operation_job,
            resolve_operation_conflict,
            list_recovery_items,
            confirm_recovery_cleanup,
            preview_diagnostics,
            export_diagnostics
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
