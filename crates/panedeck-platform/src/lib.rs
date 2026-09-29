//! Operating-system adapter boundary.
//!
//! macOS implementations live here, behind contracts owned by inner layers,
//! so the Rust core remains portable.

use panedeck_domain::DomainLayer;
use panedeck_fs::{
    FileClipboardBackend, FileClipboardError, FileClipboardSnapshot, FileSystemLayer, TrashBackend,
    TrashBackendError,
};

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
        trash::delete(path).map_err(|error| map_trash_error(path, error))
    }
}

#[cfg(not(target_os = "macos"))]
impl TrashBackend for SystemTrashAdapter {
    fn move_to_trash(&self, _path: &std::path::Path) -> Result<(), TrashBackendError> {
        Err(TrashBackendError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
fn map_trash_error(path: &std::path::Path, error: trash::Error) -> TrashBackendError {
    match error {
        trash::Error::Os { code: 1 | 13, .. } => TrashBackendError::PermissionDenied,
        trash::Error::CouldNotAccess { .. } | trash::Error::CanonicalizePath { .. } => {
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    TrashBackendError::NotFound
                }
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    TrashBackendError::PermissionDenied
                }
                _ => TrashBackendError::Platform,
            }
        }
        _ => TrashBackendError::Platform,
    }
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
        SystemTrashAdapter
            .move_to_trash(&path)
            .expect("system trash should accept test-owned file");
        assert!(!path.exists());
    }
}
