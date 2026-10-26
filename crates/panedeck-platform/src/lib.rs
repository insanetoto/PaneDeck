//! Operating-system adapter boundary.
//!
//! macOS implementations live here, behind contracts owned by inner layers,
//! so the Rust core remains portable.

use panedeck_domain::DomainLayer;
use panedeck_fs::{
    FileClipboardBackend, FileClipboardError, FileClipboardSnapshot, FileSystemLayer, TrashBackend,
    TrashBackendError,
};

mod app_removal;
mod file_icon;
mod finder_favorites;
mod locations;
mod volume_observer;
pub use app_removal::trash_app_bundle;
pub use file_icon::entry_icon_data_url;
pub use finder_favorites::{
    FinderFavorites, FinderFavoritesAdapter, FinderFavoritesDiscovery, SystemFavorite,
};
pub use locations::{
    DiscoveredLocations, LocationDiscovery, LocationKind, LocationSource, LocationWarning,
    SystemLocation, SystemLocationAdapter,
};
pub use volume_observer::{observe_volume_changes, VolumeObserver};

/// macOS file-URL clipboard adapter. It deliberately carries no cut marker:
/// destructive intent remains owned by PaneDeck's application state.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemFileClipboardAdapter;

#[cfg(target_os = "macos")]
impl FileClipboardBackend for SystemFileClipboardAdapter {
    fn write_files(
        &self,
        paths: &[panedeck_domain::NativePath],
    ) -> Result<i64, FileClipboardError> {
        use objc2::{rc::Retained, runtime::ProtocolObject};
        use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
        use objc2_foundation::{NSArray, NSURL};

        let urls = paths
            .iter()
            .map(|path| {
                NSURL::from_file_path(path.as_path()).ok_or(FileClipboardError::InvalidPath)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = urls
            .into_iter()
            .map(ProtocolObject::from_retained)
            .collect();
        let objects = NSArray::from_retained_slice(&objects);
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if !pasteboard.writeObjects(&objects) {
            return Err(FileClipboardError::Platform);
        }
        Ok(pasteboard.changeCount() as i64)
    }

    fn read_files(&self) -> Result<FileClipboardSnapshot, FileClipboardError> {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
        use objc2_foundation::{NSString, NSURL};

        let pasteboard = NSPasteboard::generalPasteboard();
        let mut paths = Vec::new();
        if let Some(items) = pasteboard.pasteboardItems() {
            for index in 0..items.count() {
                let item = items.objectAtIndex(index);
                // SAFETY: AppKit exports this immutable process-wide pasteboard type constant.
                let file_url_type = unsafe { NSPasteboardTypeFileURL };
                let Some(value) = item.stringForType(file_url_type) else {
                    continue;
                };
                let value = NSString::from_str(&value.to_string());
                let Some(url) = NSURL::URLWithString(&value) else {
                    continue;
                };
                if url.isFileURL() {
                    if let Some(path) = url.to_file_path() {
                        paths.push(panedeck_domain::NativePath::new(path));
                    }
                }
            }
        }
        Ok(FileClipboardSnapshot {
            change_count: pasteboard.changeCount() as i64,
            paths,
        })
    }
}

#[cfg(not(target_os = "macos"))]
impl FileClipboardBackend for SystemFileClipboardAdapter {
    fn write_files(
        &self,
        _paths: &[panedeck_domain::NativePath],
    ) -> Result<i64, FileClipboardError> {
        Err(FileClipboardError::Unsupported)
    }

    fn read_files(&self) -> Result<FileClipboardSnapshot, FileClipboardError> {
        Err(FileClipboardError::Unsupported)
    }
}

/// macOS system Trash adapter. No permanent-delete operation is exposed.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemTrashAdapter;

#[cfg(target_os = "macos")]
impl TrashBackend for SystemTrashAdapter {
    fn move_to_trash(&self, path: &std::path::Path) -> Result<(), TrashBackendError> {
        trash_item(path).map(|_| ())
    }
}

#[cfg(not(target_os = "macos"))]
impl TrashBackend for SystemTrashAdapter {
    fn move_to_trash(&self, _path: &std::path::Path) -> Result<(), TrashBackendError> {
        Err(TrashBackendError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
fn trash_item(
    path: &std::path::Path,
) -> Result<Option<objc2::rc::Retained<objc2_foundation::NSURL>>, TrashBackendError> {
    use objc2_foundation::{NSFileManager, NSURL};

    // Resolve the current URL directly, rather than asking Finder to resolve an
    // AppleScript file object. A moved folder may have been recreated at its old
    // path, including in an iCloud-synced directory. Preserve native path bytes.
    let url = NSURL::from_file_path(path).ok_or(TrashBackendError::Platform)?;
    let mut resulting_url = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting_url))
        .map_err(|error| map_trash_error(&error))?;
    Ok(resulting_url)
}

#[cfg(target_os = "macos")]
fn map_trash_error(error: &objc2_foundation::NSError) -> TrashBackendError {
    map_trash_error_code(&error.domain().to_string(), error.code())
}

#[cfg(target_os = "macos")]
fn map_trash_error_code(domain: &str, code: isize) -> TrashBackendError {
    // Codes only have meaning within their domain. In particular, a script's
    // exit status 1 is not the POSIX EPERM error code.
    if domain == "NSCocoaErrorDomain" {
        return match code {
            257 | 513 => TrashBackendError::PermissionDenied,
            4 | 260 => TrashBackendError::NotFound,
            _ => TrashBackendError::Platform,
        };
    }
    if domain == "NSPOSIXErrorDomain" {
        return match code {
            1 | 13 => TrashBackendError::PermissionDenied,
            2 => TrashBackendError::NotFound,
            _ => TrashBackendError::Platform,
        };
    }
    // Cloud/File Provider and unknown failures must not masquerade as access
    // denials. Do not expose localized NSError text, which may contain paths.
    TrashBackendError::Platform
}

/// Composition marker for platform adapters.
#[derive(Debug, Default)]
pub struct PlatformLayer {
    _domain: DomainLayer,
    _file_system: FileSystemLayer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn trash_errors_require_the_correct_native_domain() {
        for (domain, code, expected) in [
            (
                "NSCocoaErrorDomain",
                257,
                TrashBackendError::PermissionDenied,
            ),
            (
                "NSCocoaErrorDomain",
                513,
                TrashBackendError::PermissionDenied,
            ),
            ("NSCocoaErrorDomain", 4, TrashBackendError::NotFound),
            ("NSCocoaErrorDomain", 260, TrashBackendError::NotFound),
            ("NSPOSIXErrorDomain", 1, TrashBackendError::PermissionDenied),
            (
                "NSPOSIXErrorDomain",
                13,
                TrashBackendError::PermissionDenied,
            ),
            ("NSPOSIXErrorDomain", 2, TrashBackendError::NotFound),
            ("NSCocoaErrorDomain", 1, TrashBackendError::Platform),
            ("NSFileProviderErrorDomain", 1, TrashBackendError::Platform),
            (
                "NSFileProviderErrorDomain",
                513,
                TrashBackendError::Platform,
            ),
            ("AppleScriptExitStatus", 1, TrashBackendError::Platform),
        ] {
            assert_eq!(map_trash_error_code(domain, code), expected);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_trash_reports_a_missing_current_path() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        assert_eq!(
            SystemTrashAdapter.move_to_trash(&temp.path().join("missing")),
            Err(TrashBackendError::NotFound)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "moves test-owned folders to the real macOS Trash; set PANEDECK_RUN_TRASH_INTEGRATION=1"]
    fn opt_in_move_copy_back_then_trash_both_current_folders() {
        use panedeck_domain::NativePath;
        use panedeck_fs::{
            CopyExecutor, CopyItemOutcome, MoveExecutor, MoveItemOutcome, NeverCancel,
            OperationPlanner, OperationRequest, StandardPreflightProbe,
        };

        if std::env::var_os("PANEDECK_RUN_TRASH_INTEGRATION").is_none() {
            return;
        }
        let temp = tempfile::TempDir::new().expect("temp dir");
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        std::fs::create_dir_all(left.join("test1/nested")).unwrap();
        std::fs::create_dir(&right).unwrap();
        std::fs::write(left.join("test1/nested/data.txt"), b"test-owned payload").unwrap();
        let moved = OperationPlanner
            .plan(
                OperationRequest::Move {
                    sources: vec![NativePath::new(left.join("test1"))],
                    destination_directory: NativePath::new(&right),
                },
                &StandardPreflightProbe,
            )
            .unwrap();
        assert_eq!(
            MoveExecutor
                .execute(&moved, &StandardPreflightProbe, &NeverCancel, &mut |_| {})
                .items[0]
                .outcome,
            MoveItemOutcome::Moved
        );
        let copied = OperationPlanner
            .plan(
                OperationRequest::Copy {
                    sources: vec![NativePath::new(right.join("test1"))],
                    destination_directory: NativePath::new(&left),
                },
                &StandardPreflightProbe,
            )
            .unwrap();
        assert_eq!(
            CopyExecutor
                .execute(&copied, &StandardPreflightProbe, &NeverCancel, &mut |_| {})
                .items[0]
                .outcome,
            CopyItemOutcome::Copied
        );
        let mut trashed_items = Vec::new();
        for parent in [&right, &left] {
            let source = parent.join("test1");
            let trashed = trash_item(&source)
                .expect("trash accepts current folder URL")
                .expect("native trash returns resulting URL")
                .to_file_path()
                .expect("result is a file URL");
            assert!(!source.exists());
            assert_eq!(
                std::fs::read(trashed.join("nested/data.txt")).unwrap(),
                b"test-owned payload"
            );
            trashed_items.push(trashed);
        }
        assert_ne!(trashed_items[0], trashed_items[1]);
        // Leave the first test1 in Trash until the second deletion completes,
        // covering native collision handling as well as old-path recreation.
        // Restore only the exact URLs returned for this test's own fixtures.
        for (index, trashed) in trashed_items.into_iter().enumerate() {
            std::fs::rename(trashed, temp.path().join(format!("restored-{index}"))).unwrap();
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_adapter_reports_unsupported() {
        assert_eq!(
            SystemTrashAdapter.move_to_trash(std::path::Path::new("/not-used")),
            Err(TrashBackendError::Unsupported)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "moves a test-owned file to the real macOS Trash; set PANEDECK_RUN_TRASH_INTEGRATION=1"]
    fn opt_in_real_trash_integration() {
        if std::env::var_os("PANEDECK_RUN_TRASH_INTEGRATION").is_none() {
            return;
        }
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("panedeck-trash-integration.txt");
        std::fs::write(&path, b"test-owned").expect("test file");
        let trashed = trash_item(&path)
            .expect("system trash should accept test-owned file")
            .expect("native trash returns resulting URL")
            .to_file_path()
            .expect("result is a file URL");
        assert!(!path.exists());
        assert_eq!(std::fs::read(&trashed).unwrap(), b"test-owned");
        std::fs::rename(trashed, &path).expect("restore exact test-owned trash item");
    }
}
