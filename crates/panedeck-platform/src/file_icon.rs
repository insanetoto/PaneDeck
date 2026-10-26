//! Read-only Finder file icons for the browser list.

#[cfg(target_os = "macos")]
mod imp {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSWorkspace,
    };
    use objc2_foundation::{NSDictionary, NSSize, NSString};

    fn cache() -> &'static Mutex<HashMap<PathBuf, String>> {
        static CACHE: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Returns a `data:image/png;base64,...` URL for the same icon Finder shows.
    pub fn entry_icon_data_url(path: &Path) -> Option<String> {
        let path = path.to_path_buf();
        if let Some(cached) = cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&path)
            .cloned()
        {
            return Some(cached);
        }
        let icon = NSWorkspace::sharedWorkspace()
            .iconForFile(&NSString::from_str(&path.to_string_lossy()));
        icon.setSize(NSSize::new(32.0, 32.0));
        let tiff = icon.TIFFRepresentation()?;
        let representation = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
        let png = unsafe {
            representation
                .representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }?;
        let data = format!("data:image/png;base64,{}", STANDARD.encode(png.to_vec()));
        let mut cache = cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.insert(path, data.clone());
        Some(data)
    }
}

#[cfg(target_os = "macos")]
pub use imp::entry_icon_data_url;

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn entry_icon_data_url(_path: &std::path::Path) -> Option<String> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn produces_png_data_urls_for_common_office_types() {
        let directory = tempfile::tempdir().expect("temp dir");
        for name in ["sample.pdf", "sample.docx", "sample.xlsx"] {
            let path = directory.path().join(name);
            std::fs::write(&path, b"").expect("sample file");
            let data = super::entry_icon_data_url(&path).expect("Finder icon");
            assert!(data.starts_with("data:image/png;base64,"));
            assert!(data.len() > 200, "{name} icon should not be empty");
        }
    }
}
