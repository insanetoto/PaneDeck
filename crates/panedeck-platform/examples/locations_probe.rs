//! Read-only location discovery probe for local verification.
//!
//! Run with:
//! `cargo run -p panedeck-platform --example locations_probe`

use panedeck_platform::{LocationDiscovery, SystemLocationAdapter};

fn main() {
    let discovered = SystemLocationAdapter.discover();
    println!("locations={}", discovered.locations.len());
    for location in discovered.locations {
        println!(
            "kind={:?} source={:?} name={:?} available={} local={} removable={} path={}",
            location.kind,
            location.source,
            location.name,
            location.available,
            location.is_local,
            location.is_removable,
            location.path.display()
        );
    }
    println!("warnings={:?}", discovered.warnings);
}
