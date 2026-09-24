//! Application-service composition root for the Rust core.
//!
//! Tauri may call this crate, but this crate does not depend on Tauri. This
//! keeps commands and events as an outer transport concern.

mod browser;
mod locations;
mod path_ipc;

pub use browser::{
    BrowseErrorCode, BrowseErrorDto, BrowserService, DirectoryListingDto, DirectoryWatchEventDto,
    DirectoryWatchReasonDto, NavigateChildRequestDto, NavigateDirectoryRequestDto,
    NavigateKnownRequestDto, PaneIdDto, SortDirectionDto, SortFieldDto,
    StopDirectoryWatchRequestDto, WatchDirectoryRequestDto,
};

pub use locations::{
    LocalLocationDto, LocalLocationKindDto, LocalLocationService, LocalLocationsDto,
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
    locations: LocalLocationService,
    _file_system: FileSystemLayer,
    _jobs: JobsLayer,
    _platform: PlatformLayer,
}

impl AppServices {
    #[must_use]
    pub const fn browser(&self) -> &BrowserService {
        &self.browser
    }

    #[must_use]
    pub const fn locations(&self) -> &LocalLocationService {
        &self.locations
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
