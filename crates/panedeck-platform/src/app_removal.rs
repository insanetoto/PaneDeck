//! Reversible removal of the installed application bundle.

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    use objc2_foundation::{NSFileManager, NSString, NSURL};

    /// Moves the app bundle to the user's Trash. Never deletes it permanently.
    pub fn trash_app_bundle(path: &Path) -> Result<PathBuf, String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| format!("could not move application to Trash: {error:?}"))?;
        Ok(path.to_path_buf())
    }
}

#[cfg(target_os = "macos")]
pub use imp::trash_app_bundle;

#[cfg(not(target_os = "macos"))]
pub fn trash_app_bundle(_path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    Err("application removal is only supported on macOS".to_owned())
}
