use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use panedeck_platform::{
    DiscoveredLocations, LocationDiscovery, LocationKind, LocationSource, LocationWarning,
    SystemLocationAdapter,
};
use serde::Serialize;

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(30);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalLocationKindDto {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalLocationSourceDto {
    Standard,
    ICloud,
    FileProvider,
    Volume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalLocationWarningDto {
    Volumes,
    ICloud,
    CloudStorage,
    Timeout,
    RefreshFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLocationDto {
    pub id: String,
    pub kind: LocalLocationKindDto,
    pub source: LocalLocationSourceDto,
    pub name: String,
    pub path: String,
    pub available: bool,
    pub is_local: bool,
    pub is_removable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLocationsDto {
    pub locations: Vec<LocalLocationDto>,
    pub warnings: Vec<LocalLocationWarningDto>,
}

#[derive(Clone, Debug)]
struct CachedLocations {
    discovered: DiscoveredLocations,
    at: Instant,
}

/// Cached, single-flight discovery. Platform calls run on a worker thread so a
/// stalled network volume cannot freeze the UI command for longer than the
/// bounded wait; the worker may still finish and refresh the cache afterwards.
#[derive(Clone)]
pub struct LocalLocationService {
    backend: Arc<dyn LocationDiscovery + Send + Sync>,
    cache: Arc<Mutex<Option<CachedLocations>>>,
    refreshing: Arc<AtomicBool>,
    cache_ttl: Duration,
    timeout: Duration,
}

impl Default for LocalLocationService {
    fn default() -> Self {
        Self::with_backend(SystemLocationAdapter)
    }
}

impl LocalLocationService {
    #[must_use]
    pub fn with_backend<B>(backend: B) -> Self
    where
        B: LocationDiscovery + Send + Sync + 'static,
    {
        Self::with_backend_and_timeout(backend, DEFAULT_CACHE_TTL, DEFAULT_TIMEOUT)
    }

    #[must_use]
    fn with_backend_and_timeout<B>(backend: B, cache_ttl: Duration, timeout: Duration) -> Self
    where
        B: LocationDiscovery + Send + Sync + 'static,
    {
        Self {
            backend: Arc::new(backend),
            cache: Arc::new(Mutex::new(None)),
            refreshing: Arc::new(AtomicBool::new(false)),
            cache_ttl,
            timeout,
        }
    }

    /// Returns a fresh cached snapshot when available, otherwise waits up to
    /// the discovery timeout. `refresh` forces a new discovery start.
    #[must_use]
    pub fn discover(&self, refresh: bool) -> LocalLocationsDto {
        if !refresh {
            if let Some(cached) = self.fresh_cache() {
                return cached.into_dto(Vec::new());
            }
        }
        self.discover_with_background_timeout()
    }

    /// Cached data plus a warning, used when the Tauri blocking task is lost.
    #[must_use]
    pub fn snapshot_with_warning(&self, warning: LocalLocationWarningDto) -> LocalLocationsDto {
        self.cached_or_empty().into_dto(vec![warning])
    }

    fn discover_with_background_timeout(&self) -> LocalLocationsDto {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return self.cached_or_empty().into_dto(Vec::new());
        }
        let backend = Arc::clone(&self.backend);
        let cache = Arc::clone(&self.cache);
        let refreshing = Arc::clone(&self.refreshing);
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("panedeck-locations".to_owned())
            .spawn(move || {
                let discovered = backend.discover();
                let mut guard = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *guard = Some(CachedLocations {
                    discovered: discovered.clone(),
                    at: Instant::now(),
                });
                drop(guard);
                refreshing.store(false, Ordering::SeqCst);
                let _ = sender.send(discovered);
            });
        if spawned.is_err() {
            self.refreshing.store(false, Ordering::SeqCst);
            return self
                .cached_or_empty()
                .into_dto(vec![LocalLocationWarningDto::RefreshFailed]);
        }
        match receiver.recv_timeout(self.timeout) {
            Ok(discovered) => into_dto(discovered, Vec::new()),
            Err(_) => self
                .cached_or_empty()
                .into_dto(vec![LocalLocationWarningDto::Timeout]),
        }
    }

    fn fresh_cache(&self) -> Option<CachedLocations> {
        let guard = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .as_ref()
            .filter(|cached| cached.at.elapsed() < self.cache_ttl)
            .cloned()
    }

    fn cached_or_empty(&self) -> CachedLocations {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or(CachedLocations {
                discovered: DiscoveredLocations::default(),
                at: Instant::now(),
            })
    }
}

impl CachedLocations {
    fn into_dto(self, extra_warnings: Vec<LocalLocationWarningDto>) -> LocalLocationsDto {
        let mut warnings: Vec<_> = self
            .discovered
            .warnings
            .into_iter()
            .map(warning_dto)
            .collect();
        for warning in extra_warnings {
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
        LocalLocationsDto {
            locations: self
                .discovered
                .locations
                .into_iter()
                .map(|location| LocalLocationDto {
                    id: location.id,
                    kind: kind_dto(location.kind),
                    source: source_dto(location.source),
                    name: location.name,
                    path: location.path.to_string_lossy().into_owned(),
                    available: location.available,
                    is_local: location.is_local,
                    is_removable: location.is_removable,
                })
                .collect(),
            warnings,
        }
    }
}

fn into_dto(
    discovered: DiscoveredLocations,
    extra_warnings: Vec<LocalLocationWarningDto>,
) -> LocalLocationsDto {
    CachedLocations {
        discovered,
        at: Instant::now(),
    }
    .into_dto(extra_warnings)
}

const fn kind_dto(kind: LocationKind) -> LocalLocationKindDto {
    match kind {
        LocationKind::Home => LocalLocationKindDto::Home,
        LocationKind::Desktop => LocalLocationKindDto::Desktop,
        LocationKind::Downloads => LocalLocationKindDto::Downloads,
        LocationKind::Documents => LocalLocationKindDto::Documents,
        LocationKind::ICloudDrive => LocalLocationKindDto::ICloudDrive,
        LocationKind::CloudStorage => LocalLocationKindDto::CloudStorage,
        LocationKind::LocalVolume => LocalLocationKindDto::LocalVolume,
        LocationKind::ExternalVolume => LocalLocationKindDto::ExternalVolume,
        LocationKind::NetworkVolume => LocalLocationKindDto::NetworkVolume,
    }
}

const fn source_dto(source: LocationSource) -> LocalLocationSourceDto {
    match source {
        LocationSource::Standard => LocalLocationSourceDto::Standard,
        LocationSource::ICloud => LocalLocationSourceDto::ICloud,
        LocationSource::FileProvider => LocalLocationSourceDto::FileProvider,
        LocationSource::Volume => LocalLocationSourceDto::Volume,
    }
}

const fn warning_dto(warning: LocationWarning) -> LocalLocationWarningDto {
    match warning {
        LocationWarning::Volumes => LocalLocationWarningDto::Volumes,
        LocationWarning::ICloud => LocalLocationWarningDto::ICloud,
        LocationWarning::CloudStorage => LocalLocationWarningDto::CloudStorage,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use panedeck_platform::SystemLocation;

    #[derive(Clone)]
    struct StaticBackend {
        calls: Arc<AtomicUsize>,
        discovered: DiscoveredLocations,
    }

    impl LocationDiscovery for StaticBackend {
        fn discover(&self) -> DiscoveredLocations {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.discovered.clone()
        }
    }

    fn sample() -> DiscoveredLocations {
        DiscoveredLocations {
            locations: vec![SystemLocation {
                id: "cloud:one".to_owned(),
                kind: LocationKind::CloudStorage,
                source: LocationSource::FileProvider,
                name: "Provider".to_owned(),
                path: std::path::PathBuf::from("/tmp/provider"),
                available: true,
                is_local: true,
                is_removable: false,
            }],
            warnings: vec![LocationWarning::Volumes],
        }
    }

    #[test]
    fn caches_without_forcing_and_refreshes_when_requested() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = LocalLocationService::with_backend(StaticBackend {
            calls: Arc::clone(&calls),
            discovered: sample(),
        });
        let first = service.discover(false);
        assert_eq!(first.locations.len(), 1);
        assert_eq!(first.warnings, vec![LocalLocationWarningDto::Volumes]);
        let second = service.discover(false);
        assert_eq!(second.locations[0].name, "Provider");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = service.discover(true);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn returns_cached_data_with_a_timeout_warning_and_keeps_working() {
        #[derive(Clone)]
        struct SlowBackend(Arc<AtomicUsize>);
        impl LocationDiscovery for SlowBackend {
            fn discover(&self) -> DiscoveredLocations {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(80));
                sample()
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let service = LocalLocationService::with_backend_and_timeout(
            SlowBackend(Arc::clone(&calls)),
            Duration::from_secs(30),
            Duration::from_millis(10),
        );
        let timed_out = service.discover(false);
        assert!(timed_out.locations.is_empty());
        assert_eq!(timed_out.warnings, vec![LocalLocationWarningDto::Timeout]);

        std::thread::sleep(Duration::from_millis(150));
        let recovered = service.discover(false);
        assert_eq!(recovered.locations.len(), 1);
        assert_eq!(recovered.warnings, vec![LocalLocationWarningDto::Volumes]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_refreshes_keep_a_single_platform_probe() {
        #[derive(Clone)]
        struct SlowBackend(Arc<AtomicUsize>);
        impl LocationDiscovery for SlowBackend {
            fn discover(&self) -> DiscoveredLocations {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(100));
                sample()
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let service = LocalLocationService::with_backend_and_timeout(
            SlowBackend(Arc::clone(&calls)),
            Duration::from_secs(30),
            Duration::from_secs(1),
        );
        let first = {
            let service = service.clone();
            std::thread::spawn(move || service.discover(true))
        };
        std::thread::sleep(Duration::from_millis(10));
        let second = {
            let service = service.clone();
            std::thread::spawn(move || service.discover(true))
        };
        let _ = first.join().expect("first refresh");
        let _ = second.join().expect("second refresh");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn snapshot_with_warning_preserves_the_last_snapshot() {
        let service = LocalLocationService::with_backend(StaticBackend {
            calls: Arc::new(AtomicUsize::new(0)),
            discovered: sample(),
        });
        let _ = service.discover(false);
        let fallback = service.snapshot_with_warning(LocalLocationWarningDto::RefreshFailed);
        assert_eq!(fallback.locations.len(), 1);
        assert_eq!(
            fallback.warnings,
            vec![
                LocalLocationWarningDto::Volumes,
                LocalLocationWarningDto::RefreshFailed
            ]
        );
    }

    #[test]
    fn real_system_discovery_is_read_only_and_finds_the_home_entry() {
        let result = LocalLocationService::default().discover(true);
        assert!(
            result
                .locations
                .iter()
                .any(|location| location.kind == LocalLocationKindDto::Home),
            "the current HOME directory should be discoverable"
        );
        assert!(result
            .locations
            .iter()
            .all(|location| !location.id.is_empty() && location.path.starts_with('/')));
    }

    #[test]
    fn wire_dto_uses_camel_case_kinds_and_warnings() {
        let dto = LocalLocationsDto {
            locations: vec![LocalLocationDto {
                id: "icloud-drive".to_owned(),
                kind: LocalLocationKindDto::ICloudDrive,
                source: LocalLocationSourceDto::FileProvider,
                name: "iCloud Drive".to_owned(),
                path: "/tmp/icloud".to_owned(),
                available: true,
                is_local: true,
                is_removable: false,
            }],
            warnings: vec![LocalLocationWarningDto::Timeout],
        };
        let value = serde_json::to_value(dto).expect("dto serializes");
        assert_eq!(value["locations"][0]["kind"], "iCloudDrive");
        assert_eq!(value["locations"][0]["source"], "fileProvider");
        assert_eq!(value["warnings"][0], "timeout");
    }

    #[test]
    fn maps_every_kind_and_source_to_the_wire_enums() {
        assert_eq!(
            kind_dto(LocationKind::ICloudDrive),
            LocalLocationKindDto::ICloudDrive
        );
        assert_eq!(
            source_dto(LocationSource::FileProvider),
            LocalLocationSourceDto::FileProvider
        );
        assert_eq!(
            warning_dto(LocationWarning::CloudStorage),
            LocalLocationWarningDto::CloudStorage
        );
    }
}
