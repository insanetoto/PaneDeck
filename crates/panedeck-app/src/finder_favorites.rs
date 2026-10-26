use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use panedeck_platform::{
    FinderFavorites, FinderFavoritesAdapter, FinderFavoritesDiscovery, SystemFavorite,
};
use serde::Serialize;

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(30);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderFavoriteDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderFavoritesDto {
    pub favorites: Vec<FinderFavoriteDto>,
    pub unresolved: usize,
    pub failed: bool,
    pub timed_out: bool,
    pub visibility_available: bool,
}

#[derive(Clone, Debug)]
struct CachedFavorites {
    snapshot: FinderFavorites,
    at: Instant,
}

/// Single-flight, cached, read-only Finder favorites discovery.
#[derive(Clone)]
pub struct FinderFavoritesService {
    backend: Arc<dyn FinderFavoritesDiscovery + Send + Sync>,
    cache: Arc<Mutex<Option<CachedFavorites>>>,
    refreshing: Arc<AtomicBool>,
    cache_ttl: Duration,
    timeout: Duration,
}

impl Default for FinderFavoritesService {
    fn default() -> Self {
        Self::with_backend(FinderFavoritesAdapter)
    }
}

impl FinderFavoritesService {
    #[must_use]
    pub fn with_backend<B>(backend: B) -> Self
    where
        B: FinderFavoritesDiscovery + Send + Sync + 'static,
    {
        Self::with_backend_and_timeout(backend, DEFAULT_CACHE_TTL, DEFAULT_TIMEOUT)
    }

    #[must_use]
    fn with_backend_and_timeout<B>(backend: B, cache_ttl: Duration, timeout: Duration) -> Self
    where
        B: FinderFavoritesDiscovery + Send + Sync + 'static,
    {
        Self {
            backend: Arc::new(backend),
            cache: Arc::new(Mutex::new(None)),
            refreshing: Arc::new(AtomicBool::new(false)),
            cache_ttl,
            timeout,
        }
    }

    #[must_use]
    pub fn discover(&self, refresh: bool) -> FinderFavoritesDto {
        if !refresh {
            if let Some(cached) = self.fresh_cache() {
                return cached.into_dto(false);
            }
        }
        self.discover_with_background_timeout()
    }

    #[must_use]
    pub fn snapshot_with_warning(&self) -> FinderFavoritesDto {
        self.cached_or_empty().into_dto(true)
    }

    fn discover_with_background_timeout(&self) -> FinderFavoritesDto {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return self.cached_or_empty().into_dto(false);
        }
        let backend = Arc::clone(&self.backend);
        let cache = Arc::clone(&self.cache);
        let refreshing = Arc::clone(&self.refreshing);
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("panedeck-finder-favorites".to_owned())
            .spawn(move || {
                let snapshot = backend.discover();
                let mut guard = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *guard = Some(CachedFavorites {
                    snapshot: snapshot.clone(),
                    at: Instant::now(),
                });
                drop(guard);
                refreshing.store(false, Ordering::SeqCst);
                let _ = sender.send(snapshot);
            });
        if spawned.is_err() {
            self.refreshing.store(false, Ordering::SeqCst);
            return self.cached_or_empty().into_dto(true);
        }
        match receiver.recv_timeout(self.timeout) {
            Ok(snapshot) => into_dto(snapshot, false),
            Err(_) => self.cached_or_empty().into_dto(true),
        }
    }

    fn fresh_cache(&self) -> Option<CachedFavorites> {
        let guard = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .as_ref()
            .filter(|cached| cached.at.elapsed() < self.cache_ttl)
            .cloned()
    }

    fn cached_or_empty(&self) -> CachedFavorites {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or(CachedFavorites {
                snapshot: FinderFavorites::default(),
                at: Instant::now(),
            })
    }
}

impl CachedFavorites {
    fn into_dto(self, timed_out: bool) -> FinderFavoritesDto {
        FinderFavoritesDto {
            favorites: self
                .snapshot
                .favorites
                .into_iter()
                .map(favorite_dto)
                .collect(),
            unresolved: self.snapshot.unresolved,
            failed: self.snapshot.failed,
            timed_out,
            visibility_available: self.snapshot.visibility_available,
        }
    }
}

fn into_dto(snapshot: FinderFavorites, timed_out: bool) -> FinderFavoritesDto {
    CachedFavorites {
        snapshot,
        at: Instant::now(),
    }
    .into_dto(timed_out)
}

fn favorite_dto(favorite: SystemFavorite) -> FinderFavoriteDto {
    FinderFavoriteDto {
        id: favorite.id,
        name: favorite.name,
        path: favorite.path.to_string_lossy().into_owned(),
        available: favorite.available,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Clone)]
    struct StaticBackend {
        calls: Arc<AtomicUsize>,
        snapshot: FinderFavorites,
    }

    impl FinderFavoritesDiscovery for StaticBackend {
        fn discover(&self) -> FinderFavorites {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.snapshot.clone()
        }
    }

    fn sample() -> FinderFavorites {
        FinderFavorites {
            favorites: vec![SystemFavorite {
                id: "finder:/tmp/one".to_owned(),
                name: "One".to_owned(),
                path: std::path::PathBuf::from("/tmp/one"),
                available: true,
            }],
            unresolved: 2,
            failed: false,
            visibility_available: false,
        }
    }

    #[test]
    fn caches_and_maps_the_system_snapshot() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = FinderFavoritesService::with_backend(StaticBackend {
            calls: Arc::clone(&calls),
            snapshot: sample(),
        });
        let first = service.discover(false);
        assert_eq!(first.favorites[0].name, "One");
        assert_eq!(first.unresolved, 2);
        assert!(!first.failed);
        assert!(!first.timed_out);
        let _ = service.discover(false);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = service.discover(true);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn timeout_returns_the_last_snapshot_with_a_flag() {
        #[derive(Clone)]
        struct SlowBackend(Arc<AtomicUsize>);
        impl FinderFavoritesDiscovery for SlowBackend {
            fn discover(&self) -> FinderFavorites {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(80));
                sample()
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let service = FinderFavoritesService::with_backend_and_timeout(
            SlowBackend(Arc::clone(&calls)),
            Duration::from_secs(30),
            Duration::from_millis(10),
        );
        let timed_out = service.discover(false);
        assert!(timed_out.favorites.is_empty());
        assert!(timed_out.timed_out);
        std::thread::sleep(Duration::from_millis(150));
        let recovered = service.discover(false);
        assert_eq!(recovered.favorites.len(), 1);
        assert!(!recovered.timed_out);
    }

    #[test]
    fn system_favorites_are_never_written_or_renamed() {
        let service = FinderFavoritesService::with_backend(StaticBackend {
            calls: Arc::new(AtomicUsize::new(0)),
            snapshot: sample(),
        });
        let dto = service.discover(false);
        assert_eq!(dto.favorites[0].path, "/tmp/one");
        assert_eq!(dto.favorites[0].name, "One");
    }
}
