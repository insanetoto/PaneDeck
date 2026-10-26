//! Read-only Finder favorites probe.
//!
//! The deprecated `LSSharedFileList` API returns an empty list to unbundled
//! processes. Run the compiled example inside a temporary `.app` bundle to see
//! the real user list:
//! `cargo build -p panedeck-platform --example finder_favorites_probe`
//! then place `target/debug/examples/finder_favorites_probe` in an `.app` and
//! launch it with `open --args /tmp/finder-probe.txt`.

use panedeck_platform::{FinderFavoritesAdapter, FinderFavoritesDiscovery};

fn main() {
    let snapshot = FinderFavoritesAdapter.discover();
    let mut output = format!(
        "failed={} unresolved={} visibility_available={} count={}\n",
        snapshot.failed,
        snapshot.unresolved,
        snapshot.visibility_available,
        snapshot.favorites.len()
    );
    for favorite in snapshot.favorites {
        output.push_str(&format!(
            "name={:?} available={} path={}\n",
            favorite.name,
            favorite.available,
            favorite.path.display()
        ));
    }
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/panedeck-finder-probe.txt".to_owned());
    let _ = std::fs::write(&path, &output);
    print!("{output}");
}
