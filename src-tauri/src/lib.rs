use panedeck_app::{
    AppServices, BrowseErrorDto, DirectoryListingDto, LocalLocationsDto, NavigateChildRequestDto,
    NavigateDirectoryRequestDto, NavigateKnownRequestDto, StopDirectoryWatchRequestDto,
    WatchDirectoryRequestDto,
};
use tauri::Emitter;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppServices::default())
        .invoke_handler(tauri::generate_handler![
            open_directory,
            open_child_directory,
            open_parent_directory,
            reopen_directory,
            list_local_locations,
            start_directory_watch,
            stop_directory_watch
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
