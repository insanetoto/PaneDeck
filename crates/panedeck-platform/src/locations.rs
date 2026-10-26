//! Read-only discovery of local, cloud and mounted-location entry points.
//!
//! Discovery only reads entry metadata. It never enumerates provider contents,
//! downloads cloud files, mounts volumes, or writes Finder state.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// User-visible category for a discovered entry point.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocationKind {
    Home,
    Desktop,
    Downloads,
    Documents,
    ICloudDrive,
    CloudStorage,
    LocalVolume,
    ExternalVolume,
    NetworkVolume,
}

/// Where an entry point came from. It is presentation metadata, never an
/// authorization to write.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocationSource {
    Standard,
    ICloud,
    FileProvider,
    Volume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemLocation {
    pub id: String,
    pub kind: LocationKind,
    pub source: LocationSource,
    pub name: String,
    pub path: PathBuf,
    pub available: bool,
    pub is_local: bool,
    pub is_removable: bool,
}

/// A partial discovery failure. Other sources remain usable.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LocationWarning {
    Volumes,
    ICloud,
    CloudStorage,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveredLocations {
    pub locations: Vec<SystemLocation>,
    pub warnings: Vec<LocationWarning>,
}

pub trait LocationDiscovery {
    fn discover(&self) -> DiscoveredLocations;
}

/// macOS adapter. It is intentionally a ZST so `LocalLocationService` can hold
/// it behind a `dyn LocationDiscovery` without platform types leaking inward.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemLocationAdapter;

impl LocationDiscovery for SystemLocationAdapter {
    fn discover(&self) -> DiscoveredLocations {
        platform::discover()
    }
}

fn standard_locations(home: Option<&Path>) -> Vec<SystemLocation> {
    let Some(home) = home else {
        return Vec::new();
    };
    [
        (LocationKind::Home, "home", "Home", ""),
        (LocationKind::Desktop, "desktop", "Desktop", "Desktop"),
        (
            LocationKind::Downloads,
            "downloads",
            "Downloads",
            "Downloads",
        ),
        (
            LocationKind::Documents,
            "documents",
            "Documents",
            "Documents",
        ),
    ]
    .into_iter()
    .map(|(kind, id, name, child)| {
        let path = if child.is_empty() {
            home.to_path_buf()
        } else {
            home.join(child)
        };
        SystemLocation {
            id: id.to_owned(),
            kind,
            source: LocationSource::Standard,
            name: name.to_owned(),
            available: path.is_dir(),
            path,
            is_local: true,
            is_removable: false,
        }
    })
    .collect()
}

fn classify_volume(is_local: bool, is_internal: bool, is_removable: bool) -> LocationKind {
    if !is_local {
        LocationKind::NetworkVolume
    } else if is_removable || !is_internal {
        LocationKind::ExternalVolume
    } else {
        LocationKind::LocalVolume
    }
}

