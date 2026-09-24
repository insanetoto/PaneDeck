//! Operating-system adapter boundary.
//!
//! macOS implementations live here, behind contracts owned by inner layers,
//! so the Rust core remains portable.

use panedeck_domain::DomainLayer;
use panedeck_fs::{FileSystemLayer, TrashBackend, TrashBackendError};

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
