//! Safe removal of PaneDeck's own data and (optionally) its application bundle.
//!
//! The data allowlist is fixed to this application's identifier. User files,
//! volumes, Trash contents, and unrelated application data are never touched.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const APP_IDENTIFIER: &str = "dev.panedeck.desktop";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallDataKindDto {
    ApplicationSupport,
    Caches,
    Preferences,
    WebKit,
    HttpStorages,
    SavedState,
    Logs,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPathDto {
    pub kind: UninstallDataKindDto,
    pub path: String,
    pub exists: bool,
    pub byte_len: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallPreviewDto {
    pub paths: Vec<UninstallPathDto>,
    pub app_bundle: Option<String>,
    pub app_bundle_exists: bool,
    pub app_bundle_byte_len: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UninstallRequestDto {
    pub remove_data: bool,
    pub trash_app: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallResultDto {
    pub app_trashed: bool,
    pub app_bundle: Option<String>,
    pub removed: Vec<UninstallDataKindDto>,
    pub failed: Vec<String>,
}

fn data_paths(home: &Path) -> Vec<(UninstallDataKindDto, PathBuf)> {
    let library = home.join("Library");
    vec![
        (
            UninstallDataKindDto::ApplicationSupport,
            library.join("Application Support").join(APP_IDENTIFIER),
        ),
        (
            UninstallDataKindDto::Caches,
            library.join("Caches").join(APP_IDENTIFIER),
        ),
        (
            UninstallDataKindDto::Preferences,
            library
                .join("Preferences")
                .join(format!("{APP_IDENTIFIER}.plist")),
        ),
        (
            UninstallDataKindDto::WebKit,
            library.join("WebKit").join(APP_IDENTIFIER),
        ),
        (
            UninstallDataKindDto::HttpStorages,
            library.join("HTTPStorages").join(APP_IDENTIFIER),
        ),
        (
            UninstallDataKindDto::SavedState,
            library
                .join("Saved Application State")
                .join(format!("{APP_IDENTIFIER}.savedState")),
        ),
        (
            UninstallDataKindDto::Logs,
            library.join("Logs").join("PaneDeck"),
        ),
    ]
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn path_size(path: &Path, depth: usize) -> u64 {
    if depth > 32 {
        return 0;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| path_size(&entry.path(), depth + 1))
        .sum()
}

fn validate_app_bundle(path: &Path) -> Result<(), String> {
    if path.extension().and_then(|value| value.to_str()) != Some("app") || !path.is_dir() {
        return Err("uninstall refused: application path is not an .app bundle".to_owned());
    }
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(true)
    {
        return Err("uninstall refused: application path must not be a symlink".to_owned());
    }
    let info = path.join("Contents").join("Info.plist");
    let contents = fs::read_to_string(&info)
        .map_err(|_| "uninstall refused: application Info.plist is not readable".to_owned())?;
    if !contents.contains(&format!("<string>{APP_IDENTIFIER}</string>")) {
        return Err("uninstall refused: application bundle identifier does not match".to_owned());
    }
    Ok(())
}

/// Build a read-only preview. Paths are always derived from the fixed allowlist.
#[must_use]
pub fn preview(home: &Path, app_bundle: Option<&Path>) -> UninstallPreviewDto {
    let paths = data_paths(home)
        .into_iter()
        .map(|(kind, path)| UninstallPathDto {
            kind,
            exists: path_exists(&path),
            byte_len: path_size(&path, 0).to_string(),
            path: path.to_string_lossy().into_owned(),
        })
        .collect();
    let app_bundle =
        app_bundle.filter(|path| path.extension().and_then(|v| v.to_str()) == Some("app"));
    UninstallPreviewDto {
        paths,
        app_bundle: app_bundle.map(|path| path.to_string_lossy().into_owned()),
        app_bundle_exists: app_bundle.is_some_and(Path::is_dir),
        app_bundle_byte_len: app_bundle
            .filter(|path| path.is_dir())
            .map(|path| path_size(path, 0).to_string()),
    }
}

/// Removes only the allowlisted application data. The bundle is moved to Trash
/// first when requested, so a data-removal failure leaves the app recoverable.
pub fn uninstall(
    home: &Path,
    app_bundle: Option<&Path>,
    request: UninstallRequestDto,
) -> Result<UninstallResultDto, String> {
    if !request.remove_data && !request.trash_app {
        return Err("uninstall refused: no action selected".to_owned());
    }
    if request.trash_app {
        let bundle = app_bundle.ok_or_else(|| {
            "uninstall refused: PaneDeck is not running from an application bundle".to_owned()
        })?;
        validate_app_bundle(bundle)?;
        panedeck_platform::trash_app_bundle(bundle)?;
    }
    let mut removed = Vec::new();
    let mut failed = Vec::new();
    if request.remove_data {
        for (kind, path) in data_paths(home) {
            if !path.starts_with(home.join("Library")) {
                failed.push(format!("refused path outside Library: {}", path.display()));
                continue;
            }
            if !path_exists(&path) {
                continue;
            }
            let result = if fs::symlink_metadata(&path)
                .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
                .unwrap_or(false)
            {
                fs::remove_file(&path)
            } else {
                fs::remove_dir_all(&path)
            };
            match result {
                Ok(()) => removed.push(kind),
                Err(error) => failed.push(format!("{}: {error}", path.display())),
            }
        }
    }
    Ok(UninstallResultDto {
        app_trashed: request.trash_app,
        app_bundle: app_bundle.map(|path| path.to_string_lossy().into_owned()),
        removed,
        failed,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn preview_and_remove_only_pane_deck_data() {
        let home = TempDir::new().unwrap();
        let support = home
            .path()
            .join("Library/Application Support")
            .join(APP_IDENTIFIER);
        let cache = home.path().join("Library/Caches").join(APP_IDENTIFIER);
        fs::create_dir_all(&support).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(support.join("operations.jsonl"), b"private audit").unwrap();
        fs::write(cache.join("cache.bin"), b"cache").unwrap();
        let user_file = home.path().join("Documents/keep.txt");
        fs::create_dir_all(user_file.parent().unwrap()).unwrap();
        fs::write(&user_file, b"user data").unwrap();

        let preview = preview(home.path(), None);
        assert!(preview
            .paths
            .iter()
            .any(|path| path.kind == UninstallDataKindDto::ApplicationSupport && path.exists));

        let result = uninstall(
            home.path(),
            None,
            UninstallRequestDto {
                remove_data: true,
                trash_app: false,
            },
        )
        .unwrap();
        assert!(result
            .removed
            .contains(&UninstallDataKindDto::ApplicationSupport));
        assert!(result.removed.contains(&UninstallDataKindDto::Caches));
        assert!(!support.exists());
        assert!(!cache.exists());
        assert_eq!(fs::read(&user_file).unwrap(), b"user data");
    }

    #[test]
    fn refuses_a_noop_or_invalid_bundle_removal() {
        let home = TempDir::new().unwrap();
        assert!(uninstall(
            home.path(),
            None,
            UninstallRequestDto {
                remove_data: false,
                trash_app: false,
            }
        )
        .is_err());
        assert!(uninstall(
            home.path(),
            Some(&home.path().join("not-an-app")),
            UninstallRequestDto {
                remove_data: true,
                trash_app: true,
            }
        )
        .is_err());
    }
}
