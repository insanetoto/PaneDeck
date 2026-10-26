//! Read-only access to the user's Finder sidebar favorites.
//!
//! The deprecated `LSSharedFileList` API only exposes the user list to a
//! bundled application; an unbundled development binary receives an empty
//! list. Resolution uses the no-user-interaction and no-mount flags, and every
//! entry is filtered to an existing directory before it reaches the UI.

use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemFavorite {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub available: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FinderFavorites {
    pub favorites: Vec<SystemFavorite>,
    /// Entries that could not be resolved to an existing directory.
    pub unresolved: usize,
    /// The list itself could not be created or read.
    pub failed: bool,
    /// Whether hidden-item visibility could be read from the private archive.
    pub visibility_available: bool,
}

pub trait FinderFavoritesDiscovery {
    fn discover(&self) -> FinderFavorites;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FinderFavoritesAdapter;

impl FinderFavoritesDiscovery for FinderFavoritesAdapter {
    fn discover(&self) -> FinderFavorites {
        platform::discover()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        collections::HashMap,
        ffi::{c_char, c_void, CStr},
        os::raw::c_int,
        path::PathBuf,
    };

    use objc2::runtime::AnyObject;
    use objc2::{rc::Retained, AnyThread};
    use objc2_foundation::{
        NSArray, NSData, NSDictionary, NSFileManager, NSKeyedUnarchiver, NSNumber, NSString,
        NSURLBookmarkResolutionOptions, NSURL,
    };

    use super::*;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFURLRef = *const c_void;

    const FLAGS_NO_INTERACTION_NO_MOUNT: u32 = (1 << 0) | (1 << 1);
    const CF_URL_POSIX_PATH_STYLE: c_int = 0;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        static kLSSharedFileListFavoriteItems: CFStringRef;
        fn LSSharedFileListCreate(
            allocator: CFTypeRef,
            list_type: CFStringRef,
            options: CFTypeRef,
        ) -> CFTypeRef;
        fn LSSharedFileListCopySnapshot(list: CFTypeRef, seed: *mut u32) -> CFArrayRef;
        fn LSSharedFileListItemCopyResolvedURL(
            item: CFTypeRef,
            flags: u32,
            error: *mut CFTypeRef,
        ) -> CFURLRef;
        fn LSSharedFileListItemCopyDisplayName(item: CFTypeRef) -> CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFURLLocalizedNameKey: CFStringRef;
        fn CFRelease(value: CFTypeRef);
        fn CFArrayGetCount(array: CFArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> *const c_void;
        fn CFURLCopyFileSystemPath(url: CFURLRef, style: c_int) -> CFStringRef;
        fn CFURLCopyResourcePropertyForKey(
            url: CFURLRef,
            key: CFStringRef,
            value: *mut CFTypeRef,
            error: *mut CFTypeRef,
        ) -> u8;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFStringGetLength(string: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    }

    /// Read-only visibility hints from the shared-list archive.
    ///
    /// `LSSharedFileList` no longer exposes the hidden flag. The archive keeps
    /// a `visibility` value per item; a missing entry (for example a just-added
    /// favorite before Finder flushes) is treated as visible.
    pub(super) fn archived_visibility() -> Option<HashMap<PathBuf, i32>> {
        let home = NSFileManager::defaultManager()
            .homeDirectoryForCurrentUser()
            .path()?;
        let path = PathBuf::from(home.to_string()).join(
            "Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.FavoriteItems.sfl4",
        );
        let bytes = std::fs::read(&path).ok()?;
        let data = NSData::with_bytes(&bytes);
        let unarchiver = match unsafe {
            NSKeyedUnarchiver::initForReadingFromData_error(NSKeyedUnarchiver::alloc(), &data)
        } {
            Ok(unarchiver) => unarchiver,
            Err(_) => return None,
        };
        unsafe { unarchiver.setRequiresSecureCoding(false) };
        let root_key = NSString::from_str("root");
        let root = unsafe { unarchiver.decodeObjectForKey(&root_key) }?;
        let root = root.downcast_ref::<NSDictionary<AnyObject, AnyObject>>()?;
        let items_key: Retained<AnyObject> = NSString::from_str("items").into();
        let items = root.objectForKey(&*items_key)?;
        let items = items.downcast_ref::<NSArray<AnyObject>>()?;
        let visibility_key: Retained<AnyObject> = NSString::from_str("visibility").into();
        let bookmark_key: Retained<AnyObject> = NSString::from_str("Bookmark").into();
        let mut visibility = HashMap::new();
        for index in 0..items.count() {
            let item = items.objectAtIndex(index);
            let Some(item) = item.downcast_ref::<NSDictionary<AnyObject, AnyObject>>() else {
                continue;
            };
            let Some(value) = item.objectForKey(&*visibility_key) else {
                continue;
            };
            let Some(value) = value.downcast_ref::<NSNumber>() else {
                continue;
            };
            let Some(bookmark) = item.objectForKey(&*bookmark_key) else {
                continue;
            };
            let Some(bookmark) = bookmark.downcast_ref::<NSData>() else {
                continue;
            };
            let Ok(url) = (unsafe {
                NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                    bookmark,
                    NSURLBookmarkResolutionOptions::WithoutUI
                        | NSURLBookmarkResolutionOptions::WithoutMounting,
                    None,
                    std::ptr::null_mut(),
                )
            }) else {
                continue;
            };
            if let Some(path) = url.path() {
                visibility.insert(PathBuf::from(path.to_string()), value.intValue());
            }
        }
        Some(visibility)
    }

    pub(super) fn is_visible(visibility: Option<i32>) -> bool {
        visibility != Some(0)
    }

    pub(super) fn discover() -> FinderFavorites {
        let result = unsafe { read_favorites() };
        result.unwrap_or(FinderFavorites {
            favorites: Vec::new(),
            unresolved: 0,
            failed: true,
            visibility_available: false,
        })
    }

    unsafe fn read_favorites() -> Option<FinderFavorites> {
        // SAFETY: The constants and CoreServices functions are process-wide
        // immutable Foundation/CoreServices APIs.
        let list = unsafe {
            LSSharedFileListCreate(
                std::ptr::null(),
                kLSSharedFileListFavoriteItems,
                std::ptr::null(),
            )
        };
        if list.is_null() {
            return None;
        }
        let mut seed = 0_u32;
        let snapshot = unsafe { LSSharedFileListCopySnapshot(list, &mut seed) };
        if snapshot.is_null() {
            unsafe { CFRelease(list) };
            return None;
        }
        let count = unsafe { CFArrayGetCount(snapshot) };
        let visibility = archived_visibility();
        let visibility_available = visibility.is_some();
        let Some(visibility) = visibility else {
            // Without the visibility archive we cannot tell visible Finder items
            // from hidden ones. Return no shared items rather than exposing stale
            // or hidden favorites; the UI explains the missing permission.
            unsafe { CFRelease(snapshot) };
            unsafe { CFRelease(list) };
            return Some(FinderFavorites {
                favorites: Vec::new(),
                unresolved: 0,
                failed: false,
                visibility_available: false,
            });
        };
        let mut favorites = Vec::new();
        let mut unresolved = 0_usize;
        for index in 0..count {
            let item = unsafe { CFArrayGetValueAtIndex(snapshot, index) };
            if item.is_null() {
                unresolved += 1;
                continue;
            }
            let url = unsafe {
                LSSharedFileListItemCopyResolvedURL(
                    item,
                    FLAGS_NO_INTERACTION_NO_MOUNT,
                    std::ptr::null_mut(),
                )
            };
            if url.is_null() {
                unresolved += 1;
                continue;
            }
            let path = unsafe { url_path(url) };
            let localized = unsafe { localized_name(url) };
            let display = unsafe { LSSharedFileListItemCopyDisplayName(item) };
            let display_name = unsafe { cf_string_to_string(display) };
            if !display.is_null() {
                unsafe { CFRelease(display) };
            }
            unsafe { CFRelease(url) };
            let Some(path) = path else {
                unresolved += 1;
                continue;
            };
            if !path.is_dir() {
                unresolved += 1;
                continue;
            }
            if !is_visible(visibility.get(&path).copied()) {
                continue;
            }
            let name = display_name
                .filter(|name| !name.is_empty())
                .or(localized)
                .or_else(|| {
                    path.file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let id = format!("finder:{}", path.to_string_lossy());
            if favorites
                .iter()
                .any(|favorite: &SystemFavorite| favorite.path == path)
            {
                continue;
            }
            favorites.push(SystemFavorite {
                id,
                name,
                path,
                available: true,
            });
        }
        unsafe { CFRelease(snapshot) };
        unsafe { CFRelease(list) };
        Some(FinderFavorites {
            favorites,
            unresolved,
            failed: false,
            visibility_available,
        })
    }

    unsafe fn url_path(url: CFURLRef) -> Option<PathBuf> {
        // SAFETY: `url` is a retained file URL from CoreServices.
        let value = unsafe { CFURLCopyFileSystemPath(url, CF_URL_POSIX_PATH_STYLE) };
        let path = unsafe { cf_string_to_string(value) };
        if !value.is_null() {
            unsafe { CFRelease(value) };
        }
        path.map(PathBuf::from)
    }

    unsafe fn localized_name(url: CFURLRef) -> Option<String> {
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: The key and URL are process-wide Foundation objects; the
        // out parameter is initialized and released below when non-null.
        let success = unsafe {
            CFURLCopyResourcePropertyForKey(
                url,
                kCFURLLocalizedNameKey,
                &mut value,
                std::ptr::null_mut(),
            )
        };
        if success == 0 || value.is_null() {
            if !value.is_null() {
                unsafe { CFRelease(value) };
            }
            return None;
        }
        let name = unsafe { cf_string_to_string(value) };
        unsafe { CFRelease(value) };
        name
    }

    unsafe fn cf_string_to_string(value: CFStringRef) -> Option<String> {
        if value.is_null() {
            return None;
        }
        // SAFETY: `value` is a CFString; buffers are sized from its UTF-16
        // length and released by the caller only for owned copies.
        let length = unsafe { CFStringGetLength(value) };
        let maximum = unsafe { CFStringGetMaximumSizeForEncoding(length, CF_STRING_ENCODING_UTF8) };
        let size = if maximum <= 0 {
            length.saturating_mul(4).saturating_add(1)
        } else {
            maximum.saturating_add(1)
        };
        if size <= 0 {
            return None;
        }
        let mut buffer = vec![0 as c_char; size as usize];
        let copied = unsafe {
            CFStringGetCString(value, buffer.as_mut_ptr(), size, CF_STRING_ENCODING_UTF8)
        };
        if copied == 0 {
            return None;
        }
        // SAFETY: `CFStringGetCString` wrote a NUL-terminated UTF-8 string.
        let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        Some(text.to_string_lossy().into_owned())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    /// Read-only visibility hints from the shared-list archive.
    ///
    /// `LSSharedFileList` no longer exposes the hidden flag. The archive keeps
    /// a `visibility` value per item; a missing entry (for example a just-added
    /// favorite before Finder flushes) is treated as visible.
    pub(super) fn archived_visibility() -> Option<HashMap<PathBuf, i32>> {
        let home = NSFileManager::defaultManager()
            .homeDirectoryForCurrentUser()
            .path()?;
        let path = PathBuf::from(home.to_string()).join(
            "Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.FavoriteItems.sfl4",
        );
        let bytes = std::fs::read(&path).ok()?;
        let data = NSData::with_bytes(&bytes);
        let unarchiver = match unsafe {
            NSKeyedUnarchiver::initForReadingFromData_error(NSKeyedUnarchiver::alloc(), &data)
        } {
            Ok(unarchiver) => unarchiver,
            Err(_) => return None,
        };
        unsafe { unarchiver.setRequiresSecureCoding(false) };
        let root_key = NSString::from_str("root");
        let root = unsafe { unarchiver.decodeObjectForKey(&root_key) }?;
        let root = root.downcast_ref::<NSDictionary<AnyObject, AnyObject>>()?;
        let items_key: Retained<AnyObject> = NSString::from_str("items").into();
        let items = root.objectForKey(&*items_key)?;
        let items = items.downcast_ref::<NSArray<AnyObject>>()?;
        let visibility_key: Retained<AnyObject> = NSString::from_str("visibility").into();
        let bookmark_key: Retained<AnyObject> = NSString::from_str("Bookmark").into();
        let mut visibility = HashMap::new();
        for index in 0..items.count() {
            let item = items.objectAtIndex(index);
            let Some(item) = item.downcast_ref::<NSDictionary<AnyObject, AnyObject>>() else {
                continue;
            };
            let Some(value) = item.objectForKey(&*visibility_key) else {
                continue;
            };
            let Some(value) = value.downcast_ref::<NSNumber>() else {
                continue;
            };
            let Some(bookmark) = item.objectForKey(&*bookmark_key) else {
                continue;
            };
            let Some(bookmark) = bookmark.downcast_ref::<NSData>() else {
                continue;
            };
            let Ok(url) = (unsafe {
                NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                    bookmark,
                    NSURLBookmarkResolutionOptions::WithoutUI
                        | NSURLBookmarkResolutionOptions::WithoutMounting,
                    None,
                    std::ptr::null_mut(),
                )
            }) else {
                continue;
            };
            if let Some(path) = url.path() {
                visibility.insert(PathBuf::from(path.to_string()), value.intValue());
            }
        }
        Some(visibility)
    }

    pub(super) fn is_visible(visibility: Option<i32>) -> bool {
        visibility != Some(0)
    }

    pub(super) fn discover() -> FinderFavorites {
        FinderFavorites::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_only_hides_explicit_zero_values() {
        assert!(!super::platform::is_visible(Some(0)));
        assert!(super::platform::is_visible(Some(1)));
        assert!(super::platform::is_visible(None));
    }

    #[test]
    fn only_existing_directories_are_reported() {
        let snapshot = FinderFavoritesAdapter.discover();
        for favorite in &snapshot.favorites {
            assert!(favorite.available);
            assert!(favorite.path.is_dir());
        }
    }

    #[test]
    fn adapter_is_read_only_and_returns_a_snapshot_type() {
        let snapshot = FinderFavoritesAdapter.discover();
        assert!(snapshot.unresolved <= snapshot.favorites.len() + snapshot.unresolved);
    }
}
