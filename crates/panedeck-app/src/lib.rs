//! Application-service composition root for the Rust core.
//!
//! Tauri may call this crate, but this crate does not depend on Tauri. This
//! keeps commands and events as an outer transport concern.

mod browser;
mod clipboard;
mod context_menu;
mod desktop_menu;
pub use context_menu::{
    ContextCommandDto, ContextMenuEventDto, ContextMenuItemDto, ContextMenuKindDto,
    ContextMenuRequestDto,
};
mod diagnostics;
mod finder_favorites;
mod locations;
mod operation_log;
mod operations;
pub use operation_log::{
    ItemErrorDto, LogStatusDto, OperationLog, OperationLogEventDto, OperationLogSnapshotDto,
};
mod path_ipc;
mod recovery;
pub mod uninstall;

pub use browser::{
    BrowseErrorCode, BrowseErrorDto, BrowserService, DirectoryListingDto, DirectoryWatchEventDto,
    DirectoryWatchReasonDto, HoldTreeNodeRequestDto, NavigateChildRequestDto,
    NavigateDirectoryRequestDto, NavigateKnownRequestDto, PaneIdDto, ReadTreeNodeRequestDto,
    SortDirectionDto, SortFieldDto, StopDirectoryWatchRequestDto, TreeNodeRequestDto,
    WatchDirectoryRequestDto,
};
pub use clipboard::{
    ClipboardErrorCode, ClipboardErrorDto, ClipboardIntentDto, ClipboardService, ClipboardStateDto,
    ConfirmedPasteDto, PasteDraftDto, PasteDraftRequestDto, PasteItemCompletion,
    PreparePasteRequestDto, SetClipboardRequestDto,
};
pub use desktop_menu::{
    DesktopCommandDto, DesktopLanguageDto, DesktopMenuEventDto, DesktopMenuSnapshotDto,
};
pub use diagnostics::{DiagnosticExportRequestDto, DiagnosticPreviewDto, DiagnosticService};

pub use finder_favorites::{FinderFavoriteDto, FinderFavoritesDto, FinderFavoritesService};
pub use locations::{
    LocalLocationDto, LocalLocationKindDto, LocalLocationService, LocalLocationSourceDto,
    LocalLocationWarningDto, LocalLocationsDto,
};
pub use operations::{
    CancelOperationRequestDto, ConflictDecisionDto, ConflictEntryDto, ConflictKindDto,
    CreateChildDirectoryRequestDto, CreateDirectoryRequestDto, OperationConflictDto,
    OperationErrorCode, OperationErrorDto, OperationItemOutcomeDto, OperationJobDto,
    OperationJobStateDto, OperationKindDto, OperationPhaseDto, OperationResultDto,
    OperationService, OperationSnapshotDto, RenameRequestDto, ResolveConflictRequestDto,
    TransferDestinationDto, TransferRequestDto, TrashRequestDto,
};
pub use recovery::{CleanupRecoveryRequestDto, RecoveryError, RecoveryItemDto, RecoveryService};
pub use uninstall::{
    UninstallDataKindDto, UninstallPathDto, UninstallPreviewDto, UninstallRequestDto,
    UninstallResultDto,
};

pub use path_ipc::{
    DirectoryReferenceDto, EntryKindDto, EntryReferenceDto, EntrySnapshotDto, IpcReferenceError,
    SymlinkTargetDto,
};

use panedeck_fs::FileSystemLayer;
use panedeck_jobs::JobsLayer;
use panedeck_platform::PlatformLayer;

/// Application services available to transport adapters such as Tauri.
#[derive(Default)]
pub struct AppServices {
    browser: BrowserService,
    clipboard: ClipboardService,
    diagnostics: DiagnosticService,
    finder_favorites: FinderFavoritesService,
    locations: LocalLocationService,
    operations: OperationService,
    recovery: RecoveryService,
    _file_system: FileSystemLayer,
    _jobs: JobsLayer,
    _platform: PlatformLayer,
}

impl AppServices {
    #[must_use]
    pub fn with_data_directory(directory: std::path::PathBuf) -> Self {
        let recovery = RecoveryService::new(directory.join("operation-journal.jsonl"));
        let operations = OperationService::with_logs(
            recovery.clone(),
            OperationLog::new(directory.join("operations.jsonl")),
        );
        Self {
            operations,
            recovery,
            ..Self::default()
        }
    }

    #[must_use]
    pub const fn browser(&self) -> &BrowserService {
        &self.browser
    }

    #[must_use]
    pub const fn finder_favorites(&self) -> &FinderFavoritesService {
        &self.finder_favorites
    }

    #[must_use]
    pub const fn locations(&self) -> &LocalLocationService {
        &self.locations
    }

    #[must_use]
    pub const fn clipboard(&self) -> &ClipboardService {
        &self.clipboard
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticService {
        &self.diagnostics
    }

    #[must_use]
    pub const fn operations(&self) -> &OperationService {
        &self.operations
    }

    #[must_use]
    pub const fn recovery(&self) -> &RecoveryService {
        &self.recovery
    }
}

impl std::fmt::Debug for AppServices {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AppServices(<state redacted>)")
    }
}

/// Starts native volume-change observation through the platform layer.
#[must_use]
pub fn observe_volume_changes<F>(callback: F) -> Option<panedeck_platform::VolumeObserver>
where
    F: Fn() + 'static,
{
    panedeck_platform::observe_volume_changes(callback)
}

/// Returns the stable product name without exposing domain internals to Tauri.
#[must_use]
pub const fn product_name() -> &'static str {
    panedeck_domain::PRODUCT_NAME
}

#[cfg(test)]
mod tests {
    use super::{product_name, AppServices};

    #[test]
    fn composition_root_constructs_without_platform_side_effects() {
        let services = AppServices::default();
        assert!(format!("{services:?}").contains("AppServices"));
        assert_eq!(product_name(), "PaneDeck");
    }
}
