//! Application-service composition root for the Rust core.
//!
//! Tauri may call this crate, but this crate does not depend on Tauri. This
//! keeps commands and events as an outer transport concern.

mod browser;
mod clipboard;
mod diagnostics;
mod locations;
mod operations;
mod path_ipc;
mod recovery;

pub use browser::{
    BrowseErrorCode, BrowseErrorDto, BrowserService, DirectoryListingDto, DirectoryWatchEventDto,
    DirectoryWatchReasonDto, NavigateChildRequestDto, NavigateDirectoryRequestDto,
    NavigateKnownRequestDto, PaneIdDto, SortDirectionDto, SortFieldDto,
    StopDirectoryWatchRequestDto, WatchDirectoryRequestDto,
};
pub use clipboard::{
    ClipboardErrorCode, ClipboardErrorDto, ClipboardIntentDto, ClipboardService, ClipboardStateDto,
    ConfirmedPasteDto, PasteDraftDto, PasteDraftRequestDto, PasteItemCompletion,
    PreparePasteRequestDto, SetClipboardRequestDto,
};
pub use diagnostics::{DiagnosticExportRequestDto, DiagnosticPreviewDto, DiagnosticService};

pub use locations::{
    LocalLocationDto, LocalLocationKindDto, LocalLocationService, LocalLocationsDto,
};
pub use operations::{
    CancelOperationRequestDto, ConflictDecisionDto, ConflictEntryDto, ConflictKindDto,
    CreateDirectoryRequestDto, OperationConflictDto, OperationErrorCode, OperationErrorDto,
    OperationItemOutcomeDto, OperationJobDto, OperationJobStateDto, OperationKindDto,
    OperationResultDto, OperationService, RenameRequestDto, ResolveConflictRequestDto,
    TransferDestinationDto, TransferRequestDto, TrashRequestDto,
};
pub use recovery::{CleanupRecoveryRequestDto, RecoveryError, RecoveryItemDto, RecoveryService};

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
        let operations = OperationService::with_recovery(recovery.clone());
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
