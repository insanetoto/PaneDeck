use std::{fs, path::PathBuf};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalLocationKindDto {
    Home,
    Desktop,
    Downloads,
    Documents,
    Volume,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLocationDto {
    pub id: String,
    pub kind: LocalLocationKindDto,
    pub name: String,
    pub path: String,
    pub available: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLocationsDto {
    pub locations: Vec<LocalLocationDto>,
    pub volume_scan_failed: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalLocationService;

impl LocalLocationService {
    #[must_use]
    pub fn discover(&self) -> LocalLocationsDto {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        self.discover_from(home, PathBuf::from("/Volumes"))
    }

    #[must_use]
    pub fn discover_from(&self, home: Option<PathBuf>, volumes_root: PathBuf) -> LocalLocationsDto {
        let mut locations = Vec::new();
        if let Some(home) = home.filter(|path| path.is_absolute()) {
            locations.push(location(
                "home",
                LocalLocationKindDto::Home,
                "Home",
                home.clone(),
            ));
            locations.push(location(
                "desktop",
                LocalLocationKindDto::Desktop,
                "Desktop",
                home.join("Desktop"),
            ));
            locations.push(location(
                "downloads",
                LocalLocationKindDto::Downloads,
                "Downloads",
                home.join("Downloads"),
            ));
            locations.push(location(
                "documents",
                LocalLocationKindDto::Documents,
                "Documents",
                home.join("Documents"),
            ));
        }

        let mut volume_scan_failed = false;
        match fs::read_dir(&volumes_root) {
            Ok(entries) => {
                let mut volumes = entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        let path = entry.path();
                        let metadata = fs::metadata(&path).ok()?;
                        metadata.is_dir().then(|| {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            location(
                                &format!("volume:{name}"),
                                LocalLocationKindDto::Volume,
                                &name,
                                path,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                volumes.sort_by(|left, right| left.name.cmp(&right.name));
                locations.extend(volumes);
            }
            Err(_) => volume_scan_failed = true,
        }

        LocalLocationsDto {
            locations,
            volume_scan_failed,
        }
    }
}

fn location(id: &str, kind: LocalLocationKindDto, name: &str, path: PathBuf) -> LocalLocationDto {
    let available = fs::metadata(&path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false);
    LocalLocationDto {
        id: id.to_owned(),
        kind,
        name: name.to_owned(),
        path: path.to_string_lossy().into_owned(),
        available,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn discovers_standard_locations_and_sorted_mounted_volumes() {
        let temporary = TempDir::new().unwrap();
        let home = temporary.path().join("home");
        let volumes = temporary.path().join("Volumes");
        fs::create_dir_all(home.join("Desktop")).unwrap();
        fs::create_dir_all(home.join("Downloads")).unwrap();
        fs::create_dir_all(&volumes).unwrap();
        fs::create_dir(volumes.join("Zeta")).unwrap();
        fs::create_dir(volumes.join("Alpha")).unwrap();

        let result = LocalLocationService.discover_from(Some(home), volumes);

        assert_eq!(result.locations[0].kind, LocalLocationKindDto::Home);
        assert!(result.locations[1].available);
        assert!(!result.locations[3].available);
        let volume_names = result
            .locations
            .iter()
            .filter(|location| location.kind == LocalLocationKindDto::Volume)
            .map(|location| location.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(volume_names, ["Alpha", "Zeta"]);
        assert!(!result.volume_scan_failed);
    }

    #[test]
    fn missing_home_and_unreadable_volume_root_are_non_fatal() {
        let temporary = TempDir::new().unwrap();
        let result =
            LocalLocationService.discover_from(None, temporary.path().join("missing-volumes"));
        assert!(result.locations.is_empty());
        assert!(result.volume_scan_failed);
    }
}