fn dedupe_and_sort(mut locations: Vec<SystemLocation>) -> Vec<SystemLocation> {
    let mut seen = HashSet::new();
    locations.retain(|location| seen.insert(location.path.clone()));
    locations.sort_by(|left, right| {
        kind_rank(left.kind)
            .cmp(&kind_rank(right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    locations
}

const fn kind_rank(kind: LocationKind) -> u8 {
    match kind {
        LocationKind::Home => 0,
        LocationKind::Desktop => 1,
        LocationKind::Downloads => 2,
        LocationKind::Documents => 3,
        LocationKind::ICloudDrive => 4,
        LocationKind::CloudStorage => 5,
        LocationKind::LocalVolume => 6,
        LocationKind::ExternalVolume => 7,
        LocationKind::NetworkVolume => 8,
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{
        NSArray, NSDictionary, NSFileManager, NSNumber, NSString, NSURLIsDirectoryKey,
        NSURLLocalizedNameKey, NSURLResourceKey, NSURLVolumeIsBrowsableKey,
        NSURLVolumeIsInternalKey, NSURLVolumeIsLocalKey, NSURLVolumeIsRemovableKey,
        NSURLVolumeLocalizedNameKey, NSURLVolumeNameKey, NSURLVolumeUUIDStringKey,
        NSVolumeEnumerationOptions, NSURL,
    };

    pub(super) fn discover() -> DiscoveredLocations {
        let home = NSFileManager::defaultManager()
            .homeDirectoryForCurrentUser()
            .path()
            .map(|path| PathBuf::from(path.to_string()));
        let mut locations = standard_locations(home.as_deref());
        let mut warnings = Vec::new();
        if let Some(home) = home.as_deref() {
            if let Some(location) = icloud_location(home) {
                locations.push(location);
            } else if home
                .join("Library/Mobile Documents/com~apple~CloudDocs")
                .exists()
            {
                warnings.push(LocationWarning::ICloud);
            }
            cloud_storage_locations(home, &mut locations, &mut warnings);
        }
        volume_locations(&mut locations, &mut warnings);
        DiscoveredLocations {
            locations: dedupe_and_sort(locations),
            warnings,
        }
    }

    fn icloud_location(home: &Path) -> Option<SystemLocation> {
        let path = home.join("Library/Mobile Documents/com~apple~CloudDocs");
        if !path.is_dir() {
            return None;
        }
        let name = localized_name(&path).unwrap_or_else(|| "iCloud Drive".to_owned());
        Some(SystemLocation {
            id: "icloud-drive".to_owned(),
            kind: LocationKind::ICloudDrive,
            source: LocationSource::ICloud,
            name,
            path,
            available: true,
            is_local: true,
            is_removable: false,
        })
    }

    fn cloud_storage_locations(
        home: &Path,
        locations: &mut Vec<SystemLocation>,
        warnings: &mut Vec<LocationWarning>,
    ) {
        let root = home.join("Library/CloudStorage");
        if !root.is_dir() {
            return;
        }
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(_) => {
                warnings.push(LocationWarning::CloudStorage);
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };
            if file_name.is_empty() {
                continue;
            }
            let name = localized_name(&path).unwrap_or_else(|| file_name.to_owned());
            locations.push(SystemLocation {
                id: format!("cloud:{file_name}"),
                kind: LocationKind::CloudStorage,
                source: LocationSource::FileProvider,
                name,
                path,
                available: true,
                is_local: true,
                is_removable: false,
            });
        }
    }

    fn volume_locations(locations: &mut Vec<SystemLocation>, warnings: &mut Vec<LocationWarning>) {
        // SAFETY: Foundation exports these as immutable process-wide constants.
        let (
            name_key,
            localized_key,
            local_key,
            internal_key,
            removable_key,
            browsable_key,
            uuid_key,
            directory_key,
        ) = unsafe {
            (
                NSURLVolumeNameKey,
                NSURLVolumeLocalizedNameKey,
                NSURLVolumeIsLocalKey,
                NSURLVolumeIsInternalKey,
                NSURLVolumeIsRemovableKey,
                NSURLVolumeIsBrowsableKey,
                NSURLVolumeUUIDStringKey,
                NSURLIsDirectoryKey,
            )
        };
        let keys = NSArray::from_slice(&[
            name_key,
            localized_key,
            local_key,
            internal_key,
            removable_key,
            browsable_key,
            uuid_key,
            directory_key,
        ]);
        let Some(urls) = NSFileManager::defaultManager()
            .mountedVolumeURLsIncludingResourceValuesForKeys_options(
                Some(&keys),
                NSVolumeEnumerationOptions::SkipHiddenVolumes,
            )
        else {
            warnings.push(LocationWarning::Volumes);
            return;
        };
        for index in 0..urls.count() {
            let url = urls.objectAtIndex(index);
            let Some(path) = url.path().map(|value| PathBuf::from(value.to_string())) else {
                continue;
            };
            let Some(values) = resource_values(&url, &keys) else {
                continue;
            };
            let browsable = bool_value(&values, browsable_key).unwrap_or(true);
            let available = bool_value(&values, directory_key).unwrap_or_else(|| path.is_dir());
            if !browsable || !available {
                continue;
            }
            let name = string_value(&values, localized_key)
                .or_else(|| string_value(&values, name_key))
                .unwrap_or_else(|| {
                    path.file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        .unwrap_or("Volume")
                        .to_owned()
                });
            let is_local = bool_value(&values, local_key).unwrap_or(true);
            let is_internal = bool_value(&values, internal_key).unwrap_or(true);
            let is_removable = bool_value(&values, removable_key).unwrap_or(false);
            let id = string_value(&values, uuid_key)
                .map(|uuid| format!("volume:{uuid}"))
                .unwrap_or_else(|| format!("volume:{}", path.to_string_lossy()));
            locations.push(SystemLocation {
                id,
                kind: classify_volume(is_local, is_internal, is_removable),
                source: LocationSource::Volume,
                name,
                path,
                available,
                is_local,
                is_removable,
            });
        }
    }

    fn localized_name(path: &Path) -> Option<String> {
        // SAFETY: Foundation exports this as an immutable process-wide constant.
        let localized_key = unsafe { NSURLLocalizedNameKey };
        let url = NSURL::from_file_path(path)?;
        let keys = NSArray::from_slice(&[localized_key]);
        let values = resource_values(&url, &keys)?;
        string_value(&values, localized_key)
    }

    fn resource_values(
        url: &NSURL,
        keys: &NSArray<NSURLResourceKey>,
    ) -> Option<Retained<NSDictionary<NSURLResourceKey, AnyObject>>> {
        url.resourceValuesForKeys_error(keys).ok()
    }

    fn string_value(
        values: &NSDictionary<NSURLResourceKey, AnyObject>,
        key: &NSURLResourceKey,
    ) -> Option<String> {
        values
            .objectForKey(key)?
            .downcast_ref::<NSString>()
            .map(ToString::to_string)
    }

    fn bool_value(
        values: &NSDictionary<NSURLResourceKey, AnyObject>,
        key: &NSURLResourceKey,
    ) -> Option<bool> {
        values
            .objectForKey(key)?
            .downcast_ref::<NSNumber>()
            .map(|value| value.boolValue())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub(super) fn discover() -> DiscoveredLocations {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        DiscoveredLocations {
            locations: dedupe_and_sort(standard_locations(home.as_deref())),
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(
        id: &str,
        kind: LocationKind,
        name: &str,
        path: &str,
        available: bool,
    ) -> SystemLocation {
        SystemLocation {
            id: id.to_owned(),
            kind,
            source: match kind {
                LocationKind::ICloudDrive => LocationSource::ICloud,
                LocationKind::CloudStorage => LocationSource::FileProvider,
                LocationKind::LocalVolume
                | LocationKind::ExternalVolume
                | LocationKind::NetworkVolume => LocationSource::Volume,
                _ => LocationSource::Standard,
            },
            name: name.to_owned(),
            path: PathBuf::from(path),
            available,
            is_local: kind != LocationKind::NetworkVolume,
            is_removable: false,
        }
    }

    #[test]
    fn classifies_local_external_and_network_volumes() {
        assert_eq!(
            classify_volume(true, true, false),
            LocationKind::LocalVolume
        );
        assert_eq!(
            classify_volume(true, false, false),
            LocationKind::ExternalVolume
        );
        assert_eq!(
            classify_volume(true, true, true),
            LocationKind::ExternalVolume
        );
        assert_eq!(
            classify_volume(false, true, false),
            LocationKind::NetworkVolume
        );
    }

    #[test]
    fn keeps_the_first_entry_for_a_shared_path_and_orders_sources() {
        let locations = vec![
            location(
                "network",
                LocationKind::NetworkVolume,
                "NAS",
                "/Volumes/nas",
                true,
            ),
            location(
                "documents",
                LocationKind::Documents,
                "Documents",
                "/home/me/Documents",
                true,
            ),
            location(
                "volume",
                LocationKind::LocalVolume,
                "Macintosh HD",
                "/",
                true,
            ),
            location(
                "duplicate",
                LocationKind::ExternalVolume,
                "Duplicate",
                "/Volumes/nas",
                true,
            ),
            location(
                "icloud",
                LocationKind::ICloudDrive,
                "iCloud Drive",
                "/home/me/iCloud",
                true,
            ),
            location(
                "cloud",
                LocationKind::CloudStorage,
                "Provider",
                "/home/me/Cloud/Provider",
                true,
            ),
        ];
        let sorted = dedupe_and_sort(locations);
        let names: Vec<_> = sorted.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Documents",
                "iCloud Drive",
                "Provider",
                "Macintosh HD",
                "NAS"
            ]
        );
        assert!(sorted.iter().all(|item| item.id != "duplicate"));
    }

    #[test]
    fn standard_locations_keep_unavailable_entries_with_an_explicit_flag() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let locations = standard_locations(Some(temp.path()));
        assert_eq!(locations.len(), 4);
        assert!(locations[0].available);
        assert!(!locations[1].available);
        assert!(locations
            .iter()
            .all(|item| item.source == LocationSource::Standard));
    }
}
